// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Read-only. Decodes B2's accounts straight off devnet and checks them against
// the numbers the specification pins, so the claim rests on the cluster rather
// than on the probe's own report of what it did.
//
// Needs devnet_result.json from b2_devnet_probe.mjs for the addresses; it reads
// nothing else and signs nothing.

// Read-only. Decodes B2's accounts straight off devnet and prints what they
// say, so the claim rests on the cluster rather than on the probe's own report.
import { Connection, PublicKey } from "@solana/web3.js";
import fs from "fs";

const c = new Connection("https://api.devnet.solana.com", "confirmed");
const r = JSON.parse(fs.readFileSync("devnet_result.json", "utf8"));
const P = k => new PublicKey(k);
const out = {};

const rd = d => { let o = 8; return {
  pk: () => { const v = new PublicKey(d.subarray(o, o + 32)).toBase58(); o += 32; return v; },
  b32: () => { const v = d.subarray(o, o + 32).toString("hex"); o += 32; return v; },
  u8: () => d[o++], u16: () => { const v = d.readUInt16LE(o); o += 2; return v; },
  u32: () => { const v = d.readUInt32LE(o); o += 4; return v; },
  u64: () => { const v = d.readBigUInt64LE(o); o += 8; return v.toString(); },
  i64: () => { const v = d.readBigInt64LE(o); o += 8; return Number(v); },
}; };

const get = async k => (await c.getAccountInfo(P(k), "confirmed")).data;

const p = rd(await get(r.policy_pda));
out.policy = { authority: p.pk(), mint: p.pk(), policy_hash: p.b32(), genesis_ts: p.i64(),
  current_period_index: p.u64(), released_this_period: p.u64(), hard_ceiling: p.u64(),
  silence_floor: p.u64(), silence_grace_seconds: p.i64(), vault_count: p.u32(), bump: p.u8() };

const m = rd(await get(r.market_pda));
out.market = { oracle: m.pk(), policy_hash: m.b32(), eligible_volume: m.u64(), updated_at: m.i64(),
  max_age_seconds: m.i64(), report_count: m.u64(), market_capacity_bps: m.u16(), bump: m.u8(),
  pending_oracle: m.pk(), pending_since: m.i64() };

for (const [name, key] of [["beneficiary_vault", r.beneficiary_vault], ["purpose_vault", r.purpose_vault]]) {
  const v = rd(await get(key));
  const kind = v.u8();
  out[name] = { kind: kind === 0 ? "Beneficiary" : "Purpose", depositor: v.pk(), authority: v.pk(),
    mint: v.pk(), policy_hash: v.b32(), deposited_amount: v.u64(), monthly_cap: v.u64(),
    released_total: v.u64(), released_this_period: v.u64(), current_period_index: v.u64(),
    genesis_ts: v.i64(), cliff_end_ts: v.i64(), annual_release_bps: v.u16(),
    mint_decimals: v.u8(), state_bump: v.u8(), token_vault_bump: v.u8() };
  out[name].cliff_span_seconds = out[name].cliff_end_ts - out[name].genesis_ts;
}

for (const [name, key] of [["approval", r.approval_pda], ["approval_after_replacement", r.approval_pda_after_replacement]]) {
  const a = rd(await get(key));
  out[name] = { vault: a.pk(), approver: a.pk(), destination: a.pk(), period_index: a.u64(),
    approved_need: a.u64(), consumed: a.u64(), created_at: a.i64(), bump: a.u8() };
}

for (const [name, key] of [["beneficiary_vault_token", r.beneficiary_vault_token], ["purpose_vault_token", r.purpose_vault_token]])
  out[name + "_balance"] = (await c.getTokenAccountBalance(P(key))).value.amount;

const ms = await c.getAccountInfo(P(r.multisig), "confirmed");
out.multisig_account_exists = !!ms;
out.checks = {
  policy_authority_is_the_multisig_vault: out.policy.authority === r.vault_pda,
  purpose_vault_approver_is_the_multisig_vault: out.purpose_vault.authority === r.vault_pda,
  approval_after_replacement_approver_is_the_same_vault: out.approval_after_replacement.approver === r.vault_pda,
  beneficiary_monthly_cap_is_the_pinned_1_250_000: out.beneficiary_vault.monthly_cap === "1250000",
  purpose_monthly_cap_is_the_pinned_2_083_333: out.purpose_vault.monthly_cap === "2083333",
  hard_ceiling_is_2_500_000: out.policy.hard_ceiling === "2500000",
  silence_floor_is_zero_the_fail_closed_default: out.policy.silence_floor === "0",
  cliff_span_is_63_072_000_seconds: out.beneficiary_vault.cliff_span_seconds === 63072000,
  purpose_vault_has_no_cliff: out.purpose_vault.cliff_span_seconds === 0,
  two_vaults_on_one_window: out.policy.vault_count === 2,
  nothing_has_been_released: out.policy.released_this_period === "0" &&
    out.beneficiary_vault.released_total === "0" && out.purpose_vault.released_total === "0",
  vault_tokens_hold_the_full_deposits: out.beneficiary_vault_token_balance === "300000000" &&
    out.purpose_vault_token_balance === "500000000",
  a_rotation_is_pending_and_has_not_taken_effect:
    out.market.pending_oracle === r.proposed_oracle && out.market.oracle === r.oracle,
};
console.log(JSON.stringify(out, null, 2));
fs.writeFileSync("devnet_verified.json", JSON.stringify(out, null, 2));
