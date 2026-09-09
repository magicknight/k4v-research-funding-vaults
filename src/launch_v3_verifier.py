"""Independent standard-library decoder and accounting verifier for the v3
declared local graph. It does not use the Rust math or generated IDL, contact
RPC, prove a source is truthful, or authenticate a deployed program's bytes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address

PROGRAM = "AhTz3JFbaEvk1PMxEsmG89YiTZQJ4ALKoFc8vyr1Cf8m"
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
    _pubkey(a["address"])
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
    address, r = account(snapshot, "policy", PROGRAM, "LaunchPolicyV3")
    p = {key: r.key() for key in ("creator", "mint", "founder", "treasury", "oracle")}
    p["identity"], p["spec_hash"] = r.take(32), r.take(32)
    start = r.pos
    p["config"] = read_config(r)
    config_bytes = r.data[start:r.pos]
    p.update({key: r.number("B") for key in ("state", "funded_mask", "bump")})
    for key in ("last_action_at", "period", "shared_used", "report_period", "report_capacity", "report_at", "report_sequence"):
        p[key] = r.number("q" if key in ("last_action_at", "report_at") else "Q")
    for key in ("founder_period_used", "treasury_period_used", "founder_released_total", "treasury_released_total"):
        p[key] = r.number()
    p["annual_index"] = r.number("B")
    p["founder_annual_used"], p["treasury_annual_used"] = r.number(), r.number()
    r.finish()
    preimage = b"k4v-launch-policy-v3-test-profile-1" + _pubkey(PROGRAM)
    preimage += b"".join(_pubkey(p[k]) for k in ("creator", "mint", "founder", "treasury", "oracle"))
    preimage += p["spec_hash"] + struct.pack("<qqQH", CLIFF, PERIOD, 12, 500) + config_bytes
    require(hashlib.sha256(preimage).digest() == p["identity"], "POLICY_IDENTITY")
    expected, bump = find_program_address((b"launch-v3-policy", p["identity"]), _pubkey(PROGRAM))
    require(address == _base58_encode(expected) and p["bump"] == bump, "POLICY_PDA")
    p["address"] = address
    return p


def read_vault(snapshot, name):
    address, r = account(snapshot, name, PROGRAM, "LaunchVaultV3")
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
    fresh = (p["report_sequence"] > 0 and p["report_period"] == period
             and c["t0"] + period * PERIOD <= p["report_at"] <= now
             and now - p["report_at"] <= c["max_report_age"])
    available = [q[i] - used[i] for i in range(2)] if fresh and not pause and now > c["t0"] else [0, 0]
    return {"period": period, "annual_index": index, "period_caps": caps, "quotas": q,
            "capacity": capacity, "period_used": used, "annual_used": annual_used,
            "annual_caps": annual, "fresh_report": fresh, "correction_pause": pause,
            "available": available}


def verify(snapshot):
    require(snapshot["schema"] == "K4V-LAUNCH-V3-RAW-SNAPSHOT-v1" and snapshot["program_id"] == PROGRAM, "SCHEMA_OR_PROGRAM")
    require(snapshot["scope"] == "AUTHOR_RUN_LOCAL_LITESVM" and snapshot["private_keys_serialized"] is False, "SCOPE")
    require(type(snapshot["now"]) is str and snapshot["now"].lstrip("-").isdigit(), "CLOCK_TYPE")
    now = int(snapshot["now"])
    require(-(2**63) <= now < 2**63, "CLOCK_RANGE")
    p = read_policy(snapshot)
    c = p["config"]
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
    tokens = {name: read_token(snapshot, name) for name in
              ("source", "founder_token", "treasury_token", "founder_destination", "treasury_destination")}
    require(len({t["address"] for t in tokens.values()}) == 5, "TOKEN_ACCOUNT_ALIAS")
    require(all(t["mint"] == p["mint"] for t in tokens.values()), "TOKEN_MINT")
    surplus = []
    for i, role in enumerate(("founder", "treasury")):
        v = read_vault(snapshot, role + "_vault")
        expected, bump = find_program_address((b"launch-v3-vault", _pubkey(p["address"]), bytes([i])), _pubkey(PROGRAM))
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
        address, _ = find_program_address((b"launch-v3-token", _pubkey(v["address"])), _pubkey(PROGRAM))
        require(t["address"] == _base58_encode(address) and t["owner"] == v["address"], "TOKEN_VAULT_PDA")
        require(t["delegate"] == 0 and t["delegated_amount"] == 0 and t["close_authority"] == 0, "TOKEN_VAULT_AUTHORITY")
        excess = t["amount"] - (v["principal"] - v["released_total"])
        require(excess >= 0, "CUSTODY_DEFICIT")
        surplus.append(excess)
    require(p["shared_used"] == p["founder_period_used"] + p["treasury_period_used"], "SHARED_ACCOUNTING")
    require(p["shared_used"] <= c["shared_hard_cap"], "HARD_CAP_EXCEEDED")
    require(p["founder_annual_used"] + p["treasury_annual_used"] <= stored_rule["shared_cap"], "ANNUAL_SHARED_ACCOUNTING")
    require(sum(t["amount"] for t in tokens.values()) == supply, "TOKEN_CONSERVATION")
    require(tokens["founder_destination"]["owner"] == p["founder"], "FOUNDER_DESTINATION")
    approval_address, r = account(snapshot, "approval", PROGRAM, "TreasuryApprovalV3")
    a = {"policy": r.key(), "period": r.number(), "recipient": r.key(), "recipient_owner": r.key(),
         "need": r.number(), "consumed": r.number(), "created_at": r.number("q"), "bump": r.number("B")}
    r.finish()
    expected, bump = find_program_address((b"launch-v3-approval", _pubkey(p["address"]), struct.pack("<Q", a["period"])), _pubkey(PROGRAM))
    require(approval_address == _base58_encode(expected) and a["bump"] == bump and a["policy"] == p["address"], "APPROVAL_PDA")
    dest = tokens["treasury_destination"]
    require(a["recipient"] == dest["address"] and a["recipient_owner"] == dest["owner"] and dest["owner"] != p["treasury"], "APPROVAL_RECIPIENT")
    require(0 <= a["consumed"] <= a["need"] and a["need"] > 0 and a["created_at"] <= now, "APPROVAL_COUNTERS")
    require(p["report_at"] <= p["last_action_at"] and p["report_period"] <= (now - c["t0"]) // PERIOD, "REPORT_TIME")
    if p["report_sequence"]:
        start = c["t0"] + p["report_period"] * PERIOD
        require(start <= p["report_at"] < start + PERIOD, "REPORT_OBSERVATION_PERIOD")
    else:
        require(p["report_at"] == p["report_period"] == p["report_capacity"] == 0, "EMPTY_REPORT_STATE")
    limits = projected_limits(p, now)
    treasury_notice_ok = a["period"] == limits["period"] and now >= a["created_at"] + PERIOD
    ceilings = limits["available"].copy()
    ceilings[1] = min(ceilings[1], a["need"] - a["consumed"]) if treasury_notice_ok else 0
    def safe_numbers(value):
        if type(value) is int:
            return str(value)
        if isinstance(value, list):
            return [safe_numbers(v) for v in value]
        if isinstance(value, dict):
            return {k: safe_numbers(v) for k, v in value.items()}
        return value
    return safe_numbers({"valid": True, "scope": "SUPPLIED_LOCAL_ACCOUNT_GRAPH_ONLY",
                         "on_chain_authenticity_verified": False, "independent_human_audit": False,
                         "program_id": PROGRAM, "decimals": decimals, "conserved_supply": supply,
                         "surplus": surplus, "limits": limits, "treasury_notice_ok": treasury_notice_ok,
                         "amount_ceilings_before_transaction_signatures": ceilings})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("snapshot", type=Path)
    args = parser.parse_args()
    try:
        result = verify(json.loads(args.snapshot.read_text()))
    except (ValueError, KeyError, TypeError, struct.error) as error:
        print(json.dumps({"valid": False, "error": str(error)}))
        raise SystemExit(1) from error
    print(json.dumps(result, indent=2))
