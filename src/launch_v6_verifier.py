"""Independent v6 raw-account decoder, governance reconstruction and accounting.
No Rust execution, generated IDL, RPC access or summary fields are used. The
active dual-pool, single-depositor graph and complete change tombstones are
required. This checks supplied bytes, not their chain provenance or signatures.
Use e10_verifier for additional pinned loader-byte and whole-rehearsal checks.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address

PROGRAM = "FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU"
TOKEN = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
PERIOD, CLIFF, U64 = 2_592_000, 15_552_000, 2**64 - 1


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def take(self, count):
        require(self.pos + count <= len(self.data), "TRUNCATED_ACCOUNT")
        result = self.data[self.pos:self.pos + count]
        self.pos += count
        return result

    def number(self, kind="Q"):
        return struct.unpack("<" + kind, self.take(struct.calcsize("<" + kind)))[0]

    def key(self):
        return _base58_encode(self.take(32))

    def finish(self):
        require(self.pos == len(self.data), "TRAILING_ACCOUNT_BYTES")


def account(snapshot, name, owner, typename=None):
    a = snapshot["accounts"][name]
    require(_base58_encode(_pubkey(a["address"])) == a["address"], "NONCANONICAL_KEY")
    require(a["owner"] == owner and a["executable"] is False, "ACCOUNT_OWNER_OR_EXECUTABLE")
    r = Reader(bytes.fromhex(a["data_hex"]))
    if typename:
        require(r.take(8) == hashlib.sha256(("account:" + typename).encode()).digest()[:8], "DISCRIMINATOR")
    return a["address"], r


def read_config(r):
    c = {}
    for key in ("t0", "founder_amount", "treasury_amount", "founder_period_cap",
                "treasury_period_cap", "shared_hard_cap", "max_report_age"):
        c[key] = r.number("q" if key in ("t0", "max_report_age") else "Q")
    c["annual_rules"] = []
    for _ in range(2):
        rule = {key: r.number() for key in ("start_period", "end_period", "founder_basis", "treasury_basis", "shared_cap")}
        rule["release_bps"] = r.number("H")
        rule["source_hash"] = r.take(32).hex()
        c["annual_rules"].append(rule)
    return c


def read_policy(snapshot):
    address, r = account(snapshot, "policy", PROGRAM, "LaunchPolicyV6")
    p = {key: r.key() for key in ("creator", "mint", "founder", "treasury", "oracle")}
    p["identity"], p["spec_hash"] = r.take(32), r.take(32)
    start = r.pos
    p["config"] = read_config(r)
    p["config"]["recovery_keys"] = [r.key() for _ in range(3)]
    p["config"]["founder_recovery_keys"] = [r.key() for _ in range(3)]
    p["config"]["treasury_recovery_keys"] = [r.key() for _ in range(3)]
    config_bytes = r.data[start:r.pos]
    p.update({key: r.number("B") for key in ("state", "funded_mask", "bump")})
    for key in ("last_action_at", "period", "shared_used", "report_period", "report_capacity", "report_at", "report_sequence"):
        p[key] = r.number("q" if key in ("last_action_at", "report_at") else "Q")
    for key in ("founder_period_used", "treasury_period_used", "founder_released_total", "treasury_released_total"):
        p[key] = r.number()
    p["annual_index"] = r.number("B")
    p["founder_annual_used"], p["treasury_annual_used"] = r.number(), r.number()
    p["initial_oracle"], p["controller"] = r.key(), r.key()
    for key in ("controller_epoch", "oracle_epoch", "oracle_activated_at", "report_epoch"):
        p[key] = r.number("q" if key == "oracle_activated_at" else "Q")
    flag = r.number("B")
    require(flag in (0, 1), "REPORT_BOOL")
    p["report_valid"] = bool(flag)
    p["change_sequence"], p["pending_change"] = r.number(), r.number()
    p["withdrawal"] = [{"current": r.key(), "epoch": r.number(), "sequence": r.number(), "pending": r.number()} for _ in range(2)]
    p["approval_count"] = r.number()
    r.finish()
    preimage = b"k4v-launch-policy-v6-test-profile-1" + _pubkey(PROGRAM)
    preimage += b"".join(_pubkey(p[k]) for k in ("creator", "mint", "founder", "treasury", "initial_oracle"))
    preimage += p["spec_hash"] + struct.pack("<qqQHqBqq", CLIFF, PERIOD, 12, 500, 7_776_000, 2, PERIOD, 300) + config_bytes
    require(hashlib.sha256(preimage).digest() == p["identity"], "POLICY_IDENTITY")
    expected, bump = find_program_address((b"launch-v6-policy", p["identity"]), _pubkey(PROGRAM))
    require(address == _base58_encode(expected) and p["bump"] == bump, "POLICY_PDA")
    p["address"] = address
    return p


def read_vault(snapshot, name):
    address, r = account(snapshot, name, PROGRAM, "LaunchVaultV6")
    v = {key: r.key() for key in ("policy", "depositor", "authority")}
    v["role"], v["bump"] = r.number("B"), r.number("B")
    v.update({key: r.number() for key in ("principal", "released_total", "period", "period_used")})
    r.finish()
    v["address"] = address
    return v


def read_token(snapshot, name):
    address, r = account(snapshot, name, TOKEN)
    t = {"address": address, "mint": r.key(), "owner": r.key(), "amount": r.number()}
    t["delegate"] = r.number("I")
    r.take(32)
    t["state"] = r.number("B")
    t["native"] = r.number("I")
    r.take(8)
    t["delegated_amount"] = r.number()
    t["close_authority"] = r.number("I")
    r.take(32)
    r.finish()
    require(t["delegate"] in (0, 1) and t["close_authority"] in (0, 1), "TOKEN_OPTION")
    require(t["state"] == 1 and t["native"] == 0, "TOKEN_STATE")
    return t


def quotas(capacity, founder, treasury, eligible):
    require(all(type(x) is int and 0 <= x <= U64 for x in (capacity, founder, treasury)), "INVALID_U64")
    if not eligible:
        return [0, min(capacity, treasury)]
    if founder + treasury == 0:
        return [0, 0]
    total = min(capacity, founder + treasury)
    f = total * founder // (founder + treasury)
    return [f, total - f]


def annual_caps(rule):
    return [rule[k] * rule["release_bps"] // 10_000 for k in ("founder_basis", "treasury_basis")]


def validate_config(c, supply):
    require(c["founder_amount"] > 0 and c["treasury_amount"] > 0
            and c["founder_amount"] + c["treasury_amount"] <= supply, "PRINCIPAL_CONFIG")
    for role in ("founder", "treasury"):
        require(0 < c[role + "_period_cap"] <= c[role + "_amount"], "PERIOD_CAP_CONFIG")
    require(c["shared_hard_cap"] > 0 and 1 <= c["max_report_age"] <= 7 * 86_400, "CAPACITY_CONFIG")
    require(c["t0"] + CLIFF <= 2**63 - 1, "TIME_OVERFLOW")
    rules = c["annual_rules"]
    require(rules[0]["start_period"] == 0 and rules[0]["end_period"] == rules[1]["start_period"], "ANNUAL_CONTINUITY")
    for rule in rules:
        require(rule["start_period"] < rule["end_period"] and rule["release_bps"] <= 500
                and rule["source_hash"] != "00" * 32, "ANNUAL_RULE")
        require(rule["founder_basis"] <= c["founder_amount"] and rule["treasury_basis"] <= c["treasury_amount"], "ANNUAL_BASIS")
        require(rule["shared_cap"] <= sum(annual_caps(rule)), "ANNUAL_SHARED_CAP")
        require(c["t0"] + rule["end_period"] * PERIOD <= 2**63 - 1, "TIME_OVERFLOW")


def projected_limits(p, now):
    c = p["config"]
    period = (now - c["t0"]) // PERIOD
    index = next((i for i, r in enumerate(c["annual_rules"])
                  if r["start_period"] <= period < r["end_period"]), None)
    if index is None:
        return {"period": period, "annual_index": None, "blocked": "NO_ANNUAL_INPUT", "available": [0, 0]}
    used = [p[r + "_period_used"] for r in ("founder", "treasury")] if period == p["period"] else [0, 0]
    annual_used = [p[r + "_annual_used"] for r in ("founder", "treasury")] if index == p["annual_index"] else [0, 0]
    totals = [p[r + "_released_total"] for r in ("founder", "treasury")]
    rule = c["annual_rules"][index]
    annual = annual_caps(rule)
    prior = [annual_used[i] - used[i] for i in range(2)]
    caps = [min(c[r + "_period_cap"], annual[i] // 12, annual[i] - prior[i],
                c[r + "_amount"] - (totals[i] - used[i])) for i, r in enumerate(("founder", "treasury"))]
    capacity = min(p["report_capacity"], c["shared_hard_cap"], rule["shared_cap"] - sum(prior))
    require(min(*caps, capacity, *prior) >= 0, "NEGATIVE_ACCOUNTING")
    q = quotas(capacity, *caps, now >= c["t0"] + CLIFF)
    pause = any(used[i] > q[i] for i in range(2))
    fresh = (p["report_valid"] and p["report_epoch"] == p["oracle_epoch"]
             and p["report_at"] >= p["oracle_activated_at"] and p["report_sequence"] > 0 and p["report_period"] == period
             and c["t0"] + period * PERIOD <= p["report_at"] <= now
             and now - p["report_at"] <= c["max_report_age"])
    available = [q[i] - used[i] for i in range(2)] if fresh and not pause and now > c["t0"] else [0, 0]
    return {"period": period, "annual_index": index, "period_caps": caps, "quotas": q,
            "capacity": capacity, "period_used": used, "annual_used": annual_used,
            "annual_caps": annual, "fresh_report": fresh, "correction_pause": pause,
            "available": available}


NOTICE = 7_776_000


def verify_governance(snapshot, p, now):
    keys = p["config"]["recovery_keys"]
    require(len(set(keys)) == 3 and all(_pubkey(k) != bytes(32) for k in keys), "RECOVERY_KEYS")
    require(p["oracle_activated_at"] <= p["last_action_at"], "ORACLE_ACTIVATION")
    require(p["report_epoch"] <= p["oracle_epoch"], "REPORT_EPOCH")
    if p["report_valid"]:
        require(p["report_sequence"] > 0 and p["report_epoch"] == p["oracle_epoch"]
                and p["oracle_activated_at"] <= p["report_at"], "REPORT_GENERATION")
    elif p["report_sequence"]:
        require(p["report_epoch"] < p["oracle_epoch"], "INVALIDATED_REPORT_EPOCH")
    else:
        require(p["report_epoch"] == 0, "EMPTY_REPORT_EPOCH")
    n = p["change_sequence"]
    # Explicit bounded review profile; never silently truncate proposal history.
    require(n <= 256, "CHANGE_HISTORY_LIMIT")
    names = {k for k in snapshot["accounts"] if k.startswith("change_")}
    require(names == {f"change_{i}" for i in range(1, n + 1)}, "COMPLETE_CHANGE_HISTORY_REQUIRED")
    current = [p["initial_oracle"], p["creator"]]
    epochs = [0, 0]
    pending = None
    earliest_next_creation = -(2**63)
    latest_oracle_maturity = None
    for nonce in range(1, n + 1):
        address, r = account(snapshot, f"change_{nonce}", PROGRAM, "ChangeProposalV6")
        q = {"policy": r.key(), "nonce": r.number(), "kind": r.number("B"), "recovery": r.number("B"),
             "successor": r.key(), "controller_epoch": r.number(), "oracle_epoch": r.number(),
             "created_at": r.number("q"), "execute_after": r.number("q"),
             "status": r.number("B"), "bump": r.number("B")}
        r.finish()
        expected, bump = find_program_address((b"launch-v6-change", _pubkey(p["address"]),
                                              struct.pack("<Q", nonce)), _pubkey(PROGRAM))
        require(address == _base58_encode(expected) and q["bump"] == bump
                and q["policy"] == p["address"] and q["nonce"] == nonce, "CHANGE_PDA_BINDING")
        require(q["kind"] in (0, 1) and q["recovery"] in (0, 1) and q["status"] in (0, 1, 2), "CHANGE_ENUM")
        require(q["oracle_epoch"] == epochs[0] and q["controller_epoch"] == epochs[1], "CHANGE_EPOCH_HISTORY")
        require(_pubkey(q["successor"]) != bytes(32) and q["successor"] != current[q["kind"]], "CHANGE_SUCCESSOR")
        require(earliest_next_creation <= q["created_at"] <= p["last_action_at"] <= now
                and q["execute_after"] == q["created_at"] + NOTICE, "CHANGE_NOTICE_TIME")
        earliest_next_creation = q["created_at"]
        if q["status"] == 1:
            require(q["execute_after"] <= p["last_action_at"], "EXECUTED_BEFORE_NOTICE")
            current[q["kind"]] = q["successor"]
            epochs[q["kind"]] += 1
            earliest_next_creation = q["execute_after"]
            if q["kind"] == 0:
                latest_oracle_maturity = q["execute_after"]
        elif q["status"] == 0:
            require(nonce == n and p["pending_change"] == nonce, "PENDING_CHANGE_LINK")
            pending = {"nonce": nonce, "kind": "oracle" if q["kind"] == 0 else "controller",
                       "recovery": bool(q["recovery"]), "successor": q["successor"],
                       "execute_after": q["execute_after"], "mature": now >= q["execute_after"]}
    require(p["pending_change"] == (n if pending else 0), "PENDING_CHANGE_LINK")
    require(current == [p["oracle"], p["controller"]]
            and epochs == [p["oracle_epoch"], p["controller_epoch"]], "CURRENT_KEYS_OR_EPOCHS")
    if latest_oracle_maturity is not None:
        require(latest_oracle_maturity <= p["oracle_activated_at"], "ORACLE_ACTIVATION_NOTICE")
    else:
        require(p["oracle_activated_at"] < p["config"]["t0"], "INITIAL_ORACLE_ACTIVATION")
    return {"controller": p["controller"], "oracle": p["oracle"],
            "controller_epoch": p["controller_epoch"], "oracle_epoch": p["oracle_epoch"],
            "report_valid": p["report_valid"], "report_epoch": p["report_epoch"],
            "recovery_keys": keys, "required_recovery_signatures": 2,
            "change_sequence": n, "pending": pending,
            "founder_withdrawal_key": p["withdrawal"][0]["current"], "treasury_withdrawal_key": p["withdrawal"][1]["current"],
            "withdrawal_key_recovery_supported": True, "human_independence_verified": False}


def verify(snapshot):
    require(snapshot["schema"] == "K4V-LAUNCH-V6-RAW-SNAPSHOT-v1" and snapshot["program_id"] == PROGRAM, "SCHEMA_OR_PROGRAM")
    require(snapshot["scope"] in ("AUTHOR_RUN_LOCAL_LITESVM", "SUPPLIED_RPC_RESPONSE") and snapshot["private_keys_serialized"] is False, "SCOPE")
    require(type(snapshot["now"]) is str and snapshot["now"].lstrip("-").isdigit(), "CLOCK_TYPE")
    now = int(snapshot["now"])
    require(-(2**63) <= now < 2**63, "CLOCK_RANGE")
    require(len({a["address"] for a in snapshot["accounts"].values()}) == len(snapshot["accounts"]), "ACCOUNT_ALIAS")
    p = read_policy(snapshot)
    c = p["config"]
    governance = verify_governance(snapshot, p, now)
    require(p["state"] == 2 and p["funded_mask"] == 3, "ONLY_ACTIVE_DUAL_POOL_GRAPH_SUPPORTED")
    require(p["spec_hash"] != bytes(32) and _pubkey(p["oracle"]) != bytes(32), "POLICY_INPUT")
    require(c["t0"] <= p["last_action_at"] <= now, "CLOCK_ROLLBACK")
    require(0 <= p["period"] <= (now - c["t0"]) // PERIOD, "STORED_PERIOD")
    mint_address, mint = account(snapshot, "mint", TOKEN)
    require(mint_address == p["mint"] and mint.number("I") == 0, "MINT_AUTHORITY")
    mint.take(32)
    supply, decimals = mint.number(), mint.number("B")
    require(mint.number("B") == 1 and mint.number("I") == 0, "MINT_FREEZE_AUTHORITY")
    mint.take(32)
    mint.finish()
    validate_config(c, supply)
    require(p["annual_index"] in (0, 1), "ANNUAL_INDEX")
    stored_rule = c["annual_rules"][p["annual_index"]]
    require(stored_rule["start_period"] <= p["period"] < stored_rule["end_period"], "ANNUAL_INDEX_PERIOD")
    annual = annual_caps(stored_rule)
    founder_names = sorted(n for n in snapshot["accounts"] if n.startswith("founder_destination_"))
    require(1 <= len(founder_names) <= 8 and founder_names == [f"founder_destination_{i}" for i in range(len(founder_names))], "FOUNDER_DESTINATION_SET")
    tokens = {name: read_token(snapshot, name) for name in ["source", "founder_token", "treasury_token", "treasury_destination"] + founder_names}
    require(all(t["mint"] == p["mint"] for t in tokens.values()), "TOKEN_MINT")
    surplus = []
    for i, role in enumerate(("founder", "treasury")):
        v = read_vault(snapshot, role + "_vault")
        expected, bump = find_program_address((b"launch-v6-vault", _pubkey(p["address"]), bytes([i])), _pubkey(PROGRAM))
        require(v["address"] == _base58_encode(expected) and v["bump"] == bump and v["role"] == i, "VAULT_PDA_OR_ROLE")
        require(v["policy"] == p["address"] and v["authority"] == p[role] and v["depositor"] == tokens["source"]["owner"], "VAULT_BINDING")
        require(v["principal"] == c[role + "_amount"] and v["released_total"] == p[role + "_released_total"], "LIFETIME_ACCOUNTING")
        require(v["released_total"] <= v["principal"] and v["period"] <= p["period"], "VAULT_COUNTERS")
        used = v["period_used"] if v["period"] == p["period"] else 0
        require(used == p[role + "_period_used"] and v["period_used"] <= v["released_total"], "PERIOD_ACCOUNTING")
        require(used <= min(c[role + "_period_cap"], annual[i] // 12), "PERIOD_CAP_EXCEEDED")
        require(used <= p[role + "_annual_used"] <= min(annual[i], v["released_total"]), "ANNUAL_ACCOUNTING")
        if role == "founder" and v["released_total"]:
            require(v["period"] >= 6, "FOUNDER_CLIFF")
        t = tokens[role + "_token"]
        address, _ = find_program_address((b"launch-v6-token", _pubkey(v["address"])), _pubkey(PROGRAM))
        require(t["address"] == _base58_encode(address) and t["owner"] == v["address"], "TOKEN_VAULT_PDA")
        require(t["delegate"] == 0 and t["delegated_amount"] == 0 and t["close_authority"] == 0, "TOKEN_VAULT_AUTHORITY")
        excess = t["amount"] - (v["principal"] - v["released_total"])
        require(excess >= 0, "CUSTODY_DEFICIT")
        surplus.append(excess)
    require(p["shared_used"] == p["founder_period_used"] + p["treasury_period_used"], "SHARED_ACCOUNTING")
    require(p["shared_used"] <= c["shared_hard_cap"], "HARD_CAP_EXCEEDED")
    require(p["founder_annual_used"] + p["treasury_annual_used"] <= stored_rule["shared_cap"], "ANNUAL_SHARED_ACCOUNTING")
    require(sum(t["amount"] for t in tokens.values()) == supply, "TOKEN_CONSERVATION")
    approvals = read_approvals(snapshot, p, now)
    withdrawal = verify_withdrawal(snapshot, p, approvals, now)
    for name in founder_names:
        require(tokens[name]["amount"] == 0 or tokens[name]["owner"] in withdrawal["history"][0], "FOUNDER_DESTINATION")
    require(sum(tokens[name]["amount"] for name in founder_names) == p["founder_released_total"], "DECLARED_FOUNDER_RELEASE_GRAPH")
    dest = tokens["treasury_destination"]
    require(all(a["recipient"] == dest["address"] and a["recipient_owner"] == dest["owner"] for a in approvals), "DECLARED_TREASURY_RECIPIENT_GRAPH")
    require(sum(a["consumed"] for a in approvals) == p["treasury_released_total"] == dest["amount"], "APPROVAL_LIFETIME")
    for a in approvals:
        if a["consumed"]:
            require(now >= a["created_at"] + PERIOD and a["period"] <= p["period"], "APPROVAL_NOTICE")
        if a["period"] == p["period"]:
            require(a["consumed"] == p["treasury_period_used"], "APPROVAL_PERIOD_ACCOUNTING")
    require(p["report_at"] <= p["last_action_at"] and p["report_period"] <= (now - c["t0"]) // PERIOD, "REPORT_TIME")
    if p["report_sequence"]:
        start = c["t0"] + p["report_period"] * PERIOD
        require(start <= p["report_at"] < start + PERIOD, "REPORT_OBSERVATION_PERIOD")
    else:
        require(p["report_at"] == p["report_period"] == p["report_capacity"] == 0, "EMPTY_REPORT_STATE")
    limits = projected_limits(p, now)
    current = next((a for a in approvals if a["period"] == limits["period"]), None)
    treasury_notice_ok = current is not None and now >= current["created_at"] + PERIOD
    ceilings = limits["available"].copy()
    ceilings[1] = min(ceilings[1], current["need"] - current["consumed"]) if treasury_notice_ok else 0
    for i in range(2):
        if p["withdrawal"][i]["pending"]:
            ceilings[i] = 0
    def safe_numbers(value):
        if type(value) is int:
            return str(value)
        if isinstance(value, list):
            return [safe_numbers(v) for v in value]
        if isinstance(value, dict):
            return {k: safe_numbers(v) for k, v in value.items()}
        return value
    return safe_numbers({"valid": True, "scope": "SUPPLIED_ACTIVE_DUAL_POOL_GRAPH_ONLY",
                         "on_chain_authenticity_verified": False, "independent_human_audit": False,
                         "program_id": PROGRAM, "program_bytes_verified": False,
                         "governance": governance, "withdrawal": withdrawal, "decimals": decimals, "conserved_supply": supply,
                         "surplus": surplus, "limits": limits, "treasury_notice_ok": treasury_notice_ok,
                         "amount_ceilings_before_transaction_signatures": ceilings})


def read_approvals(snapshot, p, now):
    names = sorted(n for n in snapshot['accounts'] if n.startswith('approval_'))
    require(len(names) == p['approval_count'] <= 64, 'COMPLETE_APPROVAL_HISTORY')
    result = []
    for name in names:
        address, r = account(snapshot, name, PROGRAM, 'TreasuryApprovalV6')
        a = {'policy': r.key(), 'period': r.number(), 'recipient': r.key(), 'recipient_owner': r.key(),
             'need': r.number(), 'consumed': r.number(), 'created_at': r.number('q'), 'bump': r.number('B'),
             'author': r.key(), 'author_epoch': r.number()}
        r.finish()
        expected, bump = find_program_address((b'launch-v6-approval', _pubkey(p['address']), struct.pack('<Q', a['period'])), _pubkey(PROGRAM))
        require(name == f"approval_{a['period']}" and address == _base58_encode(expected)
                and a['bump'] == bump and a['policy'] == p['address'], 'APPROVAL_PDA')
        require(a['need'] > 0 and a['consumed'] <= a['need'] and a['created_at'] <= p['last_action_at'] <= now, 'APPROVAL_COUNTERS')
        require(a['created_at'] < p['config']['t0'] + a['period'] * PERIOD, 'APPROVAL_TARGET_PERIOD')
        result.append(a)
    return result


def verify_withdrawal(snapshot, p, approvals, now):
    originals = [p['founder'], p['treasury']]
    committees = [p['config'][r + '_recovery_keys'] for r in ('founder', 'treasury')]
    for i, keys in enumerate(committees):
        require(len(set(keys)) == 3 and all(_pubkey(k) != bytes(32) for k in keys)
                and originals[i] not in keys, 'WITHDRAWAL_REGISTRATION')
    history = [[k] for k in originals]
    activations = [[None], [None]]
    expected_masks = {}
    needed = {a['recipient_owner'] for a in approvals}
    terminal = []
    all_proposals = []
    expected_names = set()
    for role in range(2):
        current, epoch, earliest = originals[role], 0, -(2**63)
        state = p['withdrawal'][role]
        require(state['sequence'] <= 256, 'WITHDRAWAL_HISTORY_LIMIT')
        pending_nonce = 0
        for nonce in range(1, state['sequence'] + 1):
            name = f'withdrawal_{role}_{nonce}'
            expected_names.add(name)
            address, r = account(snapshot, name, PROGRAM, 'WithdrawalProposalV6')
            q = {'policy': r.key(), 'role': r.number('B'), 'nonce': r.number(), 'epoch': r.number(),
                 'predecessor': r.key(), 'successor': r.key(), 'recovery': r.number('B'),
                 'valid_from': r.number('q'), 'valid_until': r.number('q'),
                 'created_at': r.number('q'), 'execute_after': r.number('q'), 'expires_at': r.number('q'),
                 'status': r.number('B'), 'finished_at': r.number('q'), 'bump': r.number('B')}
            r.finish()
            expected, bump = find_program_address((b'launch-v6-withdraw', _pubkey(p['address']), bytes([role]), struct.pack('<Q', nonce)), _pubkey(PROGRAM))
            require(address == _base58_encode(expected) and q['bump'] == bump and q['policy'] == p['address']
                    and q['role'] == role and q['nonce'] == nonce, 'WITHDRAWAL_PDA')
            require(q['recovery'] in (0, 1) and q['status'] in (0, 1, 2, 3), 'WITHDRAWAL_ENUM')
            require(q['epoch'] == epoch and q['predecessor'] == current, 'WITHDRAWAL_EPOCH_HISTORY')
            require(_pubkey(q['successor']) != bytes(32) and q['successor'] not in history[role]
                    and q['successor'] not in committees[role], 'WITHDRAWAL_SUCCESSOR')
            require(0 <= q['valid_from'] <= q['created_at'] <= q['valid_until']
                    and 0 <= q['valid_until'] - q['valid_from'] <= 300
                    and q['valid_until'] + NOTICE + PERIOD < 2**63, 'WITHDRAWAL_ADMISSION')
            require(earliest <= q['created_at'] <= p['last_action_at'] <= now
                    and q['execute_after'] == q['created_at'] + NOTICE
                    and q['expires_at'] == q['execute_after'] + PERIOD < 2**63, 'WITHDRAWAL_NOTICE')
            needed.add(q['successor'])
            masks = expected_masks.setdefault(q['successor'], [0, 0])
            if q['status'] == 0:
                require(nonce == state['sequence'] and q['finished_at'] == 0, 'WITHDRAWAL_PENDING')
                pending_nonce = nonce
                masks[1] |= 1 << role
            else:
                require(q['created_at'] <= q['finished_at'] <= p['last_action_at'], 'WITHDRAWAL_FINISH_TIME')
                earliest = q['finished_at']
                if q['status'] == 1:
                    require(q['execute_after'] <= q['finished_at'] < q['expires_at'], 'WITHDRAWAL_EXECUTION_WINDOW')
                    current, epoch = q['successor'], epoch + 1
                    history[role].append(current)
                    activations[role].append(q['finished_at'])
                    masks[0] |= 1 << role
                elif q['status'] == 3:
                    require(q['finished_at'] >= q['expires_at'], 'WITHDRAWAL_EXPIRY')
            all_proposals.append(q)
        require(state['current'] == current and state['epoch'] == epoch and state['pending'] == pending_nonce, 'WITHDRAWAL_CURRENT_STATE')
        terminal.append({'current': current, 'epoch': epoch, 'sequence': state['sequence'], 'paused': bool(pending_nonce)})
    require({n for n in snapshot['accounts'] if n.startswith('withdrawal_')} == expected_names, 'COMPLETE_WITHDRAWAL_HISTORY')
    records = {}
    for name in snapshot['accounts']:
        if not name.startswith('key_'):
            continue
        address, r = account(snapshot, name, PROGRAM, 'WithdrawalKeyV6')
        k = {'policy': r.key(), 'subject': r.key(), 'history_mask': r.number('B'),
             'pending_mask': r.number('B'), 'recipient': r.number('B'), 'bump': r.number('B')}
        r.finish()
        expected, bump = find_program_address((b'launch-v6-key', _pubkey(p['address']), _pubkey(k['subject'])), _pubkey(PROGRAM))
        require(name == 'key_' + k['subject'] and address == _base58_encode(expected)
                and k['bump'] == bump and k['policy'] == p['address'], 'KEY_INDEX_PDA')
        require(k['recipient'] in (0, 1) and k['subject'] not in records, 'KEY_INDEX_ENUM_OR_ALIAS')
        require([k['history_mask'], k['pending_mask']] == expected_masks.get(k['subject'], [0, 0]), 'KEY_HISTORY_MASK')
        require(bool(k['recipient']) == any(a['recipient_owner'] == k['subject'] for a in approvals), 'KEY_RECIPIENT_HISTORY')
        records[k['subject']] = k
    require(needed <= set(records), 'REQUIRED_KEY_INDEX_MISSING')
    for a in approvals:
        k = records[a['recipient_owner']]
        require(a['recipient_owner'] not in originals + committees[0] + committees[1]
                and k['history_mask'] == k['pending_mask'] == 0, 'KNOWN_SELF_PAYMENT')
        e = a['author_epoch']
        require(e < len(history[1]) and a['author'] == history[1][e], 'APPROVAL_AUTHOR_EPOCH')
        require(e == 0 or a['created_at'] >= activations[1][e], 'APPROVAL_AUTHOR_TIME')
        require(e + 1 == len(history[1]) or a['created_at'] <= activations[1][e + 1], 'RETIRED_APPROVAL_AUTHOR')
        for q in all_proposals:
            if q['successor'] == a['recipient_owner']:
                require(q['status'] in (2, 3) and a['created_at'] >= q['finished_at'], 'RECIPIENT_SUCCESSOR_COLLISION')
            if q['role'] == 1:
                end = now + 1 if q['status'] == 0 else q['finished_at']
                require(not q['created_at'] < a['created_at'] < end, 'APPROVAL_DURING_PAUSE')
    return {'roles': terminal, 'history': history, 'proposals_verified': len(all_proposals),
            'key_indexes_verified': len(records), 'approvals_verified': len(approvals),
            'related_party_human_identity_verified': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    args = parser.parse_args()
    try:
        result = verify(json.loads(args.snapshot.read_text()))
    except (ValueError, KeyError, TypeError, struct.error) as error:
        print(json.dumps({'valid': False, 'error': str(error)}))
        raise SystemExit(1) from error
    print(json.dumps(result, indent=2))
