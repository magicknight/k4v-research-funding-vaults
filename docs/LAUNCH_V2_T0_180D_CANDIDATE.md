# Launch v2: T0 and 180-day local candidate

Status: **TEST_ONLY, no public-chain deployment, no production admission profile**.
This implements the lifecycle and time rules in the next engineering tranche.
It does not adopt final allocation, annual IRB, oracle recovery, controller
recovery or upgrade governance. These remain open; neither this document nor a
passing test is a launch or audit approval.

## Isolation and identity

`programs/launch-vault-v2` is a third, separate program. B1/B2 account layouts,
program IDs, 730-day code, historical receipts and frozen candidate files remain
separate. No migration, upgrade or reinterpretation of an old account occurs.
The v2 program ID `FuAkHvCjsjLjMPusJzPWue2jbWNFdZX5dc4y78hojeaN` is a
deterministic local fixture address, derived from SHA-256 of
`k4v-launch-vault-v2-test-only-program`; no deployment keypair is provided.

The default build rejects `open_policy` with `ExperimentalProfileDisabled`.
Only a build explicitly enabling `test-profile` can admit a policy. That build
is for local experiments and retains the known shared-capacity starvation
limitation. The flag is not a runtime production switch. Default admission
closure is not a general freeze of existing test accounts or a substitute for
production upgrade controls.

Policy identity is SHA-256 of the concatenation below, with no separators:

1. UTF-8 `k4v-launch-policy-v2-test-profile-1`;
2. program ID, creator, mint, founder, treasury controller, oracle: six 32-byte public keys;
3. 32-byte nonzero specification hash;
4. cliff and period: two signed little-endian i64 values, 15,552,000 and 2,592,000;
5. the exact 56-byte Borsh `LaunchConfig`.

The config field order is `t0: i64`, `founder_amount: u64`,
`treasury_amount: u64`, `founder_period_cap: u64`,
`treasury_period_cap: u64`, `shared_hard_cap: u64`, `max_report_age: i64`.
Amounts are mint base units. Client integer inputs must use bigint or decimal
strings; JavaScript Numbers are rejected by the offline identity codec.
Creator, founder and treasury consent at policy creation; creator, depositor
and the designated role owner consent at deposit. One person may hold all
three roles. This does not claim independent governance.

| Account | PDA seeds after program ID | Binding |
|---|---|---|
| `LaunchPolicyV2` | `launch-v2-policy`, identity | All actors, mint, spec, exact config, fixed time constants |
| `LaunchVaultV2` | `launch-v2-vault`, policy, role u8 | Role 0 founder or 1 treasury; original depositor and principal |
| SPL vault token account | `launch-v2-token`, vault | Classic SPL Token, policy mint, vault PDA authority |
| `TreasuryApprovalV2` | `launch-v2-approval`, policy, period u64 LE | One immutable budget per period, recipient address and owner |

The role namespace admits exactly one founder pool and one treasury pool.
Neither duplicate deposits nor a third role can attach to the shared window.
An unrelated creator can make their own candidate, but cannot pre-empt the
published identity or obtain the required official signatures. This cannot
stop someone issuing an unrelated token with a copied name; official mint
authentication remains a separate publication task.

## Lifecycle and exits

| Instruction | Required role signatures | Effect and boundary |
|---|---|---|
| `open_policy(config, spec_hash, identity)` | Creator, founder, treasury | TEST_ONLY build; future immutable T0; PREPARED |
| `deposit(role, amount)` | Creator, original depositor, role owner | PREPARED and now < T0; exact configured principal; both mint/freeze authorities None |
| `arm()` | Creator | PREPARED, both exact pools funded, now < T0 → ARMED |
| `activate()` | No role signature | ARMED and now ≥ T0 → ACTIVE; transaction payer still signs |
| `cancel()` | Creator, founder, treasury | PREPARED or ARMED, now < T0 → CANCELLED |
| `expire_unarmed()` | No role signature | PREPARED and now ≥ T0 → CANCELLED; cannot expire an ARMED policy |
| `refund()` | Original depositor | CANCELLED only; transfer token balance to any same-mint account owned by that depositor |
| `report_capacity(capacity, observed_at, sequence)` | Frozen oracle | ACTIVE; current-period observation, fresh, nonfuture, monotone sequence/time |
| `approve_treasury(period, need)` | Treasury controller | Noncancelled policy; immutable recipient and owner, nonself, future period after T0 |
| `release(amount)` | Frozen vault role owner | ACTIVE, now > T0; role rules, current report, own cap and shared cap |

Successful deposits alone set the funding mask; no PREPARED withdrawal exists.
Cancelled policy/vault accounts remain as tombstones, so the same address
cannot be reopened or retimed. Refunds include unsolicited token dust. A later
dust transfer can be refunded again, to the same depositor. A cancelled pool
cannot release normally. Refund does not require the original source token
account to remain open. SOL account rent remains in the tombstones.

Every mutation rejects time earlier than the policy's last successful action.
The time comes from Solana Clock. Failed instructions, including a failed SPL
Token CPI after counter updates, must roll back all program-account data and
token balances (ordinary transaction SOL fees are outside this invariant).

## Time and TEST_ONLY economics

Both pools use the declared T0, irrespective of deposit or activation time.
Periods are `floor((now - T0) / 2,592,000)`; period 0 begins at T0. At exactly
T0 activation is allowed but both release paths reject. Founder release starts
at **T0 + 180 × 86,400 seconds**, period 6, subject to caps. This is not six
calendar months and does not unlock the full founder principal. Missed periods
and the first 180 days create no catch-up entitlement.

Treasury release requires a notice aged at least 30 days, the approved period,
the same token-account address and owner, nonself recipient, and remaining
approved need. The **local candidate** permits a pre-T0 notice for period 0,
so a sufficiently early notice can mature for T0+1. This is not a production
choice of the first treasury payout date. After T0, new approvals must target
future periods. A notice that cannot mature before its period ends rejects.

The oracle supplies a direct capacity in base units for these fixtures. There
is no asserted market-volume derivation, annual IRB, annual cap, source-quality
guarantee or fallback. Reports need a strictly increasing sequence and current
period timestamp; stale/missing reports block release. Capacity is bounded by
the immutable shared hard cap. A report change never resets used amounts.
Period rollover resets only current-period counters; principal and lifetime
released amounts persist. Checked arithmetic rejects overflow.

**Known limitation K4V-04 remains:** the TEST_ONLY pool rule is the legacy
first-come shared ceiling, not reserved F:T quotas. An executable witness shows
treasury can exhaust a period's shared capacity before founder. E-03 must
implement the adopted reservation/annual model and a new verifier before any
production candidate exists. The new program also has no oracle/controller
rotation or recovery yet; E-04 must address those and upgrade control.

## Interfaces and reproduction

- `idl/launch_vault_v2.json` is compiler-generated with Anchor 1.1.2; account
  discriminators and sizes are compared against the compiled Rust ABI.
- `probes/launch_v2_identity.mjs` is an offline hash/config codec. Rust,
  JavaScript and an independently constructed Python byte vector agree on
  `spec/LAUNCH_V2_IDENTITY_VECTOR_v1.json`.
- `spec/LAUNCH_V2_TEST_ONLY_CANDIDATE_v1.json` freezes the two SBF artifacts and
  IDL bytes. It describes experimental artifacts, not a production config.
- `evidence/LAUNCH_V2_LOCAL_VALIDATION_2026-09-09.json` records the completed
  checks and limitations. Mint/token accounts in LiteSVM are fixtures; token
  movement, authority revocation and all v2 calls execute as signed SBF/SPL
  transactions. This is not a real-loader or independent-RPC reproduction.

With Rust 1.89.0, Solana CLI 3.1.10 and platform tools v1.52:

```sh
NO_DNA=1 cargo build-sbf --tools-version v1.52 --manifest-path programs/launch-vault-v2/Cargo.toml --sbf-out-dir target/v2-disabled -- --locked
NO_DNA=1 cargo build-sbf --tools-version v1.52 --manifest-path programs/launch-vault-v2/Cargo.toml --sbf-out-dir target/v2-test -- --locked --features test-profile
CARGO_TARGET_DIR=/tmp/k4v-native-target cargo test -p launch-vault-v2 --locked --features test-profile
CARGO_TARGET_DIR=/tmp/k4v-native-target python3 tools/build_launch_v2_idl.py --check
node --test probes/launch_v2_identity.test.mjs
python3 tools/verify_launch_v2_artifacts.py
```

Keep native Cargo output separate from SBF output. The dedicated CI workflow
builds both profiles, rejects stack-overflow diagnostics, checks exact SBF/IDL
hashes and runs the lifecycle suite. Existing B1/B2 CI continues separately.

Implementation references: [Anchor account constraints](https://www.anchor-lang.com/docs/references/account-constraints)
and [Solana token authority operations](https://solana.com/docs/tokens/basics/set-authority).
