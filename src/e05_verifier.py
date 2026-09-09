"""Pinned E-04 loader bytes plus independent v4 accounting/governance review.

This accepts supplied raw accounts, never a transaction receipt as proof of
chain history. Pins are review inputs in this source, not fields in a snapshot.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import struct
import zlib

from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address
from launch_v4_verifier import Reader, account, read_policy, require, verify, NOTICE

VAULT = "5h5iUez8fpHThaQhDdyUSQab9ngRmG2zfMBNx5bGnB9Q"
GATE = "5JVsAM5AnTBdeHsGjC78hcrrv9KfLKVgCNN4RzBiWjmA"
LOADER = "BPFLoaderUpgradeab1e11111111111111111111111"
CLOCK = "SysvarC1ock11111111111111111111111111111111"
SYSVAR = "Sysvar1111111111111111111111111111111111111"
PINS = {
    "vault-test": (423448, "f1f7929478f4d8d51ab46398f79a352c99699c38afb905d3d4589d0522355d43"),
    "vault-default": (408568, "54accf5620943fdd75cff13679d9e2e5a83fc37093e9c898e2e09d3fe44d5b9c"),
    "gate-test": (276136, "76c094b8ecac318fca485a4e70fb1c6d3a8754c7938123e5432314b6d07aed30"),
}
MAX_BLOB = 2_000_000
CHECKPOINTS = ("pending_oracle_upgrade", "notice_minus_one", "before_oracle_recovery",
               "after_oracle_recovery", "before_upgrade", "after_upgrade",
               "continued_after_upgrade", "controller_notice_minus_one",
               "before_controller_recovery", "after_controller_recovery",
               "continued_after_controller_recovery", "annual_boundary")


def pda(seeds, program):
    key, bump = find_program_address(tuple(seeds), _pubkey(program))
    return _base58_encode(key), bump


def loader_account(snapshot, name, executable):
    a = snapshot["accounts"][name]
    require(a["owner"] == LOADER and a["executable"] is executable, "LOADER_ENVELOPE")
    require(_base58_encode(_pubkey(a["address"])) == a["address"], "LOADER_ADDRESS")
    return a["address"], bytes.fromhex(a["data_hex"])


def matched_code(data, names, padded):
    for name in names:
        size, digest = PINS[name]
        if (len(data) >= size if padded else len(data) == size):
            if hashlib.sha256(data[:size]).hexdigest() == digest:
                require(not any(data[size:]), "NONZERO_PROGRAM_PADDING")
                return name
    raise ValueError("UNREVIEWED_PROGRAM_BYTES")


def program_state(snapshot, prefix, program, names):
    address, raw = loader_account(snapshot, prefix + "_program", True)
    data_address, data = loader_account(snapshot, prefix + "_programdata", False)
    expected, _ = pda([_pubkey(program)], LOADER)
    require(address == program and data_address == expected, "PROGRAMDATA_PDA")
    require(len(raw) == 36 and raw[:4] == struct.pack("<I", 2)
            and raw[4:] == _pubkey(expected), "LOADER_PROGRAM_STATE")
    require(len(data) >= 45 and data[:4] == struct.pack("<I", 3), "LOADER_PROGRAMDATA_STATE")
    require(data[12] in (0, 1), "LOADER_AUTHORITY_OPTION")
    # None uses 13 serialized bytes; the fixed 45-byte metadata allocation can
    # retain old authority bytes after sealing. Those are not executable code.
    authority = _base58_encode(data[13:45]) if data[12] else None
    profile = matched_code(data[45:], names, True)
    slot = struct.unpack_from("<Q", data, 4)[0]
    return {"program": address, "programdata": expected, "authority": authority,
            "profile": profile, "last_deploy_slot": slot,
            "code_sha256": PINS[profile][1], "code_bytes": PINS[profile][0]}


def verify_loader(snapshot):
    now = int(snapshot["now"])
    target = program_state(snapshot, "target", VAULT, ("vault-test", "vault-default"))
    gate_program = program_state(snapshot, "gate", GATE, ("gate-test",))
    require(gate_program["authority"] is None, "MUTABLE_UPGRADE_GATE")
    address, r = account(snapshot, "upgrade_gate", GATE, "UpgradeGateV1")
    g = {"target": r.key(), "programdata": r.key(), "members": [r.key() for _ in range(3)],
         "nonce": r.number(), "status": r.number("B"), "buffer": r.key(),
         "code_hash": r.take(32).hex(), "code_len": r.number(), "return_authority": r.key(),
         "created_at": r.number("q"), "execute_after": r.number("q"),
         "last_action_at": r.number("q"), "bump": r.number("B")}
    r.finish()
    expected, bump = pda([b"upgrade-gate-v1", _pubkey(VAULT)], GATE)
    require(address == expected and g["bump"] == bump and g["target"] == VAULT
            and g["programdata"] == target["programdata"], "GATE_PDA_TARGET")
    require(target["authority"] == address, "TARGET_AUTHORITY_BYPASS")
    require(len(set(g["members"])) == 3, "UPGRADE_MEMBERS")
    require(g["status"] in (0, 1, 2, 3) and g["last_action_at"] <= now, "GATE_STATUS_CLOCK")
    candidate = None
    if g["status"] == 0:
        require(g["nonce"] == g["code_len"] == g["created_at"] == g["execute_after"] == 0
                and _pubkey(g["buffer"]) == _pubkey(g["return_authority"]) == bytes(32)
                and g["code_hash"] == "00" * 32, "EMPTY_GATE")
    else:
        require(g["nonce"] > 0 and g["code_len"] > 0
                and g["created_at"] <= g["last_action_at"]
                and g["execute_after"] == g["created_at"] + NOTICE, "UPGRADE_NOTICE")
        if g["status"] == 1:
            key, raw = loader_account(snapshot, "upgrade_buffer", False)
            require(key == g["buffer"] and len(raw) == 37 + g["code_len"]
                    and raw[:5] == struct.pack("<IB", 1, 1)
                    and raw[5:37] == _pubkey(address), "LOCKED_BUFFER")
            require(hashlib.sha256(raw[37:]).hexdigest() == g["code_hash"], "BUFFER_HASH")
            candidate = matched_code(raw[37:], ("vault-test", "vault-default"), False)
        elif g["status"] == 2:
            require(g["execute_after"] <= g["last_action_at"], "UPGRADE_EXECUTED_EARLY")
            require((g["code_len"], g["code_hash"]) == PINS[target["profile"]], "EXECUTED_CODE_BINDING")
        # A cancelled buffer is returned to its owner, who may write or close it.
        # Its current bytes are not evidence of the cancelled candidate's bytes.
    clock_address, clock = account(snapshot, "clock", SYSVAR)
    require(clock_address == CLOCK and len(clock.data) == 40, "CLOCK_ACCOUNT")
    slot, epoch_start, epoch, leader_epoch, timestamp = struct.unpack("<QqQQq", clock.data)
    require(timestamp == now, "CLOCK_TIMESTAMP")
    require(target["last_deploy_slot"] <= slot and gate_program["last_deploy_slot"] <= slot, "LOADER_FUTURE_SLOT")
    return {"target": target, "gate_program": gate_program, "gate_account": address,
            "members": g["members"], "required_signatures": 2, "nonce": str(g["nonce"]),
            "status": ("empty", "pending", "executed", "cancelled")[g["status"]],
            "execute_after": str(g["execute_after"]),
            "pending_mature": g["status"] == 1 and now >= g["execute_after"],
            "pending_profile": candidate, "return_authority": g["return_authority"],
            "observed_slot": str(slot), "committee_loss_recovery_supported": False,
            "future_upgrade_semantics_guaranteed": False}


def verify_graph(snapshot):
    result = verify(snapshot)
    result["loader"] = verify_loader(snapshot)
    result["program_bytes_verified"] = True
    result["scope"] = "SUPPLIED_PINNED_TEST_CANDIDATE_GRAPH_ONLY"
    return result


def expand_bundle(bundle):
    require(bundle["schema"] == "K4V-E05-REHEARSAL-BUNDLE-v1", "BUNDLE_SCHEMA")
    require(len(bundle["blobs"]) <= 512 and len(bundle["checkpoints"]) <= 64, "BUNDLE_SIZE")
    blobs = {}
    total_bytes = 0
    for digest, encoded in bundle["blobs"].items():
        compressed = base64.b64decode(encoded, validate=True)
        d = zlib.decompressobj()
        raw = d.decompress(compressed, MAX_BLOB + 1)
        require(len(raw) <= MAX_BLOB and d.eof and not d.unused_data
                and not d.unconsumed_tail, "BLOB_SIZE_OR_STREAM")
        require(hashlib.sha256(raw).hexdigest() == digest, "BLOB_HASH")
        total_bytes += len(raw)
        require(total_bytes <= 8_000_000, "BUNDLE_RAW_SIZE")
        blobs[digest] = raw.hex()
    checkpoints = []
    for item in bundle["checkpoints"]:
        s = {k: v for k, v in item.items() if k != "accounts"}
        s["accounts"] = {name: {**a, "data_hex": blobs[a["blob"]]}
                         for name, a in item["accounts"].items()}
        checkpoints.append(s)
    return checkpoints


def verify_bundle(bundle):
    snapshots = expand_bundle(bundle)
    require(tuple(s["label"] for s in snapshots) == CHECKPOINTS, "REHEARSAL_CHECKPOINT_SEQUENCE")
    results = {s["label"]: verify_graph(s) for s in snapshots}
    policies = [read_policy(s) for s in snapshots]
    require(policies and len({p["identity"] for p in policies}) == 1, "REHEARSAL_IDENTITY_CHANGED")
    for before, after in zip(policies, policies[1:]):
        require(before["last_action_at"] <= after["last_action_at"], "REHEARSAL_CLOCK_ROLLBACK")
        for key in ("founder_released_total", "treasury_released_total", "report_sequence",
                    "change_sequence", "oracle_epoch", "controller_epoch"):
            require(before[key] <= after[key], "REHEARSAL_COUNTER_ROLLBACK")
    require(all(int(s["now"]) <= int(t["now"]) for s, t in zip(snapshots, snapshots[1:])), "CHECKPOINT_CLOCK_ROLLBACK")
    by_label = {s["label"]: s for s in snapshots}
    # Exact before/after account comparison across a real change of ELF bytes.
    before, after = by_label["before_upgrade"], by_label["after_upgrade"]
    names = set(before["accounts"]) - {"clock", "upgrade_gate", "upgrade_buffer", "target_programdata"}
    require(all(before["accounts"][name] == after["accounts"][name] for name in names), "UPGRADE_MUTATED_LIVE_STATE")
    require(results["before_upgrade"]["loader"]["target"]["profile"] == "vault-test"
            and results["after_upgrade"]["loader"]["target"]["profile"] == "vault-default", "REHEARSAL_UPGRADE_PROFILES")
    for pre, post in (("before_oracle_recovery", "after_oracle_recovery"),
                      ("before_controller_recovery", "after_controller_recovery")):
        a, b = read_policy(by_label[pre]), read_policy(by_label[post])
        stable = ("identity", "config", "mint", "founder", "treasury", "period", "shared_used",
                  "founder_period_used", "treasury_period_used", "founder_released_total",
                  "treasury_released_total", "annual_index", "founder_annual_used", "treasury_annual_used")
        require(all(a[k] == b[k] for k in stable), "RECOVERY_RESET_ACCOUNTING")
        for name in ("founder_vault", "treasury_vault", "founder_token", "treasury_token", "approval"):
            require(by_label[pre]["accounts"][name] == by_label[post]["accounts"][name], "RECOVERY_CHANGED_CUSTODY_OR_NOTICE")
    require(results["after_oracle_recovery"]["amount_ceilings_before_transaction_signatures"] == ["0", "0"], "OLD_REPORT_SURVIVED_RECOVERY")
    require(results["annual_boundary"]["limits"]["annual_index"] == "1", "ANNUAL_BOUNDARY_NOT_EXERCISED")
    require(results["notice_minus_one"]["loader"]["pending_mature"] is False
            and results["before_upgrade"]["loader"]["pending_mature"] is True, "UPGRADE_NOTICE_BOUNDARY")
    require(results["controller_notice_minus_one"]["governance"]["pending"]["mature"] is False,
            "CONTROLLER_NOTICE_BOUNDARY")
    for pre, post in (("after_upgrade", "continued_after_upgrade"),
                      ("after_controller_recovery", "continued_after_controller_recovery")):
        a, b = read_policy(by_label[pre]), read_policy(by_label[post])
        require(all(b[k] > a[k] for k in ("founder_released_total", "treasury_released_total")),
                "POST_CHANGE_RELEASES_MISSING")
    return {"valid": True, "scope": "AUTHOR_RUN_LOCAL_REHEARSAL_SUPPLIED_BYTES",
            "checkpoints_verified": len(snapshots), "results": results,
            "on_chain_authenticity_verified": False, "transaction_signatures_verified": False,
            "independent_human_audit": False, "production_ready": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.input.read_text())
        result = verify_bundle(data) if "blobs" in data else verify_graph(data)
    except (ValueError, KeyError, TypeError, struct.error, zlib.error) as error:
        print(json.dumps({"valid": False, "error": str(error)}))
        raise SystemExit(1) from error
    print(json.dumps(result, indent=2))
