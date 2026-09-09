# E-07: withdrawal recovery enforced in an isolated SBF candidate

2026-09-09 · **LOCAL TEST_ONLY ENGINEERING PASS / PRODUCTION OPEN**

E-06's Founder/Treasury authority model is now implemented in a separate
`launch-vault-v5` program. Real signed local transactions recover lost beneficiary
keys, preserve original custody and Treasury approvals, and continue bounded
withdrawals. A Python verifier independently reconstructs raw policy, proposal,
key-index, token and actual loader bytes. **This does not retrofit or deploy v4,
launch a token, or adopt production recovery rights.**

The test program ID is `EUhWSZAfU8hDki7AXskYrwh8ErwXN8iqicaHTP7yQfYS`.
It is derived from a publicly reproducible test seed, not a production signer
or official mint. Both build hashes, ABI sizes, source bindings and boundaries
are frozen in `spec/LAUNCH_V5_TEST_ONLY_CANDIDATE_v1.json`.

## What changed

| Boundary | Enforcement in the new candidate |
|---|---|
| Stable identity | Initial actors, exact financial config, all three recovery committees, 180-day cliff, 90-day notice and execution window are identity-bound; new program/domain/PDA prefixes isolate v5 from v4 |
| Current authority | Two separate current-key/epoch/nonce/pending records live in the policy; original Founder/Treasury identities and vault `authority` identity fields never rotate |
| Role consent | Initial Founder/Treasury sign the complete new policy before deposits; three distinct nonzero backups per role, excluding its own initial operating key |
| Lost-key recovery | That role's two distinct backups and accepting successor sign; controller/oracle recovery gives no implicit withdrawal permission |
| Normal rotation | Current role key and successor accept; current key can cancel only this normal route |
| Waiting and pause | 90 full days; affected role cannot withdraw, and Treasury cannot approve new needs while its proposal is pending; the other role's reservation is not borrowed |
| Execute/cancel/expire | Permissionless execution during days 90–120; successor or role quorum may cancel; anyone may close at/after day 120; fresh proposals spend a new nonce and restart the full wait |
| Replay | Role-bound canonical proposal account, nonce, current epoch/predecessor, immutable proposal fields and permanent terminal status; old withdrawal keys/epochs cannot authorize releases |
| Recipient history | Permanent per-key PDAs record used-role bits, pending-role bits and whether the key has ever been an approved recipient; no reset/close instruction |
| Treasury continuity | Approval retains original period, exact token address/owner, need, consumed amount, notice, author and author epoch; the recovered signer can continue that approval but cannot rewrite it |
| Financial rules | The v4 capacity module is byte-identical after type-name normalization; actual transfer instructions still enforce original principal, T0/cliff, fresh report, period/annual caps, reserved quotas, approved need and SPL bindings |

Known initial/retired/current beneficiary keys, registered beneficiary guardians
and pending successors cannot receive Treasury payments. A historical approved
recipient cannot become a new operating key. Canonical key records make these
lookups constant-size per instruction; there is no unbounded on-chain scan.
Permissionless empty key-record preparation carries no recovery or withdrawal
right. Even prefunding the PDA with SOL cannot take over its identity.

The policy stores an approval count, letting the review profile require every
approval, including unconsumed future approvals. The independent decoder also
requires every withdrawal proposal through both role nonces and every key index
needed to reconstruct those proposals/approvals. Arbitrary unused empty indexes
are outside the required history.

## Lifecycle and custody

Recovery proposals may begin in PREPARED, ARMED or ACTIVE. While a role is
pending, deposits for that role also reject: new principal must be accepted by
an unpaused current signer. Execution before T0 changes who may co-sign a later
deposit, while its vault retains the original identity. Arming does not bypass
pauses; activation at T0 does not shorten the notice or unlock the Founder pool.

Pre-T0 cancellation still requires the current controller and current Founder
and Treasury signatures; a pending recovery does not block that exit. Unarmed
expiry at T0 and refunds to the original depositor remain available. A cancelled
policy cannot create or execute a new recovery or release tokens. Existing
proposal cancellation/expiry may clean up its terminal records without moving
funds. If keys needed for consensual cancellation were already lost, the
original lifecycle constraints still apply; no new unilateral exit is invented.

A single operating key may fill several roles, but role/vault PDAs remain
separate. Signed tests recover Founder and Treasury using only their registered
backup scopes and an unrelated fee payer, without the missing key even paying
fees. The Controller identity remains unchanged by those instructions.

Founder release destinations must be owned by the **current** Founder key.
Previously released tokens remain in their original accounts; recovery does
not retrieve or seize them. The native rehearsal creates a new SPL destination
through signed instructions and includes both old and new accounts in supply
conservation. Treasury continues to its original third-party token account.

## Exact signed notice in this test profile

`propose_withdrawal` includes `created_at` in its signed instruction and requires
it to equal the executing bank's Clock. Stored execution/expiry times are then
exactly `created_at + 90 days` and `created_at + 120 days`. This makes successor
acceptance bind the exact notice with no backdating. It is intentionally a
**local TEST_ONLY exact-clock submission interface**. A future network client
must handle clock movement and fresh signatures; this tranche does not establish
reliable public-RPC proposal submission or silently widen that acceptance rule.

The profile retains E-06's tradeoffs: malicious recovery quorums can eventually
redirect permitted withdrawals or repeatedly pause a role; cancellation/expiry
restores the old key's operations, which is risky if it was stolen. If both the
current key and two backups are lost, there is no hidden administrator rescue.
Distinct keys do not establish distinct people. An undisclosed fresh wallet
still cannot be classified as a related party by public-key comparison.

## Signed native-loader rehearsal

`tests/support/e07.rs` starts a local bank, deploys v5 with signed loader buffer
writes and `DeployWithMaxDataLen`, and permanently removes the local program's
upgrade authority. Signed System/SPL instructions create the mint/accounts, mint
one billion whole test tokens at nine decimals, revoke mint authority and fund
the 300-million Founder / 500-million Treasury pools. Freeze authority starts
absent. No program, mint or token account is injected in this combined rehearsal.
Clock/slot control and SOL fee airdrops remain explicit fixture setup.

| Checkpoint | Result |
|---|---|
| `partial_before` | Period 6: Founder has released 100,000 and Treasury 150,000 whole test tokens; future approvals for periods 9 and 13 already exist |
| `recovery_pending` | Separate accepted recovery proposals pause both roles; further withdrawals and Treasury approval attempts reject |
| `notice_minus_one` | Execution one second before the notice ends rejects |
| `before_execute` | At period 9, fresh capacity is reported; old accounting and original approvals remain stored |
| `after_founder` / `after_treasury` | Permissionless execution changes only each role's authority state/proposal/key index; vault, token, approval bytes and financial policy fields remain equal |
| `continued_period_9` | Old keys, stale epoch and old Founder destination reject; new keys release another 100,000 / 150,000 under the original Treasury approval |
| `normal_cancelled` | A normal rotation is cancelled, retaining its spent nonce and terminal record |
| `normal_pending_expiry` | Another normal rotation pauses only Founder; no unused reservation becomes a Treasury bonus |
| `expired` | At period 13, execution is too late; permissionless expiry closes the proposal without rotating or resetting accounting |
| `continued_year_two` | Both pools resume under the second frozen annual input interval; final lifetime totals 300,000 / 450,000, with only 100,000 / 150,000 charged to the new year |

The committed example contains **11 checkpoints**, **639 submitted signed local
transactions**, **628 successful transactions**, and 11 intended refusals. These
counts exclude fee airdrops and controlled sysvar updates. All six token balances
sum to the original `1e18` base-unit supply. No sale, LP behavior or full 12/8
allocation lifecycle is simulated.

V5's native rehearsal uses an immutable local target so the verifier can pin one
exact code image. The production choice between an immutable target and a
controlled upgrade route remains open. E-04/E-05's v4/gate sources, byte pins,
actual loader-upgrade evidence and historical receipts remain unchanged.

## Acceptance and reproduction

- 58 scoped Rust checks: 2 library checks, 3 compiled-ABI/identity checks and 53
  signed SBF scenarios, including the complete native-loader rehearsal.
- The scenarios include pre-cliff recovery followed by exact 180-day/cap-plus-one
  checks, role-isolated pauses, current/successor consent, quorum substitution,
  missing acceptance, key-index squatting, cancelled/stale proposals, expiry,
  shared-key role separation, cancellation/refunds and consumed-budget continuity.
- 15 new Python adversarial checks; full Python 112 passed and one opt-in skip.
  Unknown owners, layouts, role histories, key masks, missing approvals, altered
  recipients, code/authority substitution and broken continuity fail closed.
- Four v5 JavaScript identity tests bind every committee/key/order alongside all
  original actors, financial inputs and constants. Full JS identity suite: 15.
- Compiler-generated IDL, clippy with warnings denied and exact SBF hash gates.
  The initial SBF stack diagnostic was fixed by boxing release account wrappers;
  stack diagnostics are a failing reproduction gate, even if the compiler exits 0.

Using the existing frozen Rust 1.89.0 / Solana CLI 3.1.10 / SBF tools v1.52 setup:

```sh
bash tools/run_e07_local_reproduction.sh
```

The v5 GitHub Actions job runs that same command. Native/SBF build targets must
be different. `K4V_E07_SKIP_BUILD=1` reuses local outputs but never skips their
hash verification. Fresh runtime results go to `target/`.

To inspect the committed evidence without a wallet or Rust:

```sh
PYTHONPATH=src python3 src/e07_verifier.py examples/e07_rehearsal_bundle.json
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v5*.py' -v
```

Raw bytes are reversibly compressed and rehashed before independent decoding.
The accepted profile is an ACTIVE dual-pool graph, one original depositor, one
Treasury destination, up to eight Founder destinations, up to 64 approvals and
256 proposals per role. It verifies supplied bytes and the supplied pinned
immutable loader graph; it does not authenticate transaction history, a public
chain, or an arbitrary portfolio. Same-timestamp historical event ordering is
bounded by state/epochs, not proved from transaction signatures. Author-run
Python independence is not an unrelated human security audit.

## Next: E-08 operational verification and review handoff

The local v5 candidate now closes the tested beneficiary-withdrawal recovery gap
at SBF level. Existing v4 deployments/accounts are unaffected and still lack this
feature. Next prepare a read-only v5 account-export path with explicit expected
network/account identities and complete history, exercise its RPC boundary with
local fixtures, and extend the review handoff to this exact candidate. Public
transactions and migrations require their own concrete scope; no public v5
RPC/deployment claim is made here.

Named human review, actual IRB/annual inputs, production recovery/upgrade rights,
real demand replies and real cost quotes remain separate open work. The code
change does not appoint guardians, authorize issuance, or demonstrate buyers.
