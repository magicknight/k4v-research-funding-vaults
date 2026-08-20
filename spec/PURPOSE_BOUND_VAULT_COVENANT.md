# Purpose-Bound Vault Covenant v0.3

v0.1 remains the version the deployed B1 prototype implements, and its
statements about B1 are unchanged. v0.2 added the unit statement below and the
optional absolute ceiling. v0.3 adds two named exceptions to "the oracle is
frozen forever": a noticed rotation of who may report, and an optional declared
floor for when nobody does. Neither changes a cap, rate, cliff, or notice
period, and both are stated here rather than left to a deployment to invent.

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

M_m = min(A_m, hard_ceiling)               while the volume input is fresh
M_m = min(silence_floor, hard_ceiling)     after silence_grace of silence, if a
                                           floor was declared at creation
M_m = 0, and the release is refused        otherwise

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

## When nobody is reporting

Eligible volume is not observable on chain, so the covenant depends on a party
that reports it. That party can lose its key, or simply stop. Two mechanisms
answer that, and their order matters:

1. **Rotation is the repair.** The declared policy authority may propose a
   replacement reporter. The proposal takes effect only after a public notice
   of at least 90 days, and it restores *who may report*, never *what was
   reported*: the incoming reporter must speak before any release resumes, and
   a stale input stays stale across the change. A rotation therefore cannot by
   itself reopen a window.
2. **A declared floor is the backstop**, for the case where the authority that
   would rotate the reporter is gone too. It must be declared when the policy
   is created or not at all, and it engages only after a silence longer than
   the rotation notice — so replacing the reporter is always the faster path.

A floor is an explicit, named exception to "no data means no release". It is
therefore constrained rather than merely disclosed:

- it must be small enough that withholding a report is never preferable to
  making an honest one, which is what stops the reporter from choosing silence;
- its grace period must exceed the rotation notice;
- it is a fixed number chosen in advance, never a fallback to the last reported
  volume. A fallback would turn reporter failure into a free release window.

A policy that declares no floor fails closed permanently on a lost reporter.
That remains the default, and it remains a legitimate choice.

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
- the volume reporter may be replaced only by the declared policy authority,
  only through a proposal that has served its public notice, and only in a way
  that leaves the reported figure and its timestamp untouched;
- no authority may report a volume on the reporter's behalf, set the reported
  figure directly, or shorten the rotation notice;
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
5. any release with a fresh report of zero eligible volume. A declared silence
   floor is a stated exception and applies only where there is no fresh report
   at all; a policy without one refuses in both cases;
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
