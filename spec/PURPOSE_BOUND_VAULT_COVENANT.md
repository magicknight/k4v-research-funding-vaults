# Purpose-Bound Vault Covenant v0.2

v0.1 remains the version the deployed B1 prototype implements, and its
statements about B1 are unchanged. v0.2 adds the unit statement below and the
optional absolute ceiling; it changes no cap, rate, cliff, or notice period.

Status: EXECUTABLE SPECIFICATION / BENEFICIARY B1 IMPLEMENTED / FULL ON-CHAIN COVENANT OPEN

## Pools

The covenant separates:

1. a beneficiary vault, whose economic beneficiary is disclosed; and
2. a purpose vault, whose releases require an approved, public need.

Both start with zero economically releasable balance. A deployment must
disclose total supply, allocations, authorities, vault addresses, signer
thresholds, and the exact version of this covenant it implements.

## Monthly rule

All quantities are integer base units and rates are integer basis points.

**Volume is a token amount, so it too is in base units, not in a quote
currency.** "Spot volume" conventionally means a quote-currency figure; a
report in USD would silently rescale `M_m` by the price. Stating the unit is
what keeps price out of the covenant altogether: both sides of every inequality
below are token amounts, so the price cancels. It also lets venues be summed
without a per-venue price, which is what makes a multi-pool figure well defined
at all.

~~~text
B_m = floor(floor(B_start * annual_release_bps / 10000) / 12)
P_m = floor(floor(P_start * annual_release_bps / 10000) / 12)
A_m = floor(eligible_trailing_30d_spot_volume * market_capacity_bps / 10000)
M_m = min(A_m, hard_ceiling)

beneficiary_release_m <= B_m
purpose_release_m     <= min(P_m, approved_need_m)
beneficiary_release_m + purpose_release_m <= M_m
~~~

`hard_ceiling` is an absolute bound in base units, frozen when the policy is
created and unbounded if omitted. Its purpose is stated precisely because it is
easy to overstate:

- it is **not** what stops a compromised oracle from accelerating a release.
  Nothing needs to: `B_m` and `P_m` do not depend on the oracle, so the widest
  window any report can open still leaves `B_m + P_m` as the bound on the
  period. Setting `hard_ceiling` unbounded loses no guarantee;
- it **is** the only term in `M_m` that no key can move — not the oracle's, not
  the policy authority's. A deployment that wants to promise a bound tighter
  than its own release schedule has no other way to say so, and no way to add
  one later.

A ceiling denominated in tokens needs no adjustment for price. If the token
appreciates, the same ceiling releases proportionally more value with no
governance action. The only circumstance that would motivate raising one is a
price collapse — which is precisely when releasing more tokens is what the
covenant exists to prevent. A ceiling is therefore set once or not at all.

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
4. aggregate release above the market-capacity cap, or above a frozen absolute
   ceiling where one is set;
5. any release with zero eligible volume;
6. carry-forward of unused monthly capacity;
7. bypass event omitted from economic circulation;
8. emergency acceleration;
9. mint, freeze, upgrade, or configuration authority inconsistent with the
   declared deployment;
10. deployed artifact hash inconsistent with the reviewed candidate.

The Python reference model currently evaluates vectors 1–7. The B1 Solana
prototype independently enforces the beneficiary portions of vectors 1, 2,
6, and 8 through a deliberately two-instruction surface; see
[BENEFICIARY_VAULT_B1.md](BENEFICIARY_VAULT_B1.md). Purpose-vault, market,
deployment-authority, provenance, and public-chain claims remain open.
