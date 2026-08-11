# Purpose-Bound Vault Covenant v0.1

Status: EXECUTABLE SPECIFICATION / ON-CHAIN IMPLEMENTATION OPEN

## Pools

The covenant separates:

1. a beneficiary vault, whose economic beneficiary is disclosed; and
2. a purpose vault, whose releases require an approved, public need.

Both start with zero economically releasable balance. A deployment must
disclose total supply, allocations, authorities, vault addresses, signer
thresholds, and the exact version of this covenant it implements.

## Monthly rule

All quantities are integer base units and rates are integer basis points.

~~~text
B_m = floor(floor(B_start * annual_release_bps / 10000) / 12)
P_m = floor(floor(P_start * annual_release_bps / 10000) / 12)
M_m = floor(eligible_trailing_30d_spot_volume * market_capacity_bps / 10000)

beneficiary_release_m <= B_m
purpose_release_m     <= min(P_m, approved_need_m)
beneficiary_release_m + purpose_release_m <= M_m
~~~

Before the configured beneficiary cliff, beneficiary_release_m must be zero.
Unused monthly capacity expires; it is not an input to the next month.

The reference model bounds both rates at 500 basis points. This is a safety
ceiling for the model, not a recommendation for production.

## Net-release classification

The following count as economic circulation:

- AMM or exchange sale;
- OTC transfer;
- grant, airdrop, bounty, or payment for service;
- transfer to a freely disposable wallet;
- collateral, loan, option, forward, or liquidation exposure;
- transfer of a beneficiary, claim key, ownership, or economic right.

Only migration to an equal-or-stricter lock with continuous, publicly
reconcilable balances is excluded from net release.

## Purpose gate

A purpose-vault release must not exceed the independently supplied
purpose_approved_need. Production work must define who approves a need,
how conflicts are handled, and how an approval maps to public evidence.
The reference model deliberately does not invent an approval authority.

## Authority invariants

- mint authority remains absent after fixed supply is established;
- emergency authority may pause but never accelerate release;
- upgrades or configuration changes may not shorten the cliff, increase an
  already-frozen cap, restore minting, lower signature thresholds, or create a
  new release path;
- a mainnet artifact must match the tested source and configuration hashes.

## Required rejection vectors

1. beneficiary release before cliff;
2. either vault over its monthly rate cap;
3. purpose release without approved need or above that need;
4. aggregate release above the market-capacity cap;
5. any release with zero eligible volume;
6. carry-forward of unused monthly capacity;
7. bypass event omitted from economic circulation;
8. emergency acceleration;
9. mint, freeze, upgrade, or configuration authority inconsistent with the
   declared deployment;
10. deployed artifact hash inconsistent with the reviewed candidate.

The Python reference model currently evaluates vectors 1–7. Vectors 8–10
belong to the open Solana implementation and adversarial harness.
