# SPDX-License-Identifier: MIT OR Apache-2.0
"""Read and authenticate a B1 beneficiary vault through Solana JSON-RPC.

This module is deliberately read-only. It accepts a program id and state PDA,
checks the RPC account envelopes and binary layouts, derives the token-vault
PDA, and emits the normalized snapshot consumed by
``beneficiary_vault_verifier.py``. It never requests a wallet or sends a
transaction.
"""

from __future__ import annotations

import argparse
import base64
import binascii
from dataclasses import dataclass
import hashlib
import json
import struct
import sys
from typing import Any, Protocol
from urllib import error as urlerror
from urllib import parse as urlparse
from urllib import request as urlrequest

from beneficiary_vault_verifier import (
    SNAPSHOT_SCHEMA,
    STATE_SEED,
    TOKEN_VAULT_SEED,
    _base58_encode,
    _pubkey,
    find_program_address,
    verify_snapshot,
)


TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
CLOCK_SYSVAR_ID = "SysvarC1ock11111111111111111111111111111111"
SYSVAR_PROGRAM_ID = "Sysvar1111111111111111111111111111111111111"
STATE_DISCRIMINATOR = hashlib.sha256(b"account:BeneficiaryVault").digest()[:8]
STATE_ACCOUNT_DATA_LEN = 197
TOKEN_ACCOUNT_DATA_LEN = 165
CLOCK_ACCOUNT_DATA_LEN = 40
MAX_RPC_RESPONSE_BYTES = 2_000_000


class RpcExportError(ValueError):
    """A stable, non-secret-bearing RPC export failure."""


@dataclass(frozen=True)
class RawAccount:
    owner: str
    data: bytes
    executable: bool
    lamports: int


class AccountReader(Protocol):
    def get_account_info(self, address: str) -> RawAccount: ...


def _canonical_pubkey(value: str, name: str) -> tuple[str, bytes]:
    try:
        decoded = _pubkey(value)
    except (TypeError, ValueError) as exc:
        raise RpcExportError(f"{name} is not a valid Solana public key") from exc
    canonical = _base58_encode(decoded)
    if canonical != value:
        raise RpcExportError(f"{name} is not canonically encoded")
    return canonical, decoded


class JsonRpcClient:
    def __init__(self, rpc_url: str, *, timeout_seconds: float = 10.0):
        parsed = urlparse.urlparse(rpc_url)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise RpcExportError("RPC URL must use http or https")
        self._rpc_url = rpc_url
        self._timeout_seconds = timeout_seconds
        self._request_id = 0

    def _call(self, method: str, params: list[Any]) -> Any:
        self._request_id += 1
        request_id = self._request_id
        body = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            },
            separators=(",", ":"),
        ).encode("utf-8")
        request = urlrequest.Request(
            self._rpc_url,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlrequest.urlopen(request, timeout=self._timeout_seconds) as response:
                payload = response.read(MAX_RPC_RESPONSE_BYTES + 1)
        except (OSError, TimeoutError, urlerror.URLError) as exc:
            raise RpcExportError("RPC request failed") from exc
        if len(payload) > MAX_RPC_RESPONSE_BYTES:
            raise RpcExportError("RPC response exceeded the size limit")
        try:
            decoded = json.loads(payload)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise RpcExportError("RPC response was not valid JSON") from exc
        if not isinstance(decoded, dict) or decoded.get("id") != request_id:
            raise RpcExportError("RPC response envelope was invalid")
        if decoded.get("error") is not None:
            raise RpcExportError("RPC returned an error")
        if "result" not in decoded:
            raise RpcExportError("RPC response omitted result")
        return decoded["result"]

    def get_account_info(self, address: str) -> RawAccount:
        result = self._call(
            "getAccountInfo",
            [address, {"encoding": "base64", "commitment": "confirmed"}],
        )
        if not isinstance(result, dict) or not isinstance(result.get("value"), dict):
            raise RpcExportError("RPC account was absent or malformed")
        value = result["value"]
        owner = value.get("owner")
        executable = value.get("executable")
        lamports = value.get("lamports")
        encoded = value.get("data")
        if not isinstance(owner, str):
            raise RpcExportError("RPC account owner was malformed")
        _canonical_pubkey(owner, "account owner")
        if not isinstance(executable, bool):
            raise RpcExportError("RPC executable flag was malformed")
        if isinstance(lamports, bool) or not isinstance(lamports, int) or lamports < 0:
            raise RpcExportError("RPC lamports value was malformed")
        if (
            not isinstance(encoded, list)
            or len(encoded) != 2
            or encoded[1] != "base64"
            or not isinstance(encoded[0], str)
        ):
            raise RpcExportError("RPC account data was not canonical base64")
        try:
            data = base64.b64decode(encoded[0], validate=True)
        except (ValueError, binascii.Error) as exc:
            raise RpcExportError("RPC account data contained invalid base64") from exc
        return RawAccount(owner=owner, data=data, executable=executable, lamports=lamports)


def _require_program_data(account: RawAccount, owner: str, length: int, name: str) -> bytes:
    if account.owner != owner:
        raise RpcExportError(f"{name} owner mismatch")
    if account.executable:
        raise RpcExportError(f"{name} must not be executable")
    if len(account.data) != length:
        raise RpcExportError(f"{name} data length mismatch")
    return account.data


def _pubkey_at(data: bytes, offset: int) -> str:
    return _base58_encode(data[offset : offset + 32])


def decode_vault_state(data: bytes) -> dict[str, Any]:
    if len(data) != STATE_ACCOUNT_DATA_LEN:
        raise RpcExportError("vault state data length mismatch")
    if data[:8] != STATE_DISCRIMINATOR:
        raise RpcExportError("vault state discriminator mismatch")
    offset = 8
    depositor = _pubkey_at(data, offset)
    offset += 32
    beneficiary = _pubkey_at(data, offset)
    offset += 32
    mint = _pubkey_at(data, offset)
    offset += 32
    policy_hash = data[offset : offset + 32].hex()
    offset += 32
    (
        deposited_amount,
        monthly_cap,
        released_total,
        released_this_period,
        genesis_ts,
        cliff_end_ts,
        current_period_index,
        annual_release_bps,
        mint_decimals,
        state_bump,
        token_vault_bump,
    ) = struct.unpack_from("<QQQQqqQHBBB", data, offset)
    return {
        "depositor": depositor,
        "beneficiary": beneficiary,
        "mint": mint,
        "policy_hash": policy_hash,
        "deposited_amount": deposited_amount,
        "monthly_cap": monthly_cap,
        "released_total": released_total,
        "released_this_period": released_this_period,
        "genesis_ts": genesis_ts,
        "cliff_end_ts": cliff_end_ts,
        "current_period_index": current_period_index,
        "annual_release_bps": annual_release_bps,
        "mint_decimals": mint_decimals,
        "state_bump": state_bump,
        "token_vault_bump": token_vault_bump,
    }


def _coption_pubkey(data: bytes, offset: int, name: str) -> str | None:
    option = struct.unpack_from("<I", data, offset)[0]
    if option == 0:
        return None
    if option == 1:
        return _pubkey_at(data, offset + 4)
    raise RpcExportError(f"token account {name} option was malformed")


def decode_token_account(data: bytes) -> dict[str, Any]:
    if len(data) != TOKEN_ACCOUNT_DATA_LEN:
        raise RpcExportError("vault token account data length mismatch")
    account_state = data[108]
    if account_state not in {1, 2}:
        raise RpcExportError("vault token account was not initialized")
    is_native_option = struct.unpack_from("<I", data, 109)[0]
    if is_native_option not in {0, 1}:
        raise RpcExportError("token account native option was malformed")
    return {
        "mint": _pubkey_at(data, 0),
        "authority": _pubkey_at(data, 32),
        "amount": struct.unpack_from("<Q", data, 64)[0],
        "delegate": _coption_pubkey(data, 72, "delegate"),
        "state": "initialized" if account_state == 1 else "frozen",
        "is_native": is_native_option == 1,
        "delegated_amount": struct.unpack_from("<Q", data, 121)[0],
        "close_authority": _coption_pubkey(data, 129, "close authority"),
    }


def decode_clock_timestamp(data: bytes) -> int:
    if len(data) != CLOCK_ACCOUNT_DATA_LEN:
        raise RpcExportError("clock sysvar data length mismatch")
    return struct.unpack_from("<q", data, 32)[0]


def export_snapshot(
    reader: AccountReader, program_id: str, vault_state_address: str
) -> dict[str, Any]:
    canonical_program, program_bytes = _canonical_pubkey(program_id, "program id")
    canonical_state, state_address_bytes = _canonical_pubkey(
        vault_state_address, "vault state"
    )
    state_account = reader.get_account_info(canonical_state)
    state_data = _require_program_data(
        state_account, canonical_program, STATE_ACCOUNT_DATA_LEN, "vault state"
    )
    state = decode_vault_state(state_data)

    beneficiary = _pubkey(state["beneficiary"])
    mint = _pubkey(state["mint"])
    policy_hash = bytes.fromhex(state["policy_hash"])
    expected_state, state_bump = find_program_address(
        (STATE_SEED, beneficiary, mint, policy_hash), program_bytes
    )
    if expected_state != state_address_bytes or state["state_bump"] != state_bump:
        raise RpcExportError("vault state PDA reconciliation failed")
    token_address_bytes, token_bump = find_program_address(
        (TOKEN_VAULT_SEED, state_address_bytes), program_bytes
    )
    if state["token_vault_bump"] != token_bump:
        raise RpcExportError("vault token PDA bump reconciliation failed")
    token_address = _base58_encode(token_address_bytes)
    token_account = reader.get_account_info(token_address)
    token_data = _require_program_data(
        token_account, TOKEN_PROGRAM_ID, TOKEN_ACCOUNT_DATA_LEN, "vault token account"
    )
    token = decode_token_account(token_data)

    clock_account = reader.get_account_info(CLOCK_SYSVAR_ID)
    clock_data = _require_program_data(
        clock_account, SYSVAR_PROGRAM_ID, CLOCK_ACCOUNT_DATA_LEN, "clock sysvar"
    )
    snapshot = {
        "schema": SNAPSHOT_SCHEMA,
        "program_id": canonical_program,
        "vault_state": canonical_state,
        "vault_token_account": token_address,
        "observed_at_ts": decode_clock_timestamp(clock_data),
        "state": state,
        "token_account": token,
    }
    return snapshot


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Export and verify a B1 vault snapshot from read-only JSON-RPC"
    )
    parser.add_argument("--rpc-url", default="http://127.0.0.1:8899")
    parser.add_argument("--program-id", required=True)
    parser.add_argument("--vault-state", required=True)
    args = parser.parse_args(argv)
    try:
        snapshot = export_snapshot(
            JsonRpcClient(args.rpc_url), args.program_id, args.vault_state
        )
        verification = verify_snapshot(snapshot)
    except RpcExportError as exc:
        print(f"RPC export failed: {exc}", file=sys.stderr)
        return 1
    if not verification.valid:
        print(
            "RPC snapshot failed verification: " + ",".join(verification.reasons),
            file=sys.stderr,
        )
        return 1
    print(json.dumps(snapshot, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
