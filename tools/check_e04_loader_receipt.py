#!/usr/bin/env python3
"""Check a local receipt's loader headers and frozen artifact coherence.
This does not authenticate signatures, transaction history, code semantics or RPC.
"""
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from beneficiary_vault_verifier import _base58_decode, _base58_encode, find_program_address

candidate = json.loads((ROOT / "spec/E04_TEST_ONLY_CANDIDATE_v1.json").read_text())
path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "evidence/UPGRADE_GATE_LOCAL_REAL_LOADER_2026-09-09.json"
receipt = json.loads(path.read_text())
assert receipt["schema"] == "K4V-UPGRADE-GATE-LOCAL-REAL-LOADER-v1"
assert receipt["public_chain_transactions"] == 0
assert receipt["program_injection_used"] is False
assert receipt["private_keys_serialized"] is False
assert receipt["accountable_human_audit"] is False
loader = _base58_decode("BPFLoaderUpgradeab1e11111111111111111111111")
target = _base58_decode(candidate["vault_program_id"])
gate_program = _base58_decode(candidate["gate_program_id"])
gate = find_program_address((b"upgrade-gate-v1", target), gate_program)[0]
assert receipt["gate_account"] == _base58_encode(gate)
for label, program, authority in (("gate", gate_program, None), ("target", target, gate)):
    account = receipt[label]
    assert _base58_decode(account["program"]) == program
    assert _base58_decode(account["programdata"]) == find_program_address((program,), loader)[0]
    assert _base58_decode(account["owner"]) == loader and account["executable"] is False
    raw = bytes.fromhex(account["loader_header_hex"])
    assert len(raw) == 45 and struct.unpack_from("<I", raw)[0] == 3 and account["data_len"] > 45
    assert raw[12] in (0, 1)
    decoded = raw[13:45] if raw[12] == 1 else None
    assert decoded == authority
    assert account["authority"] == (_base58_encode(authority) if authority is not None else None)
artifacts = {item["path"]: item for item in candidate["artifacts"]}
for field, artifact in [
    ("gate_code_hash", "target/gate-test/upgrade_gate_v1.so"),
    ("before_code_hash", "target/v4-disabled/launch_vault_v4.so"),
    ("after_code_hash", "target/v4-test/launch_vault_v4.so"),
]:
    assert receipt[field] == artifacts[artifact]["sha256"]
assert receipt["after_code_bytes"] == artifacts["target/v4-test/launch_vault_v4.so"]["bytes"]
assert receipt["execute_after"] - receipt["created_at"] == candidate["upgrade_notice_seconds"]
assert receipt["early_execute_rejected_at"] == receipt["execute_after"] - 1
assert receipt["execute_succeeded_at"] >= receipt["execute_after"]
assert receipt["proposal_nonce"] > 0
assert receipt["one_committee_key_absent"] and receipt["permissionless_execute"]
assert 0 < receipt["successful_transactions"] < receipt["signed_transactions_sent"]
for field in ("proposal_signature", "execute_signature"):
    assert len(_base58_decode(receipt[field])) == 64
print(json.dumps({
    "valid": True,
    "scope": "LOCAL_RECEIPT_HEADER_AND_ARTIFACT_COHERENCE_ONLY",
    "authenticates_public_chain": False,
    "authenticates_transaction_history": False,
    "independent_human_audit": False,
}))
