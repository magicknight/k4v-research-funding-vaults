# Launch v3: reserved capacity and annual-input test candidate

Status: **TEST_ONLY**. This is the E-03 engineering candidate. It implements
the proposed F:T reservation and bounded annual-input interface; it does not
adopt production allocation rights, IRB sources, a calendar, update governance,
recovery or upgrade controls. No public-chain deployment or token issuance is
part of this change.

## What changed

The v2 fixture could let treasury consume a whole shared period before founder
acted. The new `launch-vault-v3` program computes a separate quota for each
pool. With fixed inputs, valid withdrawals from one quota cannot spend the
other. A transaction-order regression exercises both orders for six periods.

The v3 program and account namespace are separate from B1, B2 and v2. All older
source, SBF/IDL pins and historical receipts remain unchanged, including v2's
explicit starvation witness. The local fixture program ID is
`AhTz3JFbaEvk1PMxEsmG89YiTZQJ4ALKoFc8vyr1Cf8m`, the base58 encoding of SHA-256
of `k4v-launch-vault-v3-reserved-test-only-program`. No deployment keypair is
provided. The default build rejects `open_policy`; explicit `test-profile`
enables local candidates. There is no production-admission flag or migration.

V3 keeps the common T0, 180-day founder cliff, two exact role-bound deposits,
classic SPL Token mint/freeze revocation, consented cancellation and original
depositor refund paths. Treasury approval still requires a 30-day notice,
correct period and recipient account **and owner**. A solo owner can hold
multiple roles; that is not independent governance.

## Full-period reservation

Let F and T be the two **full-period** ceilings, and C the **full-period**
shared capacity. After founder's cliff:

```text
A  = min(C, F + T)
qF = floor(A * F / (F + T))
qT = A - qF
```

Both quotas are zero when F+T=0. Before the cliff, qF=0 and qT=min(C,T).
Amounts use mint base units. The sum and multiplication use u128; results
fit u64. Treasury receives the fractional rounding remainder, less than one
base unit relative to the exact proportional amount.

Each request requires `used_i + amount <= q_i`. Unused quota expires at the
period boundary and cannot be borrowed. The other pool can still receive zero
when shared capacity is tiny, its own cap is zero, the annual budget is
exhausted or founder is still locked. The guarantee is protection of a positive
**current-period** reservation, not guaranteed income or a reserved annual
minimum. Pre-cliff treasury spending still counts against its applicable
shared annual budget.

The following are computed from counters at the period's start:

```text
annual_i       = floor(frozen_basis_i * frozen_rate_bps / 10000)
annual_prior_i = annual_used_i - current_period_used_i
life_prior_i   = lifetime_used_i - current_period_used_i
F or T         = min(configured_period_cap_i,
                    floor(annual_i / 12),
                    annual_i - annual_prior_i,
                    principal_i - life_prior_i)
C              = min(report_capacity, frozen_shared_hard_cap,
                     annual_shared_cap - sum(annual_prior_i))
```

At a period rollover, current-period used amounts are projected as zero.
At an annual input boundary, annual used amounts are also projected as zero;
lifetime amounts remain. Annual input boundaries must align to a period in
this test profile, so a new annual epoch cannot start midway through a period.
The division by 12 is the existing reference rate convention, **not** an
assertion that a production year has twelve 30-day windows. An explicit
13-window fixture demonstrates why a separate annual cap is necessary.

The formula does not subtract current-period spending twice. Repeated partial
withdrawals therefore preserve F:T weights under the same report and rule.
The actual per-pool annual totals, lifetime principal and shared capacity are
also enforced when consuming the resulting quota.

## Corrected reports

Reports remain signed by the frozen oracle and must have increasing sequence
numbers and current-period observations. New reports never reset counters.
If a new C gives either pool a quota below its already-used amount, **both
pools pause**. A new fresh report that covers both used amounts can resume
the remaining quota; otherwise the next period begins without carry.

Already transferred tokens cannot be recalled by lowering a report. A paused
state may legitimately have used amounts above the latest corrected quota.
The verifier reports that state as consistent but gives both pools zero new
release capacity. It does not reject the state merely to hide the correction.

## Annual-input interface and its limits

The frozen `LaunchConfig` contains exactly two `AnnualRule` records:

| Field | Encoding | Meaning in this candidate |
|---|---|---|
| `start_period`, `end_period` | u64 LE each | Inclusive/exclusive explicit period range |
| `founder_basis`, `treasury_basis` | u64 LE each | Frozen test bases, bounded by initial principals |
| `shared_cap` | u64 LE | Aggregate budget for this interval |
| `release_bps` | u16 LE | Test rate from 0 through 500; zero means zero |
| `source_hash` | 32 bytes | Nonzero commitment to an input artifact; not proof of its truth |

The ranges must be contiguous, start at period 0 and avoid arithmetic overflow.
The aggregate budget cannot exceed the sum of the two calculated annual caps.
All fields are bound into policy identity and consented to at creation.
There is no update instruction, report-based reset, silent extension, or
fallback to 500 bps. Outside the two declared ranges release fails closed.

This is a bounded **input-interface experiment**, not a dynamic production IRB
service. The local 12/13-window cases do not choose 360 days, 390 days or a
calendar year for production. Actual source selection, retrieval dates,
year-start basis derivation, nonaligned calendar boundaries, later-year input
publication, notice and update authorization remain open. The current source
hashes are visibly synthetic fixture bytes. Never fund this finite-horizon
candidate with real assets.

## ABI and independent reconstruction

V3 uses the same ten instruction names as v2 and new discriminators for
`LaunchPolicyV3`, `LaunchVaultV3` and `TreasuryApprovalV3`. PDA prefixes are
`launch-v3-policy`, `launch-v3-vault`, `launch-v3-token` and
`launch-v3-approval`; the remaining seeds retain the v2 roles and ordering.

The config is 204 bytes: the v2 seven-field 56-byte prefix followed by two
74-byte annual rules. Policy state adds both pool period/lifetime/annual
counters and the annual index, for **544 bytes including its discriminator**.
Vault and approval accounts are 138 and 137 bytes respectively. Compiler-
generated `idl/launch_vault_v3.json` and Rust ABI checks pin these layouts.

Identity is SHA-256 of the following concatenation:

1. UTF-8 `k4v-launch-policy-v3-test-profile-1`;
2. program, creator, mint, founder, treasury and oracle public keys, 32 bytes each;
3. the 32-byte specification hash;
4. CLIFF i64 LE, PERIOD i64 LE, rate divisor 12 u64 LE, ceiling 500 u16 LE;
5. the exact 204 config bytes.

The frozen vector agrees across Rust, JavaScript and independently assembled
Python bytes. JavaScript accepts bigint/decimal strings and rejects lossy
Numbers, out-of-range values and malformed annual records.

`src/launch_v3_verifier.py` independently decodes raw bytes without the IDL or
Rust math. It verifies owners, discriminators, PDAs, config commitments,
escrow authorities, role bindings, period/annual/lifetime accounting, custody,
the current approval and full token conservation. It then reconstructs the
two reserved amounts and reports ceilings before transaction signatures.

Its declared local graph contains ten accounts: policy, two vault states,
mint, a common depositor source, two escrow token accounts, two destinations
and one approval. It requires those five token accounts to be distinct and
reconcile the mint's supply. It is not yet a general explorer for arbitrary
multi-source funding or transaction history. The sample and CI inputs are
exported after actual signed SBF/SPL transactions; initial mint/accounts use
LiteSVM fixtures. A consistent supplied snapshot is **not authenticated chain
state**, verified deployed bytecode, a truthful IRB source or a human audit.

## Reproduce

Use Rust 1.89.0, Solana CLI 3.1.10, platform tools v1.52 and Anchor 1.1.2.

```sh
NO_DNA=1 cargo build-sbf --tools-version v1.52 --manifest-path programs/launch-vault-v3/Cargo.toml --sbf-out-dir target/v3-disabled -- --locked
NO_DNA=1 cargo build-sbf --tools-version v1.52 --manifest-path programs/launch-vault-v3/Cargo.toml --sbf-out-dir target/v3-test -- --locked --features test-profile
CARGO_TARGET_DIR=/tmp/k4v-native-target K4V_V3_SNAPSHOT_OUT="$PWD/target/v3-raw-snapshot.json" cargo test -p launch-vault-v3 --features test-profile --locked
PYTHONPATH=src python3 src/launch_v3_verifier.py target/v3-raw-snapshot.json
PYTHONPATH=src python3 -m unittest discover -s tests -p test_launch_v3_verifier.py -v
node --test probes/launch_v3_identity.test.mjs
CARGO_TARGET_DIR=/tmp/k4v-native-target python3 tools/build_launch_v3_idl.py --check
python3 tools/verify_launch_v3_artifacts.py
```

The dedicated CI job also fails on any SBF stack-overflow diagnostic and
checks the exact SBF/IDL bindings. Existing B1/B2 and v2 workflows continue
to verify their own unchanged artifacts. See the versioned candidate and
`evidence/LAUNCH_V3_LOCAL_VALIDATION_2026-09-09.json` for exact scope and hashes.

Next: production allocation/annual choices, E-04 oracle and controller
recovery plus upgrade control, then E-05 real-loader/RPC integration and
accountable external review. K4V-04 is addressed in this test candidate;
production adoption and K4V-05..07 remain open.
