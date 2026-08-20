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
market.report_count > 0                                  # the oracle has spoken at least once
now - market.updated_at <= market.max_age_seconds        # or the silence floor below
absorption      = floor(market.eligible_volume * market.market_capacity_bps / 10000)
market_capacity = min(absorption, policy.hard_ceiling)          # fresh input
                = min(policy.silence_floor, policy.hard_ceiling) # declared floor,
                                                                # after the grace

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

`market.report_count`, not `market.updated_at`, is what says whether the oracle
has ever spoken. A timestamp cannot carry that meaning: zero is also a real
instant, and a report made at it would be indistinguishable from no report at
all. The distinction is not academic — the test fixture's clock starts at zero
and sat exactly on the collision.

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
rate, name a destination, or replace itself.

## Losing the oracle, and the two ways out

A frozen key is a single point of failure over a twenty-year release schedule,
and the failure is not only theft or loss: a reporter that simply stops has the
same effect. B2 carries two answers, in a deliberate order.

**Rotation is the repair.** `propose_oracle` lets the policy authority — and
only it — name a replacement. The proposal is recorded on chain with its
timestamp and takes effect no sooner than 90 days later, through a permissionless
`execute_oracle_rotation`: the authorisation was the proposal, the public
already had its notice, and requiring the authority again would only add a way
for an aged, announced rotation to be silently withheld. There is no cancel
instruction; a second proposal replaces the first and restarts its clock, and
proposing the sitting oracle is how one is withdrawn.

A rotation restores **who may report, never what was reported**. It does not
touch `eligible_volume`, `updated_at` or `report_count`, so a stale input stays
stale across the change and the incoming oracle must speak before any release
resumes. That is what keeps rotation from being a release path.

The rotation authority is worth stating plainly: whoever holds it can name
themselves and report an inflated figure. That is bounded, and bounded by
things they cannot reach — the per-vault caps, the cliff, the approved need and
the notice period — so the widest window they can open still leaves the sum of
the frozen caps as the bound on the period. `hard_ceiling` bounds it further
where one is set. The 90-day notice is what makes the attempt visible before it
binds.

**A declared floor is the backstop**, for the case where the authority that
would rotate the oracle is gone as well. `silence_floor` and
`silence_grace_seconds` are frozen at `open_policy`; the floor defaults to zero,
which means the policy fails closed on a lost oracle exactly as before. Where
one is declared:

- the grace period is bounded to between 180 and 730 days, so it always exceeds
  the 90-day rotation notice and replacing the oracle stays the faster path;
- a floor without a grace period, or a grace period without a floor, is
  rejected: a floor that engaged the instant an input went stale would be a
  fallback in everything but name;
- the floor is a fixed number chosen in advance, never the last reported
  volume, and it is bounded by `hard_ceiling` like any other window;
- it releases a trickle, not an income. Its purpose is that a lost oracle does
  not mean a permanently sealed vault, not that operations continue.

An honest report puts the ordinary window straight back. The floor is a
backstop, not a ratchet.

`eligible_volume` is denominated in **mint base units**, not in a quote
currency. The unit is stated here, in the covenant, and on the account itself
because "spot volume" conventionally means a quote-currency figure, and a
report in USD would rescale every ceiling this account feeds without any code
noticing. Base units also keep price out of the comparison — `market_capacity`
and `vault.monthly_cap` are then the same kind of quantity — and let separate
venues be summed without a per-venue price.

## What a compromised oracle can and cannot do

Worth stating exactly, because the intuition runs the wrong way. A captured
oracle key reports an inflated volume, `market_capacity` grows, and the
aggregate rule stops binding. It cannot do anything else: `vault.monthly_cap`,
`cliff_end_ts`, `approval.approved_need` and the 30-day notice are all derived
from data the oracle never touches. So the widest window any report can open
still leaves the sum of the frozen per-vault caps as the bound on the period —
the oracle can only ever slow a release down, never speed one past the
schedule. `an_inflated_oracle_report_cannot_lift_a_release_past_the_frozen_schedule`
spends an inflated window down to that bound and shows the next base unit
refused by a per-vault gate.

`policy.hard_ceiling` is an absolute bound in base units, frozen at
`open_policy` and inert at `u64::MAX`. It adds nothing against a compromised
oracle — the paragraph above already holds without it. It exists because it is
the only term in the window that **no** key can move, so a deployment wanting
to promise a bound tighter than its own release schedule has somewhere to say
it, and because B2 has no update instruction: a ceiling that is not set at
creation can never be added. Zero is rejected, since it would open a policy
that can never release and can never be repaired.

## No update surface

B2 has no update, configure, close, migrate, emergency-release, alternate
destination, or administrative transfer instruction. **The oracle is the one
exception, and it is a noticed one:** it can be replaced through the 90-day
rotation above, which is why the surface is eight instructions rather than six.
There is no way to change the approver, the beneficiary, a rate, a cliff, a
ceiling, a floor, or a cap after
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
| 4 | aggregate above market-capacity cap, or above a frozen absolute ceiling | enforced across every vault on the policy |
| 5 | any release with a fresh report of zero eligible volume | enforced; a declared silence floor is a stated exception where there is no fresh report at all |
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
- `silence_floor` is zero in every test fixture except the one that exercises
  it, so B2's default remains "a lost oracle locks the vaults permanently". No
  production value has been chosen, and like the ceiling it cannot be added
  after policy creation.
- The rotation authority is the policy authority, which in every fixture is a
  single key. A rotation is only as trustworthy as whoever holds it, and the
  90-day notice bounds the damage rather than preventing it.
- `hard_ceiling` is inert in every test fixture except the one that exercises
  it. Whether a deployment sets one, and at what number, is a policy decision
  that belongs with the same parameter freeze. B2 provides the field because it
  cannot be added afterwards, not because a value has been chosen.
- Which venues count toward eligible volume, and which addresses are excluded
  from it, is decided off chain. B2 receives one integer and does not know how
  it was assembled. Denominating that integer in base units removes the price
  from the aggregation but not the judgement.
- The approval authority in tests is a single key. Production requires a
  multisig, and no treasury signers exist yet. B2 builds the mechanism, not the
  governance.
- No deployment, no audit, no mainnet, and no claim of production readiness.
