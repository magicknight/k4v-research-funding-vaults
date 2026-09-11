#!/usr/bin/env python3
"""Recheck published account/journal bytes. No RPC, transaction or private ledger read."""
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from e11b_verifier import CODE_SHA256, verify_graph
from launch_v7_rpc_exporter import bind_identity, validate_manifest
from launch_v7_verifier import read_policy, require
from soak_journal import Journal, read_json


def verify(directory):
    root = Path(directory)
    receipt, manifest = read_json(root / "acceptance.json"), read_json(root / "manifest.json")
    require(receipt["schema"] == "K4V-PERSISTENT-SOAK-ACCEPTANCE-v1", "SCHEMA")
    require(receipt["program_sha256"] == CODE_SHA256, "PROGRAM_HASH")
    for key in ("clock_override", "policy_token_account_injection", "natural_90_180_day_soak",
                "continuous_bootstrap_to_maturity_history", "production_ready", "independent_human_review"):
        require(receipt[key] is False, "UNSUPPORTED_SCOPE_" + key)
    require(receipt["public_chain_transactions"] == 0, "PUBLIC_TRANSACTION_CLAIM")
    validate_manifest(manifest)
    require(manifest["expected"]["genesis_hash"] == receipt["genesis_hash"], "GENESIS_BINDING")
    labels = ["before-restart", "after-restart", "before-backup", "after-restore", "after-restored-transaction"]
    require([x["label"] for x in receipt["observations"]] == labels, "OBSERVATION_ORDER")
    snapshots, policies = {}, {}
    for item in receipt["observations"]:
        name = "observation-" + item["label"] + ".json"
        require(item["path"] == name, "OBSERVATION_PATH")
        raw = (root / name).read_bytes()
        require(hashlib.sha256(raw).hexdigest() == item["sha256"], "OBSERVATION_HASH")
        snapshot = read_json(root / name)["snapshot"]
        verify_graph(snapshot)
        bind_identity(snapshot, manifest["expected"])
        require(str(snapshot["slot"]) == str(item["slot"]) and str(snapshot["now"]) == str(item["now"]), "OBSERVATION_BANK")
        snapshots[item["label"]] = snapshot
        policies[item["label"]] = read_policy(snapshot)
    for before, after in [("before-restart", "after-restart"), ("before-backup", "after-restore")]:
        a, b = snapshots[before], snapshots[after]
        require(int(b["slot"]) >= int(a["slot"]) and int(b["now"]) >= int(a["now"]), "RESTART_TIME_REGRESSION")
        require({k: v for k, v in a["accounts"].items() if k != "clock"}
                == {k: v for k, v in b["accounts"].items() if k != "clock"}, "RESTART_ACCOUNT_CHANGE")
    require([policies[label]["report_sequence"] for label in labels] == [1, 1, 2, 2, 3], "REPORT_SEQUENCE")
    transactions = read_json(root / "signed-transactions.json")
    require(len(transactions) == receipt["finalized_client_transactions"] == 13, "TRANSACTION_COUNT")
    require(len({t["signature"] for t in transactions}) == 13, "DUPLICATE_TRANSACTION")
    for label, count in [("same-ledger", 11), ("restored", 12), ("final", 13)]:
        history = read_json(root / ("history-" + label + ".json"))
        require(history["bounds"] == {"minimum_ledger_slot": 0, "first_available_block": 0}, "PRUNED_HISTORY")
        require(len(history["statuses"]) == count, "HISTORY_COUNT")
        for tx, status in zip(transactions[:count], history["statuses"]):
            require(status["signature"] == tx["signature"] and status["err"] is None
                    and status["confirmationStatus"] == "finalized" and status["slot"] == tx["result"]["slot"], "HISTORY_STATUS")
    journal = Journal(root / "journal", receipt["journal"]["head_sha256"]).summary()
    require(journal["samples"] == 3 and journal["observer_sessions"] == 3
            and journal["observation_errors"] == 0 and journal["unclosed_observer_sessions"] == 0, "JOURNAL_SUMMARY")
    require(journal["anomalies"] == [], "JOURNAL_ANOMALIES")
    require([x["event"] for x in receipt["lifecycle"]] == ["START", "STOP"] * 3, "LIFECYCLE")
    return {"valid": True, "mode": "OFFLINE_RECORDED_BYTES_RECHECK", "raw_account_checkpoints": 5,
            "recorded_finalized_transactions": 13, "journal_samples": 3,
            "rpc_observed_by_this_command": False, "private_ledger_verified_by_this_command": False,
            "natural_90_180_day_soak": False, "production_ready": False, "independent_human_review": False}


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify_soak_persistent.py PUBLIC_EVIDENCE_DIRECTORY")
    print(json.dumps(verify(sys.argv[1]), indent=2))
