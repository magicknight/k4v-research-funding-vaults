# SPDX-License-Identifier: MIT OR Apache-2.0
"""Independent, standard-library verifier for a B1 beneficiary-vault snapshot.

The verifier does not sign transactions or control funds. It recomputes the
two Solana PDAs and checks the frozen accounting invariants in an exported JSON
snapshot. ``beneficiary_vault_rpc_exporter.py`` supplies the read-only path
from authenticated RPC account envelopes and raw bytes into this schema.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
from pathlib import Path
import sys
from typing import Any


SNAPSHOT_SCHEMA = "beneficiary-vault-snapshot/v0.1"
VERIFICATION_SCHEMA = "beneficiary-vault-verification/v0.1"
BPS_DENOMINATOR = 10_000
MONTHS_PER_YEAR = 12
MAX_ANNUAL_RELEASE_BPS = 500
MIN_CLIFF_SECONDS = 730 * 24 * 60 * 60
PERIOD_SECONDS = 30 * 24 * 60 * 60
STATE_SEED = b"beneficiary-vault"
TOKEN_VAULT_SEED = b"beneficiary-token"
PDA_MARKER = b"ProgramDerivedAddress"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


@dataclass(frozen=True)
class Verification:
    valid: bool
    reasons: tuple[str, ...]
    expected_state_pda: str | None
    expected_token_vault_pda: str | None
    expected_monthly_cap: int | None
    observed_period_index: int | None
    currently_releasable_amount: int


def _base58_decode(value: str) -> bytes:
    if not isinstance(value, str) or not value:
        raise ValueError("base58 value must be a non-empty string")
    number = 0
    for character in value:
        try:
            digit = BASE58_ALPHABET.index(character)
        except ValueError as exc:
            raise ValueError("invalid base58 character") from exc
        number = number * 58 + digit
    payload = number.to_bytes((number.bit_length() + 7) // 8, "big") if number else b""
    return b"\x00" * (len(value) - len(value.lstrip("1"))) + payload


def _base58_encode(value: bytes) -> str:
    number = int.from_bytes(value, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    return "1" * (len(value) - len(value.lstrip(b"\x00"))) + (encoded or "")


def _pubkey(value: str) -> bytes:
    decoded = _base58_decode(value)
    if len(decoded) != 32:
        raise ValueError("Solana public key must decode to 32 bytes")
    return decoded


def _is_ed25519_point(compressed: bytes) -> bool:
    """Match Solana's rejection of PDA hashes that decompress on Ed25519."""

    if len(compressed) != 32:
        return False
    field = 2**255 - 19
    y = int.from_bytes(compressed, "little") & ((1 << 255) - 1)
    sign = compressed[31] >> 7
    if y >= field:
        return False
    d = (-121665 * pow(121666, field - 2, field)) % field
    y_squared = y * y % field
    denominator = (d * y_squared + 1) % field
    if denominator == 0:
        return False
    x_squared = (y_squared - 1) * pow(denominator, field - 2, field) % field
    x = pow(x_squared, (field + 3) // 8, field)
    if x * x % field != x_squared:
        x = x * pow(2, (field - 1) // 4, field) % field
    if x * x % field != x_squared:
        return False
    return not (x == 0 and sign == 1)


def find_program_address(seeds: tuple[bytes, ...], program_id: bytes) -> tuple[bytes, int]:
    if len(program_id) != 32:
        raise ValueError("program id must be 32 bytes")
    if len(seeds) >= 16 or any(len(seed) > 32 for seed in seeds):
        raise ValueError("invalid PDA seed dimensions")
    for bump in range(255, -1, -1):
        candidate = hashlib.sha256(
            b"".join((*seeds, bytes([bump]), program_id, PDA_MARKER))
        ).digest()
        if not _is_ed25519_point(candidate):
            return candidate, bump
    raise ValueError("no viable PDA bump")


def _integer(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{name} must be an integer")
    return value


def _canonical_sha256(value: dict[str, Any]) -> str:
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def verify_snapshot(snapshot: dict[str, Any]) -> Verification:
    reasons: list[str] = []
    expected_state_pda: str | None = None
    expected_token_vault_pda: str | None = None
    expected_cap: int | None = None
    observed_period: int | None = None
    releasable = 0

    if snapshot.get("schema") != SNAPSHOT_SCHEMA:
        reasons.append("SCHEMA_MISMATCH")

    try:
        state = snapshot["state"]
        token = snapshot["token_account"]
        program_id = _pubkey(snapshot["program_id"])
        beneficiary = _pubkey(state["beneficiary"])
        mint = _pubkey(state["mint"])
        policy_hash = bytes.fromhex(state["policy_hash"])
        if len(policy_hash) != 32:
            raise ValueError("policy hash must be 32 bytes")

        state_pda, state_bump = find_program_address(
            (STATE_SEED, beneficiary, mint, policy_hash), program_id
        )
        expected_state_pda = _base58_encode(state_pda)
        token_pda, token_bump = find_program_address(
            (TOKEN_VAULT_SEED, state_pda), program_id
        )
        expected_token_vault_pda = _base58_encode(token_pda)

        if snapshot["vault_state"] != expected_state_pda:
            reasons.append("STATE_PDA_MISMATCH")
        if snapshot["vault_token_account"] != expected_token_vault_pda:
            reasons.append("TOKEN_VAULT_PDA_MISMATCH")
        if state["state_bump"] != state_bump:
            reasons.append("STATE_BUMP_MISMATCH")
        if state["token_vault_bump"] != token_bump:
            reasons.append("TOKEN_VAULT_BUMP_MISMATCH")
        if policy_hash == bytes(32):
            reasons.append("ZERO_POLICY_HASH")

        deposited = _integer(state["deposited_amount"], "deposited_amount")
        annual_bps = _integer(state["annual_release_bps"], "annual_release_bps")
        stored_cap = _integer(state["monthly_cap"], "monthly_cap")
        released_total = _integer(state["released_total"], "released_total")
        released_period = _integer(
            state["released_this_period"], "released_this_period"
        )
        stored_period = _integer(state["current_period_index"], "current_period_index")
        genesis = _integer(state["genesis_ts"], "genesis_ts")
        cliff_end = _integer(state["cliff_end_ts"], "cliff_end_ts")
        observed_at = _integer(snapshot["observed_at_ts"], "observed_at_ts")
        token_amount = _integer(token["amount"], "token amount")

        if deposited <= 0:
            reasons.append("INVALID_DEPOSIT")
        if not 1 <= annual_bps <= MAX_ANNUAL_RELEASE_BPS:
            reasons.append("INVALID_ANNUAL_RELEASE_RATE")
        expected_cap = (deposited * annual_bps // BPS_DENOMINATOR) // MONTHS_PER_YEAR
        if stored_cap != expected_cap or expected_cap <= 0:
            reasons.append("MONTHLY_CAP_MISMATCH")
        if cliff_end - genesis < MIN_CLIFF_SECONDS:
            reasons.append("CLIFF_TOO_SHORT")
        if min(released_total, released_period, stored_period, token_amount) < 0:
            reasons.append("NEGATIVE_ACCOUNTING_VALUE")
        if released_period > stored_cap:
            reasons.append("PERIOD_CAP_EXCEEDED")
        if released_total > deposited:
            reasons.append("DEPOSIT_EXCEEDED")
        if token_amount + released_total != deposited:
            reasons.append("TOKEN_CONSERVATION_FAILURE")
        if token.get("mint") != state["mint"]:
            reasons.append("TOKEN_MINT_MISMATCH")
        if token.get("authority") != snapshot.get("vault_state"):
            reasons.append("TOKEN_AUTHORITY_MISMATCH")
        if token.get("state", "initialized") != "initialized":
            reasons.append("TOKEN_ACCOUNT_NOT_ACTIVE")
        if token.get("delegate") is not None:
            reasons.append("TOKEN_DELEGATE_PRESENT")
        if token.get("delegated_amount", 0) != 0:
            reasons.append("TOKEN_DELEGATED_AMOUNT_PRESENT")
        if token.get("close_authority") is not None:
            reasons.append("TOKEN_CLOSE_AUTHORITY_PRESENT")
        if token.get("is_native", False):
            reasons.append("NATIVE_TOKEN_ACCOUNT_UNSUPPORTED")

        if observed_at < genesis:
            reasons.append("OBSERVATION_BEFORE_GENESIS")
        elif observed_at < cliff_end:
            observed_period = None
            if released_total != 0:
                reasons.append("RELEASE_BEFORE_CLIFF")
        else:
            observed_period = (observed_at - cliff_end) // PERIOD_SECONDS
            if stored_period > observed_period:
                reasons.append("PERIOD_FROM_FUTURE")
            elif stored_period < observed_period:
                releasable = min(stored_cap, deposited - released_total)
            else:
                releasable = min(stored_cap - released_period, deposited - released_total)
    except (KeyError, TypeError, ValueError, OverflowError):
        reasons.append("MALFORMED_SNAPSHOT")

    return Verification(
        valid=not reasons,
        reasons=tuple(dict.fromkeys(reasons)),
        expected_state_pda=expected_state_pda,
        expected_token_vault_pda=expected_token_vault_pda,
        expected_monthly_cap=expected_cap,
        observed_period_index=observed_period,
        currently_releasable_amount=max(0, releasable),
    )


def verification_receipt(snapshot: dict[str, Any]) -> dict[str, Any]:
    verification = verify_snapshot(snapshot)
    body = {
        "schema": VERIFICATION_SCHEMA,
        "snapshot": snapshot,
        "verification": asdict(verification),
    }
    return {**body, "sha256": _canonical_sha256(body)}


def main(argv: list[str] | None = None) -> int:
    arguments = sys.argv[1:] if argv is None else argv
    if len(arguments) != 1:
        print("usage: beneficiary_vault_verifier.py SNAPSHOT.json", file=sys.stderr)
        return 2
    try:
        snapshot = json.loads(Path(arguments[0]).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"could not read snapshot: {exc}", file=sys.stderr)
        return 2
    receipt = verification_receipt(snapshot)
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0 if receipt["verification"]["valid"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
