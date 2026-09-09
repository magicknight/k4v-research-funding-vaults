"""Read-only, single-bank RPC export for the bounded E-05 review graph.

The caller supplies a reviewed address map, expected genesis hash and optional
minimum slot. One getMultipleAccounts response contains all accounts and Clock.
No wallet, signing, deployment, token purchase or transaction method exists.
An RPC server's response is an observation, not a cryptographic chain proof.
"""
import argparse
import base64
import json
from pathlib import Path
import struct
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from beneficiary_vault_verifier import _base58_encode, _pubkey
from e05_verifier import CLOCK, VAULT, verify_graph
from launch_v4_verifier import require

MAX_RESPONSE = 3_000_000


def canonical(value):
    require(isinstance(value, str) and _base58_encode(_pubkey(value)) == value, "RPC_PUBLIC_KEY")
    return value


class Client:
    def __init__(self, endpoint):
        url = urlparse(endpoint)
        require(url.scheme == "https" or (url.scheme == "http" and url.hostname in ("localhost", "127.0.0.1", "::1")), "RPC_TRANSPORT")
        require(bool(url.netloc), "RPC_ENDPOINT")
        self.endpoint, self.sequence = endpoint, 0

    def call(self, method, params):
        require(method in ("getGenesisHash", "getMultipleAccounts"), "READ_ONLY_RPC")
        self.sequence += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params}).encode()
        request = Request(self.endpoint, data=body, headers={"Content-Type": "application/json"}, method="POST")
        try:
            with urlopen(request, timeout=15) as response:
                payload = response.read(MAX_RESPONSE + 1)
        except (OSError, TimeoutError) as error:
            raise ValueError("RPC_REQUEST_FAILED") from error
        require(len(payload) <= MAX_RESPONSE, "RPC_RESPONSE_TOO_LARGE")
        try:
            decoded = json.loads(payload)
        except (UnicodeError, ValueError) as error:
            raise ValueError("RPC_JSON") from error
        require(isinstance(decoded, dict) and decoded.get("jsonrpc") == "2.0"
                and type(decoded.get("id")) is int and decoded["id"] == self.sequence
                and "error" not in decoded and "result" in decoded, "RPC_ENVELOPE")
        return decoded["result"]


def export(client, addresses, expected_genesis, min_context_slot=0):
    canonical(expected_genesis)
    require(type(min_context_slot) is int and 0 <= min_context_slot < 2**64, "RPC_MIN_SLOT")
    require(isinstance(addresses, dict) and 1 <= len(addresses) <= 100, "RPC_ADDRESS_COUNT")
    require(addresses.get("clock") == CLOCK and len(set(addresses.values())) == len(addresses), "RPC_ADDRESS_GRAPH")
    for value in addresses.values():
        canonical(value)
    require(client.call("getGenesisHash", []) == expected_genesis, "RPC_GENESIS_MISMATCH")
    response = client.call("getMultipleAccounts", [list(addresses.values()),
        {"encoding": "base64", "commitment": "finalized", "minContextSlot": min_context_slot}])
    require(isinstance(response, dict) and isinstance(response.get("context"), dict), "RPC_CONTEXT")
    slot = response["context"].get("slot")
    values = response.get("value")
    require(type(slot) is int and min_context_slot <= slot < 2**64, "RPC_CONTEXT_SLOT")
    require(isinstance(values, list) and len(values) == len(addresses), "RPC_VALUE_COUNT")
    accounts = {}
    for (name, address), raw in zip(addresses.items(), values):
        require(isinstance(raw, dict), "RPC_MISSING_ACCOUNT")
        owner = canonical(raw.get("owner"))
        require(type(raw.get("executable")) is bool and type(raw.get("lamports")) is int
                and 0 < raw["lamports"] < 2**64, "RPC_ACCOUNT_ENVELOPE")
        encoded = raw.get("data")
        require(isinstance(encoded, list) and len(encoded) == 2 and encoded[1] == "base64"
                and isinstance(encoded[0], str), "RPC_DATA_ENCODING")
        data = base64.b64decode(encoded[0], validate=True)
        require(base64.b64encode(data).decode() == encoded[0], "RPC_NONCANONICAL_BASE64")
        accounts[name] = {"address": address, "owner": owner, "executable": raw["executable"], "data_hex": data.hex()}
    clock = bytes.fromhex(accounts["clock"]["data_hex"])
    require(len(clock) == 40 and struct.unpack_from("<Q", clock)[0] == slot, "RPC_CLOCK_BANK_MISMATCH")
    now = struct.unpack_from("<q", clock, 32)[0]
    snapshot = {"schema": "K4V-LAUNCH-V4-RAW-SNAPSHOT-v1", "program_id": VAULT,
                "scope": "SUPPLIED_RPC_RESPONSE", "private_keys_serialized": False,
                "now": str(now), "accounts": accounts}
    return {"snapshot": snapshot, "verification": verify_graph(snapshot),
            "provenance": {"genesis_hash": expected_genesis, "context_slot": str(slot),
                           "requested_commitment": "finalized", "single_account_bank": True,
                           "rpc_server_trust_required": True, "chain_proof_verified": False}}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--addresses", type=Path, required=True, help="JSON name-to-address map, including all change tombstones and Clock")
    parser.add_argument("--expected-genesis-hash", required=True)
    parser.add_argument("--min-context-slot", type=int, default=0)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = export(Client(args.rpc_url), json.loads(args.addresses.read_text()), args.expected_genesis_hash, args.min_context_slot)
    except (ValueError, KeyError, TypeError, struct.error) as error:
        print(json.dumps({"valid": False, "error": str(error)}))
        raise SystemExit(1) from error
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print("RPC observation exported and supplied graph verified; chain proof and human review remain unverified")
