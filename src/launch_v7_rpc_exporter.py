"""Read-only v7 export: explicit identity, complete history, one final bank.

Discovery responses only select addresses. All accepted bytes, including Clock,
are read again in one finalized getMultipleAccounts response (maximum 100).
This observes an RPC server; it does not authenticate a public chain or audit.
"""
import argparse
import base64
import hashlib
from http.client import HTTPException
import json
from pathlib import Path
import struct
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener

from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address
from e11b_verifier import CLOCK, LOADER, SYSVAR, verify_graph
from launch_v7_verifier import PROGRAM, account, read_policy, require

MAX_RESPONSE = 3_000_000
MAX_ACCOUNTS = 100
MANIFEST_SCHEMA = "K4V-V7-RPC-REVIEW-MANIFEST-v1"


def canonical(value):
    require(isinstance(value, str) and 32 <= len(value) <= 44, "RPC_PUBLIC_KEY")
    require(_base58_encode(_pubkey(value)) == value, "RPC_PUBLIC_KEY")
    return value


def strict_json(payload):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "RPC_JSON_DUPLICATE_KEY")
            result[key] = value
        return result

    def nonfinite(_):
        raise ValueError("RPC_JSON_NONFINITE")

    try:
        return json.loads(payload, object_pairs_hook=pairs, parse_constant=nonfinite)
    except (UnicodeError, ValueError, RecursionError) as error:
        raise ValueError("RPC_JSON") from error


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("RPC_REDIRECT_REFUSED")


class Client:
    def __init__(self, endpoint):
        require(isinstance(endpoint, str), "RPC_ENDPOINT")
        try:
            url = urlparse(endpoint)
            port = url.port
            require(url.scheme == "https" or (url.scheme == "http" and url.hostname in
                    ("localhost", "127.0.0.1", "::1")), "RPC_TRANSPORT")
            require(bool(url.hostname) and url.username is None and url.password is None
                    and not url.fragment and (port is None or port > 0)
                    and not any(ord(c) < 33 for c in endpoint), "RPC_ENDPOINT")
        except ValueError as error:
            raise ValueError("RPC_ENDPOINT_OR_TRANSPORT") from error
        self.endpoint, self.sequence = endpoint, 0
        self.opener = build_opener(NoRedirect())

    def call(self, method, params):
        require(method in ("getGenesisHash", "getMultipleAccounts"), "READ_ONLY_RPC")
        self.sequence += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.sequence,
                           "method": method, "params": params}).encode()
        request = Request(self.endpoint, data=body, headers={"Content-Type": "application/json"}, method="POST")
        try:
            with self.opener.open(request, timeout=15) as response:
                payload = response.read(MAX_RESPONSE + 1)
        except (OSError, ValueError, TimeoutError, HTTPException) as error:
            raise ValueError("RPC_REQUEST_FAILED") from error
        require(len(payload) <= MAX_RESPONSE, "RPC_RESPONSE_TOO_LARGE")
        decoded = strict_json(payload)
        require(isinstance(decoded, dict) and decoded.get("jsonrpc") == "2.0"
                and type(decoded.get("id")) is int and decoded["id"] == self.sequence
                and "error" not in decoded and "result" in decoded, "RPC_ENVELOPE")
        return decoded["result"]


def pda(*seeds, program=PROGRAM):
    return _base58_encode(find_program_address(seeds, _pubkey(program))[0])


def validate_manifest(manifest):
    require(isinstance(manifest, dict) and set(manifest) ==
            {"schema", "expected", "external_accounts", "approval_periods"}, "RPC_MANIFEST_FIELDS")
    require(manifest["schema"] == MANIFEST_SCHEMA, "RPC_MANIFEST_SCHEMA")
    expected = manifest["expected"]
    keys = {"genesis_hash", "program_id", "policy", "mint", "creator", "founder", "treasury", "initial_oracle"}
    require(isinstance(expected, dict) and set(expected) == keys | {"identity_sha256", "spec_sha256"}, "RPC_EXPECTED_FIELDS")
    for key in keys:
        canonical(expected[key])
    require(expected["program_id"] == PROGRAM, "RPC_EXPECTED_PROGRAM")
    for key in ("identity_sha256", "spec_sha256"):
        value = expected[key]
        require(isinstance(value, str) and len(value) == 64
                and all(c in "0123456789abcdef" for c in value), "RPC_EXPECTED_HASH")
    require(expected["policy"] == pda(b"launch-v7-policy", bytes.fromhex(expected["identity_sha256"])), "RPC_EXPECTED_POLICY_PDA")
    external = manifest["external_accounts"]
    require(isinstance(external, dict), "RPC_EXTERNAL_ACCOUNTS")
    count = len(external) - 2
    require(1 <= count <= 8 and set(external) == {"source", "treasury_destination"} |
            {f"founder_destination_{i}" for i in range(count)}, "RPC_EXTERNAL_ACCOUNTS")
    for value in external.values():
        canonical(value)
    periods = manifest["approval_periods"]
    require(isinstance(periods, list) and len(periods) <= 64
            and all(type(n) is int and 0 <= n < 2**64 for n in periods)
            and periods == sorted(set(periods)), "RPC_APPROVAL_PERIODS")
    policy_key = _pubkey(expected["policy"])
    addresses = {"policy": expected["policy"], "mint": expected["mint"],
                 "program": PROGRAM, "program_data": pda(_pubkey(PROGRAM), program=LOADER), "clock": CLOCK}
    for role, name in enumerate(("founder", "treasury")):
        vault = pda(b"launch-v7-vault", policy_key, bytes([role]))
        addresses[name + "_vault"] = vault
        addresses[name + "_token"] = pda(b"launch-v7-token", _pubkey(vault))
    addresses["preparation"] = pda(b"launch-v7-preparation", bytes.fromhex(expected["identity_sha256"]))
    addresses.update(external)
    check_addresses(addresses)
    return addresses


def check_addresses(addresses):
    require(1 <= len(addresses) <= MAX_ACCOUNTS, "RPC_COMPLETE_GRAPH_EXCEEDS_100")
    require(len(set(addresses.values())) == len(addresses) and addresses.get("clock") == CLOCK, "RPC_ACCOUNT_ALIAS_OR_CLOCK")


def read_accounts(client, addresses, minimum):
    check_addresses(addresses)
    response = client.call("getMultipleAccounts", [list(addresses.values()),
        {"encoding": "base64", "commitment": "finalized", "minContextSlot": minimum}])
    require(isinstance(response, dict) and isinstance(response.get("context"), dict), "RPC_CONTEXT")
    slot = response["context"].get("slot")
    require(type(slot) is int and minimum <= slot < 2**64, "RPC_CONTEXT_SLOT")
    values = response.get("value")
    require(isinstance(values, list) and len(values) == len(addresses), "RPC_VALUE_COUNT")
    accounts = {}
    for (name, address), raw in zip(addresses.items(), values):
        require(isinstance(raw, dict), "RPC_MISSING_ACCOUNT")
        owner = canonical(raw.get("owner"))
        require(type(raw.get("executable")) is bool and type(raw.get("lamports")) is int
                and 0 < raw["lamports"] < 2**64, "RPC_ACCOUNT_ENVELOPE")
        encoded = raw.get("data")
        require(isinstance(encoded, list) and len(encoded) == 2 and encoded[1] == "base64"
                and isinstance(encoded[0], str) and len(encoded[0]) <= 2_666_668, "RPC_DATA_ENCODING")
        data = base64.b64decode(encoded[0], validate=True)
        require(len(data) <= 2_000_000 and base64.b64encode(data).decode() == encoded[0], "RPC_NONCANONICAL_BASE64")
        accounts[name] = {"address": address, "owner": owner, "executable": raw["executable"],
                          "lamports": str(raw["lamports"]), "data_hex": data.hex()}
    clock = accounts["clock"]
    data = bytes.fromhex(clock["data_hex"])
    require(clock["owner"] == SYSVAR and clock["executable"] is False and len(data) == 40
            and struct.unpack_from("<Q", data)[0] == slot, "RPC_CLOCK_BANK_MISMATCH")
    return {"schema": "K4V-LAUNCH-V7-RAW-SNAPSHOT-v1", "program_id": PROGRAM,
            "scope": "SUPPLIED_RPC_RESPONSE", "private_keys_serialized": False,
            "slot": str(slot), "now": str(struct.unpack_from("<q", data, 32)[0]), "accounts": accounts}


def bind_identity(snapshot, expected):
    p = read_policy(snapshot)
    require(p["address"] == expected["policy"] and p["identity"].hex() == expected["identity_sha256"]
            and p["spec_hash"].hex() == expected["spec_sha256"]
            and all(p[k] == expected[k] for k in ("mint", "creator", "founder", "treasury", "initial_oracle")), "RPC_POLICY_IDENTITY_MISMATCH")
    return p


def history_addresses(p, periods):
    require(p["approval_count"] == len(periods), "RPC_COMPLETE_APPROVAL_PERIODS_REQUIRED")
    sequences = [p["change_sequence"]] + [s["sequence"] for s in p["withdrawal"]]
    require(all(n <= 256 for n in sequences), "RPC_HISTORY_PROFILE_LIMIT")
    key = _pubkey(p["address"])
    result = {f"change_{n}": pda(b"launch-v7-change", key, struct.pack("<Q", n))
              for n in range(1, sequences[0] + 1)}
    for role in range(2):
        result.update({f"withdrawal_{role}_{n}": pda(b"launch-v7-withdraw", key, bytes([role]), struct.pack("<Q", n))
                       for n in range(1, sequences[role + 1] + 1)})
    result.update({f"approval_{n}": pda(b"launch-v7-approval", key, struct.pack("<Q", n)) for n in periods})
    return result


def key_addresses(discovery, history, policy):
    # Discovery only reads fixed-width subjects. Final verify_graph validates
    # every proposal/PDA/enum/epoch/notice and reconstructs the persistent masks.
    subjects = set()
    for name in history:
        if name.startswith("withdrawal_"):
            _, r = account(discovery, name, PROGRAM, "WithdrawalProposalV7")
            require(len(r.data) == 172, "RPC_WITHDRAWAL_LAYOUT")
            subjects.add(_base58_encode(r.data[89:121]))
        elif name.startswith("approval_"):
            _, r = account(discovery, name, PROGRAM, "TreasuryApprovalV7")
            require(len(r.data) == 177, "RPC_APPROVAL_LAYOUT")
            subjects.add(_base58_encode(r.data[80:112]))
    return {"key_" + k: pda(b"launch-v7-key", _pubkey(policy), _pubkey(k)) for k in sorted(subjects)}


def unchanged(before, after, names):
    # Lamport donations do not change the graph. Raw state and ownership do.
    require(all(all(before["accounts"][n][k] == after["accounts"][n][k]
                    for k in ("address", "owner", "executable", "data_hex")) for n in names),
            "RPC_DISCOVERY_CHANGED_RESTART_REQUIRED")


def export(client, manifest, min_context_slot=0):
    require(type(min_context_slot) is int and 0 <= min_context_slot < 2**64, "RPC_MIN_SLOT")
    addresses = validate_manifest(manifest)
    expected = manifest["expected"]
    require(client.call("getGenesisHash", []) == expected["genesis_hash"], "RPC_GENESIS_MISMATCH")
    first = read_accounts(client, {"policy": addresses["policy"], "clock": CLOCK}, min_context_slot)
    p = bind_identity(first, expected)
    history = history_addresses(p, manifest["approval_periods"])
    addresses.update(history)
    check_addresses(addresses)  # Do not spend requests on a graph already over the limit.
    discovery = read_accounts(client, {"policy": addresses["policy"], "clock": CLOCK, **history}, int(first["slot"]))
    unchanged(first, discovery, ("policy",))
    addresses.update(key_addresses(discovery, history, addresses["policy"]))
    check_addresses(addresses)  # Keys count toward the same 100-account limit.
    final = read_accounts(client, addresses, int(discovery["slot"]))
    unchanged(discovery, final, ("policy", *history))
    bind_identity(final, expected)
    verification = verify_graph(final)
    require(client.call("getGenesisHash", []) == expected["genesis_hash"], "RPC_GENESIS_CHANGED")
    digest = hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"schema": "K4V-V7-RPC-OBSERVATION-v1", "snapshot": final, "verification": verification,
            "provenance": {"expected_genesis_hash": expected["genesis_hash"], "manifest_sha256": digest,
                           "discovery_slots": [first["slot"], discovery["slot"]], "context_slot": final["slot"],
                           "requested_commitment": "finalized", "final_account_count": len(addresses),
                           "single_final_response": True, "discovery_bytes_used_in_snapshot": False,
                           "rpc_server_trust_required": True, "chain_proof_verified": False,
                           "live_public_deployment_verified": False, "independent_human_audit": False}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--manifest", type=Path, required=True, help="Reviewed explicit network, identity, external accounts and all approval periods")
    parser.add_argument("--min-context-slot", type=int, default=0)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        manifest = strict_json(args.manifest.read_bytes())
        result = export(Client(args.rpc_url), manifest, args.min_context_slot)
        # No output is created on verification failure; never overwrite earlier evidence.
        with args.output.open("x") as output:
            output.write(json.dumps(result, indent=2) + "\n")
    except (OSError, ValueError, KeyError, TypeError, struct.error) as error:
        reason = str(error)
        reason = reason if reason.isupper() and all(c.isupper() or c.isdigit() or c == "_" for c in reason) else "RPC_EXPORT_FAILED"
        print(json.dumps({"valid": False, "error": reason}))
        return 1
    print("Supplied RPC bytes verified; public-chain authenticity and human review remain unverified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
