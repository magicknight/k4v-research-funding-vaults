# Purpose Vault B2

Status: SPECIFICATION / LOCAL IMPLEMENTATION IN PROGRESS / NOT DEPLOYED

B1 is the smallest custody mechanism that can replace one beneficiary's verbal
promise to release slowly. B2 is the smallest mechanism that can replace a
second, harder promise: *this pool is bound to a purpose, and the two pools
together cannot exceed what the market can absorb.*

The difference is not size. B1 answers a clock question — has enough time
passed — and a clock is readable on chain. B2 answers a **purpose** question,
and purpose is not readable on chain. So B2 does not try to judge whether a
release is genuinely for the stated purpose. It enforces that an approval
exists, that it was recorded before the fact, that the release does not exceed
it, that the approver is not the payee, and that both pools together stay under
a market-capacity ceiling derived from a declared, dated input.

## Relationship to B1

B1 stays exactly as deployed. Its devnet receipt continues to attest what it
attested. B2 is a separate program that supersedes B1 functionally; it does not
modify, upgrade, or wrap it.

One consequence must be stated plainly rather than discovered later:

> **The deployed B1 program cannot participate in a B2 joint cap.** B1 has a
> two-instruction surface with no knowledge of any aggregate, and it is not
> upgradeable in a way that would add one. A joint cap is only complete across
> vaults managed by *this* program. A production deployment that wants the
> aggregate guarantee must place both pools in B2.

For that reason B2 implements **both** vault kinds — beneficiary and purpose —
in one program sharing one capacity window. A single-kind B2 could not enforce
the covenant's aggregate rule at all, and shipping one that appeared to would
be worse than shipping none.

## Deliberate difference from B1: one shared period clock

B1 anchors its 30-day periods at its own `cliff_end_ts`. Two independently
deployed B1 vaults therefore disagree about when "this month" starts, and an
aggregate over them is undefined.

B2 anchors **every** period at the policy's `genesis_ts`. The shared window and
every vault bound to it index the same 30-day windows:

~~~text
period_index = floor((now - policy.genesis_ts) / 2592000)
~~~

The beneficiary cliff remains a separate gate on top of that clock, not the
clock itself.

The fixed-second convention is still not being represented as calendar-month or
calendar-year accounting. That semantic bridge remains open, exactly as in B1.

## Protected property

For every successful release, with `amount > 0`:

~~~text
now - market.updated_at <= market.max_age_seconds        # and market.updated_at > 0
market_capacity = floor(market.eligible_volume * market.market_capacity_bps / 10000)

vault.released_this_period  + amount <= vault.monthly_cap
vault.released_total        + amount <= vault.deposited_amount
policy.released_this_period + amount <= market_capacity

monthly_cap = floor(floor(deposited_amount * annual_release_bps / 10000) / 12)
~~~

For a **beneficiary** release, additionally:

~~~text
now >= vault.cliff_end_ts
destination.owner == vault.authority
~~~

For a **purpose** release, additionally:

~~~text
approval.vault        == vault
approval.period_index == period_index
approval.destination  == destination
approval.consumed + amount <= approval.approved_need
now - approval.created_at >= 2592000                     # 30-day notice
~~~

`policy.released_this_period` is a single counter debited by every vault on the
policy. When the period index advances, both the shared counter and the vault
counter reset to zero. Unused capacity is never an input to a later period.

## Accounts

~~~text
policy   = PDA("purpose-policy",   policy_hash)
market   = PDA("purpose-market",   policy_hash)
vault    = PDA("purpose-vault",    policy_hash, kind, authority, mint)
token    = PDA("purpose-token",    vault)
approval = PDA("purpose-approval", vault, period_index_le)
~~~

`policy_hash` is a non-zero 32-byte digest binding the deployment to an
externally published policy artifact. B2 checks its shape and freezes it; it
does not decide whether the referenced prose is true or legally effective.

## The market input is an oracle, and it fails closed

`market_capacity` depends on eligible trailing 30-day spot volume, which is not
observable on chain. B2 therefore carries a `MarketInput` account written by a
single frozen oracle key.

Three rules make this conservative rather than convenient:

1. **A stale input rejects the release.** It does not fall back to the previous
   value. A fallback would convert oracle failure into a free release window,
   which is the exact opposite of the covenant's intent.
2. **An unreported input rejects the release.** `updated_at` starts at zero, so
   a freshly opened policy releases nothing until the oracle has spoken.
3. **Zero eligible volume rejects every positive release**, because it makes
   `market_capacity` zero. This is not a special case in the code; it falls out
   of the same inequality.

The oracle can only report volume. It cannot move funds, change a cap, change a
rate, or name a destination.

## No update surface

B2 has no update, configure, close, migrate, emergency-release, alternate
destination, or administrative transfer instruction. There is no way to change
the oracle, the approver, the beneficiary, a rate, a cliff, or a cap after
creation.

The cost of that is real and is accepted deliberately: **if the oracle key is
lost, no further release is possible and the deposits stay locked forever.**
That is the conservative direction of failure. The covenant permits an
emergency power to pause but never to accelerate, and a recovery path that
could restore releases would be an acceleration path wearing a different name.

## Conflict of interest

The covenant requires the founder to recuse when they are the payee. B2
enforces the structural half of that: **an approval whose destination token
account is owned by the approver is rejected.** The judgement half — whether an
approver is independent in substance, whether a related-party wallet is really
unrelated — is not decidable on chain and is not claimed.

## Notice

The covenant requires 30 days' public notice before a scheduled net release.
B2 makes the on-chain half mechanical: an approval must exist on chain for at
least 2,592,000 seconds before it can be consumed, and an approval may only be
created for a period strictly later than the current one. The announcement
itself — pool, purpose, budget, venue, expected stablecoin use — is off-chain
and is not enforced here.

## Rejection vectors

Against the numbered list in
[PURPOSE_BOUND_VAULT_COVENANT.md](PURPOSE_BOUND_VAULT_COVENANT.md):

| # | Vector | B2 |
|---:|---|---|
| 1 | beneficiary release before cliff | enforced |
| 2 | either vault over its monthly rate cap | enforced for both kinds |
| 3 | purpose release without approved need or above it | enforced |
| 4 | aggregate above market-capacity cap | enforced across every vault on the policy |
| 5 | any release with zero eligible volume | enforced |
| 6 | carry-forward of unused monthly capacity | enforced for both counters |
| 7 | bypass event omitted from economic circulation | **not enforced** — off-chain classification |
| 8 | emergency acceleration | no such instruction exists |
| 9 | authority inconsistent with declared deployment | **not enforced** — deployment-time check |
| 10 | artifact hash inconsistent with reviewed candidate | **not enforced** — verified-build check |

Vectors 7, 9 and 10 are honest gaps at this layer, not oversights. Vector 7 is
a classification question about events that happen outside the vault. Vectors 9
and 10 belong to the deployment and verified-build layer, where B1 already has
a working answer.

## What B2 does not establish

- It does not verify that a release was actually spent on the stated purpose.
  It verifies that an approval existed, was aged, was capped, and that the
  approver was not the payee. Truthfulness of purpose rests on the public
  budget and ledger, not on this program.
- `market_capacity_bps` and the definition of eligible volume are **test
  parameters**, not frozen production values. The covenant's `alpha` range of
  2–5% and the IRB data sources remain open until the parameter-freeze gate.
- The approval authority in tests is a single key. Production requires a
  multisig, and no treasury signers exist yet. B2 builds the mechanism, not the
  governance.
- No deployment, no audit, no mainnet, and no claim of production readiness.
