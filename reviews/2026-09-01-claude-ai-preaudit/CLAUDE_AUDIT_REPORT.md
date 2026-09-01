# Independent Security Pre-Audit — `k4v-research-funding-vaults`

**Source identity (verified in-checkout):** `.git/refs/heads/main` = `e1afead138fbf56956b298ebae7a97a8ae9ad956`, matching the pinned commit.
**Scope:** `programs/purpose-vault` (B2), `programs/beneficiary-vault` (B1), plus `src/`, `tests/`, `probes/`, `tools/`, `spec/`, `docs/`, `evidence/`, CI.
**Method:** read-only static review of files present in this checkout. No build, no execution, no network, no RPC.

---

## 1. System map

### Entry points

| Program | ID (`declare_id!`) | Instructions |
|---|---|---|
| B2 `purpose_vault` | `2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w` (`programs/purpose-vault/src/lib.rs:12`) | `open_policy`, `report_volume`, `propose_oracle`, `execute_oracle_rotation`, `deposit`, `approve`, `release_beneficiary`, `release_purpose` (`lib.rs:20-81`) |
| B1 `beneficiary_vault` | `BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM` (`programs/beneficiary-vault/src/lib.rs:11`) | `deposit`, `release` (`lib.rs:17-35`) |

### Critical state

`PolicyWindow`, `MarketInput`, `CovenantVault`, `Approval` (`programs/purpose-vault/src/state.rs:25-113`); `BeneficiaryVault` (`programs/beneficiary-vault/src/state.rs:3-21`). PDA namespaces at `programs/purpose-vault/src/constants.rs:28-32` and `programs/beneficiary-vault/src/constants.rs:7-8`.

### Privileged actors and their on-chain powers

| Actor | Power | Bound by |
|---|---|---|
| `policy.authority` | propose an oracle rotation (`rotate_oracle.rs:26-47`); co-sign every `deposit` (`deposit.rs:18`) | 90-day notice; cannot move funds; **frozen forever, no transfer path** |
| `market.oracle` | write `eligible_volume`, unbounded, any value including 0 (`report_volume.rs:18-26`) | `hard_ceiling`; per-vault caps; **can halt all releases immediately** |
| beneficiary vault `authority` | release up to `monthly_cap`/period after the cliff (`release_beneficiary.rs:54-106`) | cliff, cap, deposit total, shared window |
| purpose vault `authority` (approver) | record approvals; release up to `approved_need` ∧ `monthly_cap` (`approve.rs:46-86`, `release_purpose.rs:60-141`) | 30-day notice, period match, `destination.owner != approver` |
| depositor | one-time deposit; no withdrawal path | — |
| **loader upgrade authority** | replace the entire program | **nothing on chain** |

### Fund flow

`depositor_token → vault_token (PDA)` on deposit; `vault_token → destination` on release, signed by the vault PDA. Three transfer sites, all `transfer_checked`: `purpose-vault/.../deposit.rs:95-107`, `release_beneficiary.rs:93-106`, `release_purpose.rs:128-141` (plus B1 `deposit.rs:74-86`, `release.rs:81-94`).

### Trust boundaries

On-chain enforced: caps, cliff, notice, period math, PDA binding, approval consumption. **Off-chain only:** the meaning of `policy_hash`, the honesty and unit of `eligible_volume`, approver independence, purpose truthfulness, mint authority state, upgrade authority state, and all governance.

---

## 2. Confirmed findings

### K4V-01 — MEDIUM — CONFIRMED — `open_policy` is permissionless and the policy PDA omits the authority, so any `policy_hash` can be permanently squatted

**Evidence.** `programs/purpose-vault/src/instructions/open_policy.rs:13-14` (`authority: Signer`, otherwise unconstrained); `:20-27` and `:28-35` — `policy` and `market` PDAs are derived from `[POLICY_SEED, policy_hash]` / `[MARKET_SEED, policy_hash]` **only**; `:48` is the sole content check (`policy_hash != [0;32]`). No close, migrate, or re-open instruction exists (`lib.rs:20-81`). `spec/PURPOSE_VAULT_B2.md:99-108` defines `policy_hash` as "a non-zero 32-byte digest binding the deployment to an externally published policy artifact".

**Violated assumption.** That the party which publishes a covenant artifact controls the policy account canonically derived from its digest.

**Exploit path.** The digest is, by construction, of a *published* artifact, so it is public before the policy is opened. An adversary calls `open_policy(policy_hash=H, …)` first with parameters of their choosing. They become `policy.authority` and set `market.oracle`, `market_capacity_bps`, `max_age_seconds`, `hard_ceiling`, `silence_floor`, `silence_grace_seconds` (`:73-95`) — all frozen with no update instruction. The intended operator's `open_policy(H, …)` then fails (`init` on an existing account) forever. Preconditions: only the digest and a few thousand lamports of rent.

**Impact.** Permanent denial of the intended policy address; the canonical on-chain artifact for the published document is a hostile one with an attacker-chosen oracle and ceiling. No funds at risk: `deposit` requires the policy authority's signature (`deposit.rs:18,32`), so the operator cannot accidentally fund the squatted policy without the squatter co-signing.

**Why existing checks/tests miss it.** `a_stranger_cannot_attach_a_vault_to_someone_elses_capacity_window` (`tests/b2_litesvm.rs:1640-1674`) covers the *deposit* gate, not policy creation. No test opens a policy from an unexpected signer.

**Repair (minimal).** Add the creator to the namespace: `seeds = [POLICY_SEED, authority.key().as_ref(), policy_hash.as_ref()]` (and the same for `MARKET_SEED`, propagating to the vault seeds via `policy.key()` instead of `policy.policy_hash`). **Architectural.** Require `policy_hash` to commit to the authority key, e.g. `require!(policy_hash == sha256(artifact_digest || authority.key()))`, so the digest itself is unforgeable.

**Retest.** Setup: fresh SVM, key A and key B. Action: `open_policy(H)` from B, then `open_policy(H)` from A. Expected today: A's transaction fails with an account-already-in-use error and `policy.authority == B`. Expected after repair: both succeed at distinct addresses, and A's policy is the one derivable from A's key.

---

### K4V-02 — MEDIUM — CONFIRMED — B1 `deposit` is permissionless, so the `(beneficiary, mint, policy_hash)` vault PDA can be squatted with dust

**Evidence.** `programs/beneficiary-vault/src/instructions/deposit.rs:15-47`: only `depositor` signs; `beneficiary` is `UncheckedAccount` (`:20`); `vault_state` seeds are `[STATE_SEED, beneficiary, mint, policy_hash]` (`:32`) with no depositor component. `:56-68` accepts any deposit whose `monthly_cap > 0` — 240 base units at 500 bps yields a cap of 1.

**Violated assumption.** `spec/BENEFICIARY_VAULT_B1.md:47-50`: "It binds a vault address to an externally published policy artifact."

**Exploit path.** An adversary reads the published beneficiary, mint, and policy hash, then deposits 240 base units with `cliff_seconds = MIN_CLIFF_SECONDS` and any rate ≤ 500 bps. The vault PDA is now occupied. The intended deposit fails permanently at that address. The attacker's own 240 units are locked for 730 days — a negligible cost.

**Impact.** Permanent DoS of the intended vault address, plus a misleading artifact: `src/beneficiary_vault_verifier.py:158-170` reconstructs exactly this PDA and would report `valid=true` for the attacker's dust vault with an attacker-chosen `depositor`, rate, and cliff.

**Why existing checks/tests miss it.** No test deposits from an unrelated key. `deposit_rejects_a_cliff_shorter_than_the_frozen_minimum` (`tests/b1_litesvm.rs:336-368`) uses a fresh `policy_hash` precisely to avoid the collision, which is the same mechanism from the benign side.

**Repair (minimal).** Add `depositor.key()` to the state seeds, or require `beneficiary` to co-sign. **Architectural.** Bind `policy_hash` to the depositor as in K4V-01.

**Retest.** Setup: beneficiary K, mint M, hash H. Action: stranger S deposits 240 units for `(K, M, H)`; then the intended depositor deposits 1e9. Expected today: the second deposit fails and the verifier accepts the 240-unit vault. Expected after repair: the intended deposit succeeds at its own address.

---

### K4V-03 — MEDIUM — CONFIRMED — Neither program constrains the mint's freeze authority; a live freeze authority permanently bricks every vault

**Evidence.** `open_policy.rs:19` and `purpose-vault/.../deposit.rs:22` accept any `Account<'info, Mint>` with no `freeze_authority`/`mint_authority` constraint; likewise `beneficiary-vault/.../deposit.rs:21`. All releases go through `token::transfer_checked` on `vault_token` as source (`release_beneficiary.rs:93-106`, `release_purpose.rs:128-141`, B1 `release.rs:81-94`), which the SPL Token program rejects when the source account is frozen. There is no pause, close, migrate, or emergency instruction in either program (`purpose-vault/src/lib.rs:20-81`, `beneficiary-vault/src/lib.rs:17-35`).

**Violated assumption.** `spec/PURPOSE_VAULT_B2.md:222-227` frames permanent lockup as reachable only via oracle loss, "the conservative direction of failure". A third party — the mint's freeze authority — can produce the same terminal state at will.

**Exploit path.** Deployment uses a mint that has not revoked `freeze_authority` (the on-chain default for `initialize_mint2` with a freeze authority argument). After deposits land, the freeze authority calls `FreezeAccount` on each `vault_token` PDA. Every subsequent release fails. Only the freeze authority can thaw; if it is hostile or lost, the deposits are unrecoverable.

**Impact.** Total, permanent lockup of all deposits under that mint, for both programs. Preconditions: the mint retains a freeze authority at deployment time — a deployment-configuration property that nothing on chain checks.

**Why existing checks/tests miss it.** The property is asserted only about the *test fixture* (`tests/b2_litesvm.rs:858-861`, `:1078-1079`), never enforced by the program. `docs/THREAT_MODEL.md:22` classifies "Hidden mint or freeze power" as `LOCALNET EVIDENCE`, i.e. an off-chain observation. `src/beneficiary_vault_verifier.py:243-244` detects a vault token that is *already* frozen but never fetches the mint, so it cannot warn beforehand.

**Repair (minimal).** In `open_policy` (B2) and `deposit` (B1): `constraint = mint.freeze_authority.is_none() @ CovenantError::MintHasFreezeAuthority`. Consider `mint.mint_authority.is_none()` as well, since B2's window is denominated in base units of a supply an active minter can dilute. **Architectural.** Additionally have the exporter fetch and report the mint's authority state.

**Retest.** Setup: mint with `freeze_authority = Some(F)`; deposit; advance past the cliff. Action: `FreezeAccount(vault_token)` signed by F, then attempt a release. Expected today: release fails with `AccountFrozen` and no instruction can recover. Expected after repair: `open_policy`/`deposit` rejects the mint before any funds move.

---

### K4V-04 — MEDIUM — CONFIRMED — The shared window has no per-vault reservation; one vault can starve another indefinitely, and the policy authority can dilute without limit

**Evidence.** `programs/purpose-vault/src/instructions/common.rs:33-36` (single `policy.released_this_period`, reset on period advance, no carry-forward) and `:62-70` (first transaction to arrive consumes the window). `deposit.rs:130-133` increments `vault_count` with no maximum. The test suite states the behaviour explicitly: "the two vaults compete for it exactly as they compete for the market term" (`tests/b2_litesvm.rs:2282-2283`).

**Violated assumption.** `spec/PURPOSE_VAULT_B2.md:68-70` presents the three inequalities as jointly protective, and the "What B2 does not establish" list (`:269-296`) does not mention that a co-tenant can consume another vault's entitlement.

**Failure scenario.** Using the exact parameters the repository reports for the live devnet policy — `hard_ceiling = 2,500,000`, beneficiary cap `1,250,000`, purpose cap `2,083,333` (`probes/b2_devnet_verify.mjs:67-69`) — a purpose approver who transacts first each period takes `2,083,333`, leaving `416,667` of the beneficiary's `1,250,000`. If a policy is opened with `hard_ceiling ≤ PURPOSE_CAP`, or if the policy authority attaches further vaults (which it may do unilaterally, supplying its own tokens and naming itself the authority), the beneficiary receives **zero every period, indefinitely**. Unused capacity is never restored (`common.rs:33-40`).

**Impact.** Indefinite liveness denial of a beneficiary's release entitlement. Not theft — the principal remains claimable if the window ever opens — but with no on-chain arbitration there is no bound on the delay. Blast radius: every non-first-mover vault on the policy.

**Why existing checks/tests miss it.** Every joint-capacity test releases beneficiary-first and then measures the remaining headroom (`tests/b2_litesvm.rs:1330-1374`, `:1066-1158`, `:1160-1227`); none reverses the order or iterates across periods to show cumulative starvation.

**Repair (minimal).** Store a `window_share_bps` per vault at `deposit` (validated so the policy's shares sum to ≤ 10 000) and add a fourth inequality in `apply_release`: `vault.released_this_period + amount <= capacity * vault.window_share_bps / 10_000`. **Architectural.** A two-phase claim/settle within each period, or per-period pro-rata allocation proportional to `monthly_cap`.

**Retest.** Setup: policy with `hard_ceiling = PURPOSE_CAP`, one beneficiary vault past its cliff, one purpose vault with pre-recorded approvals for periods P…P+5, fresh oracle report each period. Action: for each period, release the full purpose cap first, then attempt a beneficiary release of 1. Expected today: six consecutive `AggregateCapacityExceeded` and `beneficiary.released_total == 0`. Expected after repair: the beneficiary obtains at least its reserved share every period.

---

### K4V-05 — MEDIUM — CONFIRMED — The oracle key is an immediate, unilateral, unbounded halt switch with no remedy faster than 90 days

**Evidence.** `report_volume.rs:18-26` accepts any `u64`, including `0`, with no bounds, deviation limit, minimum interval, confidence field, or multi-source aggregation. `policy.rs:70-95` makes capacity `0` when volume is `0`, and `common.rs:16,62-70` rejects every positive release against a zero window (test: `zero_eligible_volume_rejects_every_release`, `tests/b2_litesvm.rs:1377-1404`). The only remedies are `propose_oracle` + 90 days (`constants.rs:20`, `rotate_oracle.rs:74-77`) and a silence floor that defaults to `0` and cannot engage before 180 days (`constants.rs:25`, `open_policy.rs:55-60`).

**Violated assumption.** `spec/PURPOSE_VAULT_B2.md:133-134`: "The oracle can only report volume. It cannot move funds, change a cap, change a rate, name a destination, or replace itself." Accurate as far as it goes, but it omits that the oracle can stop every release on the policy at any moment. Line `:199` partially concedes this ("can only ever slow a release down").

**Failure scenario.** A compromised or coerced oracle key reports `0` (or simply stops). Every vault on the policy is frozen. The policy authority's fastest repair is `propose_oracle` → 90 days → `execute_oracle_rotation` → a fresh report. If the policy authority is also unavailable, and no silence floor was declared (the default and the state of the reported devnet policy — `probes/b2_devnet_verify.mjs:70`), the halt is permanent.

**Impact.** Total liveness DoS of all vaults on a policy from one key, ≥ 90 days minimum recovery. Blast radius: every depositor and beneficiary on the policy.

**Why existing checks/tests miss it.** The suite tests the *fail-closed* property as a feature (`a_policy_whose_oracle_never_reported_releases_nothing`, `without_a_declared_floor_a_silent_oracle_locks_both_vaults`) but never frames it as an adversarial capability with a bounded recovery time.

**Repair (minimal).** Reduce the rotation notice when the oracle has been silent past a threshold, or allow a policy to declare a floor that engages on *hostile* reports (a zero report) and not only on silence. **Architectural.** Multiple reporters with a median and a staleness quorum, so no single key can zero the window.

**Retest.** Setup: funded policy, two vaults, fresh reports. Action: oracle reports `0`; attempt both release paths each period for 90 days of simulated time. Expected: `AggregateCapacityExceeded` throughout; the earliest possible new-oracle report is `pending_since + 7_776_000`.

---

### K4V-06 — MEDIUM — CONFIRMED — `policy.authority` is immutable; compromise or loss is unrecoverable for the policy's lifetime

**Evidence.** `state.rs:28` (`authority`), written once at `open_policy.rs:73`, read only at `rotate_oracle.rs:12`. No instruction in `lib.rs:20-81` writes it.

**Failure scenario (compromise).** The attacker calls `propose_oracle(attacker)`, waits 90 days (`rotate_oracle.rs:74-77`), executes permissionlessly (`ExecuteOracleRotation` has no signer at all — `rotate_oracle.rs:54-62`), then reports an inflated volume. With the default `hard_ceiling = u64::MAX` — the value in every fixture (`tests/b2_litesvm.rs:60,265-274`) — the aggregate rule ceases to bind and only the per-vault caps remain. They may also attach additional vaults via `deposit` co-signature (see K4V-04). They cannot redirect existing vault funds: releases require the vault authority's signature and PDA-derived seeds.

**Failure scenario (loss).** With the authority lost, oracle rotation becomes impossible. If the oracle is subsequently lost and `silence_floor == 0`, all deposits are locked permanently — stated as deliberate at `spec/PURPOSE_VAULT_B2.md:222-227`, but the *absence of any way to replace the authority itself* is not stated.

**Impact.** Loss of B2's distinguishing aggregate guarantee under compromise; permanent lockup under compound loss. Bounded — not theft — which is why this is MEDIUM and not higher; the bound is asserted by `an_inflated_oracle_report_cannot_lift_a_release_past_the_frozen_schedule` (`tests/b2_litesvm.rs:1769-1823`).

**Repair (minimal).** A `propose_authority` / `execute_authority_transfer` pair with the same 90-day notice pattern already implemented for the oracle. **Architectural.** Require the authority to be a threshold account at `open_policy`, or require a non-inert `hard_ceiling` so the aggregate rule survives authority capture.

**Retest.** Setup: policy with `hard_ceiling = u64::MAX`. Action: from the authority, propose itself as oracle, advance 90 days, execute, report `u64::MAX`, then release both vaults at full cap. Expected: both succeed and `policy.released_this_period == BENEFICIARY_CAP + PURPOSE_CAP > MARKET_CAPACITY`; assert no instruction can change `policy.authority`.

---

### K4V-07 — MEDIUM — CONFIRMED (operational/deployment) — Both programs are deployed upgradeable under a retained single key, and B2's specification omits the caveat B1's states

**Evidence.** `README.md:247-249`: "No mainnet or production deployment exists… The devnet upgrade authority is a retained test key." `spec/BENEFICIARY_VAULT_B1.md:81-88` correctly states the immutability boundary; `docs/ROADMAP.md:136-150` reserves the word "immutable". **But** `spec/PURPOSE_VAULT_B2.md:213-221` asserts without qualification: "B2 has no update, configure, close, migrate, emergency-release, alternate destination, or administrative transfer instruction… There is no way to change the approver, the beneficiary, a rate, a cliff, a ceiling, a floor, or a cap after creation." `docs/THREAT_MODEL.md:14` lists "Upgrade weakens covenant" as `OPEN`.

**Violated assumption.** Instruction-surface minimality is being presented as an invariant. It is a property of the *current bytes only*, and the loader authority can replace those bytes in one transaction.

**Impact.** With an upgrade authority present, every invariant in this report — caps, cliff, notice, recusal, aggregate window — is revocable by one key, with total blast radius over all deposits in both programs.

**Repair.** Add an "Immutability boundary" section to `spec/PURPOSE_VAULT_B2.md` mirroring B1's; for any deployment holding value, deploy non-upgradeable or verifiably revoke the authority and publish the loader state, as `docs/ROADMAP.md:145-150` already requires.

**Retest.** Read `ProgramData` for both program IDs and assert `upgrade_authority_address == None` before any value is deposited; add a CI/receipt check that fails when it is `Some`.

---

### K4V-08 — LOW — CONFIRMED — An approval for the immediately following period is unusable, and unreclaimable, unless it was created early enough in the current period

**Evidence.** `approve.rs:60-65` requires `period_index > current`. `release_purpose.rs:76-89` requires **both** `approval.period_index == period` **and** `now - approval.created_at >= MIN_NOTICE_SECONDS`. `constants.rs:6,10`: `PERIOD_SECONDS == MIN_NOTICE_SECONDS == 2_592_000`.

**Derivation.** Approving at offset `r` into period `C` (`0 ≤ r < 2_592_000`) for `P = C+1` gives a usable window of exactly `[created_at + 2_592_000, genesis + (C+2)·2_592_000)`, whose length is `2_592_000 − r`. As `r → 2_592_000` the window is empty and the approval can never be consumed. There is no close instruction, so its rent is stranded permanently.

**Evidence in tests.** `tests/b2_litesvm.rs:1518-1539` constructs exactly this state (`period_start(1) - 100`, approving for period 1) and asserts `NoticePeriodActive` — but treats it as a notice-boundary check rather than a permanently dead approval.

**Impact.** Operational footgun for a treasury operating on a monthly cadence: a late-in-the-month approval silently produces a dead account and a missed release window. No fund loss beyond rent.

**Repair (minimal).** `require!(period_index >= current + 2, CovenantError::ApprovalPeriodTooSoon)` in `approve_handler`, making the full period always available; document the rule in `spec/PURPOSE_VAULT_B2.md:237-244`. **Architectural.** Allow consumption in the approved period *or later*, up to a declared expiry, and add `close_approval` to return rent after expiry.

**Retest.** Setup: purpose vault, policy genesis `G`. Action: at `G + 2_592_000 − 1`, approve for period 1; advance to `created_at + 2_592_000` and attempt a release. Expected today: `ApprovalPeriodMismatch`, and no timestamp exists at which the approval is consumable. Expected after repair: `approve` rejects with `ApprovalPeriodTooSoon`.

---

### K4V-09 — LOW — CONFIRMED — No state versioning or realloc path; a future upgrade that extends any account layout bricks all existing accounts

**Evidence.** `state.rs:25-113` and `beneficiary-vault/src/state.rs:3-21` contain no version field or reserved padding. Accounts are sized exactly: `open_policy.rs:22` and `:31`, `purpose-vault/.../deposit.rs:40`, `approve.rs:38`, `beneficiary-vault/.../deposit.rs:31`. A repository-wide search finds no `realloc`, no `close`, and no `AccountLoader`.

**Failure scenario.** `MarketInput` already grew once (`pending_oracle`, `pending_since` at `state.rs:69-74`). If a future upgrade adds another field, `Account<MarketInput>::try_deserialize` fails on the existing 147-byte accounts, every instruction touching the market fails, and — with no migrate or close instruction — the deposits are permanently locked. The programs are upgradeable (K4V-07), so this is one routine mistake away.

**Impact.** Potential total, permanent lockup. Requires an operator error, not an attacker.

**Repair (minimal).** Add `version: u8` plus reserved padding to each account now, while the deployed set is small. **Architectural.** Add an authority-gated migrate instruction with `realloc`, and a CI check that `INIT_SPACE` never shrinks or grows for an already-deployed discriminator.

**Retest.** Add a compile-time pin — `const _: () = assert!(MarketInput::INIT_SPACE == 139);` for each account — so any layout change fails the build rather than the cluster.

---

### K4V-10 — LOW — CONFIRMED — `authority` / `beneficiary` are unchecked, so a vault can be created that nobody can ever release

**Evidence.** `purpose-vault/.../deposit.rs:21` and `beneficiary-vault/.../deposit.rs:20` are `UncheckedAccount` with no non-default and no system-owned check. The release paths require that exact key to sign (`release_beneficiary.rs:11`, `release_purpose.rs:13`, B1 `release.rs:11`), which is impossible for `Pubkey::default()`, a program id, or a PDA of a program with no signing path.

**Impact.** Permanent lockup of the full deposit. Self-inflicted by the depositor; no adversarial path (the depositor chooses the key), which is why this is LOW.

**Repair.** `require!(authority.key() != Pubkey::default())` plus `constraint = authority.owner == &System::id()`, or require the authority to co-sign the deposit.

**Retest.** Deposit with `authority = Pubkey::default()`. Expected today: success, followed by an unreleasable vault. Expected after repair: rejection at deposit.

---

### K4V-11 — LOW — CONFIRMED — Rent is never recoverable; `Approval` accounts accumulate for the life of the schedule

**Evidence.** No close instruction in either program (`purpose-vault/src/lib.rs:20-81`, `beneficiary-vault/src/lib.rs:17-35`). One `Approval` PDA per `(vault, period)` (`approve.rs:35-41`), paid by the approver, never closed even after the period passes or the approval is dead (K4V-08). Over a 20-year monthly schedule that is 240 permanently-stranded accounts per purpose vault, plus the vault state, token vault, policy, and market accounts.

**Repair.** `close_approval` returning rent to `approval.approver`, permitted only once `period_index < current_period_index`. Note the deliberate trade-off: closing approvals removes the on-chain audit record the design leans on, so an alternative is to accept the cost and document it.

---

### K4V-12 — LOW — CONFIRMED — The "exactly eight instructions" surface test parses its own source text and cannot see the deployed program

**Evidence.** `programs/purpose-vault/src/lib.rs:84-118`. It reads `include_str!("lib.rs")`, splits on the literal `"pub mod purpose_vault {"`, and collects lines whose trimmed prefix is `"pub fn "`. Its own docstring (`:87-92`) claims this is what makes the specification's no-update-surface claim worth something.

**Why it does not hold.** It inspects the same file in the same crate at test time. It cannot observe the generated IDL, the compiled artifact, or an upgrade (K4V-07). It is also fragile to formatting: a signature whose `pub fn` is not the first token of a trimmed line, or an instruction introduced through a macro or an included module, would not be collected.

**Repair.** Assert over the generated instruction discriminator set (`purpose_vault::instruction::*`) or the IDL, and pin the deployed artifact SHA-256 in CI.

---

### K4V-13 — LOW — CONFIRMED — Frozen time constants have no literal regression pin; `cargo test --workspace` passes if any of them changes

**Evidence.** Every Rust test consumes the constants symbolically: `tests/b2_litesvm.rs:503,616`, `programs/purpose-vault/src/policy.rs:283-287`, `tests/b1_litesvm.rs:211,354`. `tests/test_purpose_vault_b2_parity.py:210-213` asserts `PERIOD_SECONDS == 2_592_000` and `MIN_CLIFF_SECONDS == 63_072_000` against constants it defines itself at `:38-40` — it never imports the Rust values, so it cannot observe drift. Only `MIN_SILENCE_GRACE_SECONDS` and `ORACLE_ROTATION_NOTICE_SECONDS` are genuinely pinned (`tests/b2_litesvm.rs:79-80`).

**Consequence.** Changing `MIN_CLIFF_SECONDS`, `PERIOD_SECONDS`, `MIN_NOTICE_SECONDS`, or `MAX_INPUT_AGE_CEILING_SECONDS` in `constants.rs` passes the entire test suite — while `README.md:250-254` calls the 730-day cliff "a locked invariant".

**Repair.** Add `const _: () = assert!(MIN_CLIFF_SECONDS == 63_072_000);` and equivalents alongside the two existing pins; emit the constants to a generated file the Python parity test imports.

---

### K4V-14 — LOW — CONFIRMED — `probes/b2_devnet_verify.mjs` does not authenticate account identity, weakening the README claim it supports

**Evidence.** `probes/b2_devnet_verify.mjs:29` fetches `getAccountInfo(...).data` with no `owner` check; `:20-27` decodes from byte 8 with no Anchor discriminator check; the file contains no PDA derivation at all — every address comes from `devnet_result.json`, written by the probe itself (`:16`). `:63-83` prints `out.checks` and always exits 0, even when checks are `false`.

**Claim affected.** `README.md:122-127`: "Every number was then read back by decoding the accounts off the cluster rather than trusting the script."

**Contrast within the repository.** `probes/r3_full_scale_squads_verify.mjs:42-51` checks owner and discriminator, and `:147-161` re-derives every PDA from `policy_hash` — the correct pattern is already present.

**Repair.** Port the `b2Account`/`findProgramAddressSync` pattern from the R3 verifier into `b2_devnet_verify.mjs`, and set `process.exitCode = 1` when any check is false.

---

### K4V-15 — LOW — CONFIRMED — The R3-B2 probe installs the program with a Surfpool cheat code and hashes the local file, not the installed bytes

**Evidence.** `programs/purpose-vault/examples/r3_full_scale_rpc_probe.rs:322-332` calls `surfnet_writeProgram` and then only checks `loaded.executable`. `sbf_sha256` is computed from the local file at `:322-323` and recorded at `:778-780`; the receipt honestly labels the install `"surfnet_writeProgram_local_only"`.

**Consequence.** The R3-B2 receipt does not establish that the runtime executed the bytes whose hash it publishes, and the B2 path never exercises a real upgradeable-loader deploy — unlike B1, which has a 252-write loader probe (`README.md:33-36`). `spec/FULL_SCALE_ONE_MINT_R3.md:167` calls for binding the exact SBF hash into the receipt.

**Repair.** Read the installed program/ProgramData bytes back over RPC and compare to `sbf_sha256`, or reuse B1's real-loader path for B2.

---

## 3. Informational

**K4V-16 — Deployment status is stated three mutually contradictory ways at this commit.** `README.md:50-59` and `:100-129` describe B1 and B2 as deployed and driven on public devnet; `README.md:260-263` states "B2 exists only as a local build. It has never been sent to any cluster, has no published IDL"; `spec/PURPOSE_VAULT_B2.md:3` says "NOT DEPLOYED" and `:296` "No deployment, no audit, no mainnet"; `spec/BENEFICIARY_VAULT_B1.md:90-91` says the program ID "is not a mainnet or devnet deployment address" while `programs/beneficiary-vault/src/lib.rs:11` declares exactly the devnet address the README cites. A reader cannot determine the live state from the repository alone.

**K4V-17 — `hard_ceiling` is per-period, not lifetime.** `common.rs:33-36` resets the shared counter each period, so the ceiling bounds `N × hard_ceiling` over N periods. `state.rs:34-37` and `spec/PURPOSE_VAULT_B2.md:64` are precise; `README.md:96-99` ("freeze an absolute ceiling on the window") reads as a lifetime bound.

**K4V-18 — Model/program divergence and evidence-tooling details.** `src/purpose_bound_vault.py:83-84` accepts `market_capacity_bps == 0` where `open_policy.rs:61-64` requires `1..=500`. `src/beneficiary_vault_rpc_exporter.py:127` reads at `confirmed`, not `finalized`, for accounts used as durable evidence. `SHA256SUMS` is unsigned and in-repo, so `sha256sum -c` (`.github/workflows/ci.yml:30-31`) detects drift, not adversarial modification.

**K4V-19 — Boundary-value test gaps.** `spec/FULL_SCALE_ONE_MINT_R3.md:175` requires R3-N03 "one second before cliff"; the actual tests use timestamps ~21 months early (`tests/b2_litesvm.rs:1584-1595`) or the deposit-time clock (`tests/b1_litesvm.rs:248-255`). `MIN_NOTICE_SECONDS - 1` is never tested (only 100 s). B1 has no `ZeroAmount` and no `DepositExceeded` test. Neither suite passes an adversarial substitute `policy`, `market`, or `vault_token` account to a release path — the Anchor constraints appear sound (see §4), but that is unverified by execution.

**K4V-20 — Unsolicited transfers into a `vault_token` PDA are permanently locked, not merely non-releasable.** `common.rs:52-60` caps `released_total` at `deposited_amount` and there is no sweep instruction. `README.md:44-46`, `docs/THREAT_MODEL.md:16`, and `src/beneficiary_vault_verifier.py:234-238` treat this as intended "safe surplus", which is correct about entitlement but understates that the surplus is unrecoverable by anyone.

**K4V-21 — Batch pre-approval satisfies the notice rule.** `approve.rs:60-65` permits an approval for any future `period_index`, so an approver can pre-authorize an unlimited number of future periods in one session. Each release still carries ≥ 30 days of dated public notice, so `spec/PURPOSE_VAULT_B2.md:237-244` holds as written; noted because it means contemporaneous per-period review is not enforced.

---

## 4. Candidates investigated and rejected

| Candidate | Rejecting evidence |
|---|---|
| Missing account binding on the release paths (confused deputy) | `release_beneficiary.rs:13-51` / `release_purpose.rs:15-57` bind vault↔mint (`has_one = mint`), vault↔signer (the signer key is a seed component), vault↔policy and vault↔market (seeds over `vault.policy_hash`), vault↔token vault (`seeds = [TOKEN_VAULT_SEED, vault.key()]` + `token::authority = vault`), approval↔vault/approver (`has_one`). No substituted account can satisfy address derivation. |
| Non-canonical bump / signer-seed forgery | Seeds use `vault.state_bump` / `policy.bump` / `market.bump` read from program-owned, discriminator-checked accounts created with Anchor's canonical bump (`deposit.rs:48,125-126`; `open_policy.rs:26,34,83,95`). No account of these types can exist at a non-canonical address. |
| PDA namespace collision across seed prefixes | `constants.rs:28-32` prefixes differ, and the concatenated seed lengths differ except for policy vs market, whose 14-byte prefixes differ. `pda_vector_is_frozen` (`policy.rs:320-356`) asserts pairwise distinctness. |
| Token-2022 / transfer-fee / transfer-hook confusion | Both programs pin `Program<'info, Token>` and `anchor_spl::token::{Mint, TokenAccount}` (classic `Tokenkeg…` owner) at `deposit.rs:8,60`, `release_beneficiary.rs:7,51`, `release_purpose.rs:9,57`, B1 `deposit.rs:6,45` / `release.rs:6,39`. A Token-2022 mint cannot enter. |
| `transfer` vs `transfer_checked` / decimals confusion | All five transfer sites use `transfer_checked` with `mint.decimals` (deposit) or the frozen `vault.mint_decimals` bound to the same mint by `has_one` (`deposit.rs:95-107`, `release_beneficiary.rs:93-106`, `release_purpose.rs:128-141`, B1 `deposit.rs:74-86`, `release.rs:81-94`). SPL mint decimals are immutable. |
| Integer overflow / underflow in cap and capacity math | `policy.rs:7-24` widens to `u128` before multiplying, uses `checked_mul` and checked narrowing; `common.rs:42-70` uses `checked_add` on every counter; `deposit.rs:91-93,130-133` uses `checked_add`; workspace sets `overflow-checks = true` (`Cargo.toml:7`). `the_widest_admissible_report_does_not_overflow` (`policy.rs:174-178`) covers `u64::MAX`. |
| `updated_at == 0` sentinel collision (oracle never spoke vs. reported at epoch) | `report_count` is the sentinel (`policy.rs:73`, `state.rs:63-65`), covered by `a_floor_cannot_resurrect_a_policy_whose_oracle_never_spoke` (`policy.rs:248-265`). |
| Future-dated oracle report treated as maximally fresh | `assert_fresh` requires `age ∈ [0, max_age]` (`policy.rs:100-109`); the stale branch additionally requires `age >= grace >= 180 days` (`policy.rs:86-93`), so a negative age fails both branches. |
| Oracle rotation used as a release path | `execute_oracle_rotation_handler` leaves `eligible_volume`, `updated_at`, and `report_count` untouched (`rotate_oracle.rs:79-85`); covered by `a_release_resumes_only_after_the_replacement_oracle_has_spoken` (`tests/b2_litesvm.rs:2146-2206`). |
| Rotation replay / stale-proposal execution | `pending_oracle` is zeroed and `pending_since` reset on execution (`rotate_oracle.rs:81-82`); a second proposal restarts the clock (`:34-35`); covered by `tests/b2_litesvm.rs:2041-2090`. |
| Approval replay / double-claim | One `Approval` PDA per `(vault, period_index)` created with `init` (`approve.rs:35-41`); `consumed` is monotone (`release_purpose.rs:91-100,143`); consumption is confined to the approved period (`:78-81`). |
| Approver redirecting a release to itself after the notice | The destination is pinned in the approval and `ApproverIsPayee` is re-checked at release (`release_purpose.rs:65-74`), which covers closing and reopening the destination account under a new owner. |
| Self-transfer to the vault's own token account as a theft path | It moves nothing yet still debits `released_total` and `released_this_period` (`common.rs:52-74`), so it costs the approver its own entitlement. It is only a self-funded way to burn shared-window capacity — already captured by K4V-04. |
| B1 cliff bypass (no explicit `now >= cliff_end_ts` check in `release_handler`) | `period_index(now, cliff_end_ts)` returns `CliffActive` for negative elapsed (`beneficiary-vault/src/policy.rs:14-20`); exercised at `tests/b1_litesvm.rs:248-255`. |
| Hand-rolled Ed25519 point test in the Python PDA finder diverging from `curve25519-dalek` | `src/beneficiary_vault_verifier.py:80-101` differs from dalek only for 32-byte values whose low 255 bits are ≥ 2²⁵⁵−19, or which encode ±1 with the sign bit set — roughly 2⁻²⁵² of the SHA-256 output space. The pinned vector at `beneficiary-vault/src/policy.rs:60-68` and the tests at `tests/test_beneficiary_vault_verifier.py:70-88` confirm agreement on real derivations. |
| Vault kind confusion (a purpose vault released via the beneficiary path) | `kind.seed_byte()` is a seed component (`state.rs:15-20`, `deposit.rs:44`) and both handlers assert the kind explicitly (`release_beneficiary.rs:55-58`, `release_purpose.rs:61-64`). |
| Vault attached to a mint other than the policy's | `deposit.rs:32` places `has_one = mint` on the policy, so every vault on a policy shares one mint and the base-unit aggregate is well defined. |

---

## 5. Unresolved uncertainty

**K4V-U1 — LOW — UNCERTAIN — `CpiContext::new` is passed a `Pubkey`, not an `AccountInfo`.** Five sites: `beneficiary-vault/.../deposit.rs:75-76` and `release.rs:82-83`; `purpose-vault/.../deposit.rs:96-97`, `release_beneficiary.rs:94-95`, `release_purpose.rs:129-130` — all pass `ctx.accounts.token_program.key()`. Under `anchor-lang = "=1.1.2"` (`Cargo.lock:276-278`) I cannot compile or read the dependency source in this checkout. Two outcomes: either the 1.x API takes a program id, in which case there is **no security consequence** — `Program<'info, Token>` already pins the invoked program to `Tokenkeg…`; or the workspace does not compile, which would invalidate every executable claim in the repository. The latter is unlikely, since CI runs `cargo clippy -- -D warnings` and `cargo test --locked` on every push (`.github/workflows/ci.yml:46-50,72`). **Evidence to resolve:** `cargo build -p purpose-vault -p beneficiary-vault`, or the `anchor-lang 1.1.2` `CpiContext::new` signature.

**K4V-U2 — INFORMATIONAL — UNCERTAIN — No deployed-artifact or evidence claim is verifiable here.** No network, no `target/deploy/*.so`, no cluster access. Every hash, signature, slot, address, and balance in `evidence/*.json`, `README.md:50-129`, and `SHA256SUMS` is taken as unverified assertion. In particular, the claimed byte-for-byte reproducibility of `d6a38fe4…` / `081b6c16…` and the devnet account states are outside this review.

**K4V-U3 — INFORMATIONAL — CONFIRMED — The B2 devnet probes are only syntax-checked.** `package.json:8` runs `node --check` only, invoked at `.github/workflows/ci.yml:28-29`. `probes/b2_devnet_probe.mjs` and `probes/b2_devnet_verify.mjs` are never executed in CI, so K4V-14 would not surface there.

---

## 6. Severity counts

### Confirmed

| Severity | Count | IDs |
|---|---:|---|
| CRITICAL | 0 | — |
| HIGH | 0 | — |
| MEDIUM | 7 | K4V-01, 02, 03, 04, 05, 06, 07 |
| LOW | 8 | K4V-08, 09, 10, 11, 12, 13, 14, 15 |
| INFORMATIONAL | 7 | K4V-16, 17, 18, 19, 20, 21, U3 |
| **Total** | **22** | |

### Non-confirmed

| Severity | Status | Count | IDs |
|---|---|---:|---|
| LOW | UNCERTAIN | 1 | K4V-U1 |
| INFORMATIONAL | UNCERTAIN | 1 | K4V-U2 |
| — | FALSE_POSITIVE_RISK | 0 | — |
| — | rejected as false positives | 16 | §4 |

---

## 7. Three highest-leverage retests

1. **Namespace squatting, both programs (K4V-01 + K4V-02).** From an unrelated key, call `open_policy(H)` for a hash the operator intends to use, and deposit 240 base units into the B1 PDA for the operator's `(beneficiary, mint, H)`. Assert the intended operator can no longer create either account, and that `src/beneficiary_vault_verifier.py` reports `valid=true` for the attacker's dust vault. This is the cheapest attack in the report and its effect is permanent.

2. **Frozen-mint lockup (K4V-03).** Deposit under a mint with a live `freeze_authority`, advance past the cliff, `FreezeAccount(vault_token)`, then attempt a release from both programs. Assert `AccountFrozen` and enumerate the instruction surface to show no recovery path exists. This converts a documented deployment assumption into a demonstrated on-chain terminal state.

3. **Shared-window starvation over consecutive periods (K4V-04).** With the reported devnet parameters (`hard_ceiling = 2_500_000`, caps `1_250_000` / `2_083_333`), pre-record purpose approvals for periods P…P+5, refresh the oracle each period, and release the purpose cap first each period. Assert `beneficiary.released_total == 0` after six periods. This tests B2's central claim — the aggregate window — from the direction the existing suite never takes.

---

## 8. Mainnet-readiness verdict

**`NOT_READY`**

The two programs are carefully constructed at the account-validation layer: PDA binding, signer derivation, arithmetic widening, `transfer_checked`, discriminator and ownership checks are correct throughout, and sixteen plausible attack candidates were rejected on code evidence (§4). No confirmed path permits theft, double-spend, or release above the frozen schedule. That is a real result and should be read as such.

It is nonetheless not ready, for reasons that are code-level and not merely procedural. Necessary conditions before any mainnet deployment:

1. **Close the two permissionless squatting paths** (K4V-01, K4V-02) by binding the policy and vault namespaces to their creator. Both are exploitable today by anyone, for the price of rent, with permanent effect.
2. **Enforce the mint's authority state on chain** (K4V-03). A live freeze authority converts a design whose worst documented failure is "locked forever if the oracle is lost" into "locked forever at a third party's discretion".
3. **Give the shared window a per-vault reservation** (K4V-04), or state explicitly in the specification that a co-tenant may consume another vault's entitlement indefinitely.
4. **Resolve the single-key powers**: an authority-transfer path with the existing 90-day notice pattern (K4V-06), and a remedy for a hostile or silent oracle faster than 90 days (K4V-05). Both are currently unrecoverable single points of failure over a twenty-year schedule.
5. **Deploy non-upgradeable, or verifiably revoke the upgrade authority and publish the loader state** (K4V-07). Until then every invariant above is advisory, and `spec/PURPOSE_VAULT_B2.md:213-221` should carry the caveat `spec/BENEFICIARY_VAULT_B1.md:81-88` already states.
6. **Add account versioning or reserved padding** before the deployed account set grows (K4V-09).
7. **Reconcile the contradictory deployment-status statements** (K4V-16), and pin the frozen time constants with literal assertions (K4V-13) so the "locked invariant" language is backed by a failing test.

The repository's own position — no independent audit (`README.md:247`), no production parameters, no real governance, no mainnet authorization — is consistent with this verdict and is stated more carefully than most projects at this stage.

---

## 9. Limitations

**Not evaluated.** `probes/r3_full_scale_squads_probe.mjs` (602 lines), `probes/squads_authority_probe.mjs`, `probes/solana_fixed_supply_probe.cjs`, `probes/b2_devnet_probe.mjs` beyond a key-handling and endpoint scan; the three B1 example probes (`b1_real_loader_probe.rs`, `b1_public_devnet_probe.rs`, `b1_rpc_transaction_probe.rs`, ~1,860 lines); `src/covenant_cli.py`; `src/purpose_bound_vault.py` beyond its cap/capacity arithmetic; `tools/verify_declare_id_delta.py`; `tests/test_purpose_bound_vault.py`, `test_beneficiary_vault_idl.py`, `test_beneficiary_vault_rpc_exporter.py`, `test_beneficiary_vault_rpc_surfpool.py`; `package-lock.json` and the JavaScript dependency tree; `docs/COMMERCIAL_DISCLOSURE.md`, `CONTRIBUTING.md`, `TRADEMARKS.md`, `CITATION.cff`; the full contents of `evidence/*.json` and `spec/PURPOSE_BOUND_VAULT_COVENANT.md` (read only where a specific claim was checked).

**Not executed.** Nothing was built, run, deployed, or queried. No `cargo build`, no `cargo test`, no `cargo clippy`, no LiteSVM run, no Surfpool, no RPC. All statements about test coverage are derived from reading test source, not from observing outcomes. `target/deploy/*.so` is absent from this checkout, so the artifacts the LiteSVM suites load do not exist here.

**Dependency assumptions.** `anchor-lang`/`anchor-spl` `=1.1.2`, `litesvm 0.10.0`, `solana-*` 3.x, `spl-token-interface 2.0.0` were read from `Cargo.toml` and `Cargo.lock` but not fetched. Anchor's `#[derive(Accounts)]` constraint semantics (`init`, `seeds`/`bump`, `has_one`, `token::mint`, `token::authority`), discriminator checks, and owner checks are assumed to behave as documented for 1.1.2; §4's rejections depend on that. K4V-U1 is the one place where a divergence would be material.

**Deployment assumptions.** The devnet deployment state, upgrade-authority state, mint authority state, and every hash and signature in `evidence/` and `README.md` are unverified assertions. K4V-03, K4V-04, and K4V-07 are calibrated using parameters the repository *reports* for the live policy (`probes/b2_devnet_verify.mjs:67-70`); if the live values differ, the concrete numbers change but the mechanisms do not.

**Prose treated as evidence, not instruction.** Specifications, comments, README claims, and evidence files were used only as statements to check against code. Where they conflict with code, the code governs; where they conflict with each other, both are reported (K4V-16).

