# E-10: signed submission windows implemented in isolated v6 SBF

Status: LOCAL IMPLEMENTATION AND REPRODUCTION PASS. The original v5 exact-clock
counterexample remains frozen. This candidate has a separate program identity,
account namespace and Cargo workspace/lockfile under `candidates/launch-vault-v6`.
The default build refuses `open_policy`; only the explicit TEST_ONLY build can
admit policies. No public deployment or production rights are established.

## What changed and why

A proposal signed at one second can reach execution at another second. V6 accepts
a signed inclusive interval `[valid_from, valid_until]` of at most 300 seconds.
Actual `Clock.unix_timestamp` at admission sets `created_at`; the entire 90-day
notice and subsequent 30-day execution interval start from that actual time.
Admission at the last allowed second gets the full notice. A relayer cannot
shorten it by withholding the message. All three durations are TEST_ONLY.

The signed instruction carries role, recovery mode, nonce, epoch, both bounds
and an explicit predecessor public key. Account metas bind successor and policy;
the program identity binds the fixed constants. The on-chain predecessor must
equal the current role key for the signed epoch. Changing any of these fields,
the policy or the program requires fresh signatures from every required signer.

Negative, reversed, oversize and not-yet-open/expired windows are rejected. The
whole permitted interval is checked for i64 overflow, including notice and
execution after its latest endpoint. Future scheduled intervals and zero-width
intervals are allowed; the latter retain exact-clock delivery fragility.

Current-key/2-of-3 guardian authority, successor consent, role pause, cancellation,
expiry, consumed nonces and known-key recipient exclusions retain v5 semantics.
There is no pre-admission revocation operation. Fee payers cannot refresh only
their own signature to extend the interval or replace a blockhash.

## ABI and immutable identity

Test program: `FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU`.
Its known local fixture seed is `[88; 32]`; it is unsuitable for public use.
The identity domain is `k4v-launch-policy-v6-test-profile-1`. It binds the maximum
submission interval in addition to all v5 actors, committees, configuration and
notice/execution constants. PDAs use `launch-v6-*`; all six account types are V6.

`propose_withdrawal` instruction bytes, including its 8-byte discriminator:

| Offset | Field | Bytes |
|---|---|---|
| 8 | role | 1 |
| 9 | recovery mode | 1 |
| 10 | nonce | 8 |
| 18 | epoch | 8 |
| 26 | valid_from | 8 |
| 34 | valid_until | 8 |
| 42 | predecessor | 32 |

The proposal grows from 156 to **172 bytes**. The existing policy/role/nonce/
epoch/predecessor/successor/mode fields keep their offsets; bounds occupy 122 and
130, actual creation 138, notice 146, expiry 154, status 162, finish 163, bump 171.
Policy/config/vault/approval/key-index sizes stay 1065/492/138/177/76 bytes.
Compiler-generated IDL, compiled Borsh sizes and independent Python/JavaScript
identity encodings are checked. Old discriminators/layouts cannot enter v6 export.

## Evidence and its scope

**Signed SBF probe:** five new integration tests send 142 target transactions:
44 accepted and 98 expected refusals. Both roles and both modes cover 0/1/30/300
second delay, deadline +1, early/negative/reversed/oversize windows, maximum-safe
time, overflow, every intent field, missing signatures, partial re-signing,
wrong predecessor, competing nonce, cancellation replay and expired blockhash.
Program/mint injection, SOL airdrops and Clock control are explicit probe fixtures.
These five tests do not transfer funding tokens.

**Financial rehearsal:** signed native-loader deployment uploads and seals the
actual test SBF. SPL instructions create/fund the mint and accounts, revoke mint
authority, and make the funding/release transfers. No program, mint or token
account injection is used in this rehearsal. It submits 641 transactions:
630 successful, 11 expected refusals, excluding SOL airdrops.

The sequence is partial Founder/Treasury releases at period 6, two recovery
messages signed before their respective 30-second delays, complete notice,
role recovery, further releases at period 9, normal cancellation, explicit expiry
and continued releases at period 13 in the next annual input epoch. The second
recovery's full notice matures 60 seconds after the old undelayed schedule.
The principal, T0, used budgets, original Treasury approval authors and notices,
permanent key indexes and supply conservation are checked. Final released totals
are 300,000 Founder and 450,000 Treasury tokens, with 100,000/150,000 used in year
two; all amounts are fixture values, not a funding forecast.

There are **63 Rust checks** (2 library, 3 ABI, 53 lifecycle/recovery including
the native-loader rehearsal, 5 submission tests), 4 JavaScript identity checks
and **47 Python raw-account/RPC checks**. Eleven raw checkpoints independently
reconstruct both signed bounds and actual start, financial state, immutable
loader bytes and complete proposal/approval/key history. Loopback HTTP exports
all 11 checkpoints with 55 read-only requests and one final bank response per
export. Supply remains exactly 1,000,000,000 tokens at 9 decimals.

The Python verifier validates supplied bytes and does not independently verify
transaction signatures or chain provenance. Local Clock jumps intentionally
keep a valid blockhash for timing probes; explicit blockhash expiry is a separate
test. This establishes no real blockhash lifetime or public-network delivery
guarantee. The HTTP exercise replays local runtime bytes, not validator RPC.

## Reproduce and review

```bash
bash tools/run_e10_local_reproduction.sh
```

The command checks preserved E-07/E-08/E-09 inputs, rebuilds both v6 profiles with
Solana CLI 3.1.10 / SBF tools v1.52, rejects stack diagnostics, verifies exact hashes,
runs the signed tests and independently decodes fresh rehearsal bytes. Native
and SBF caches are separate. `K4V_E10_SKIP_BUILD=1` reuses only hash-verified SBF.

Review inputs: `spec/LAUNCH_V6_TEST_ONLY_CANDIDATE_v1.json`, compiler-generated
`idl/launch_vault_v6.json`, `examples/e10_rehearsal_bundle.json`,
`evidence/E10_LOCAL_VALIDATION_2026-09-10.json` and `evidence/e10/`.
The capacity and generic governance kernels are unchanged after namespace
normalization. Old root `Cargo.lock`, v5 sources, build bytes and review inputs
remain frozen; the v6 lockfile is independent.

## Next engineering step

E-11 will connect a v6 transaction client to a local validator: inspect canonical
state, construct the exact signed intent, handle blockhash/interval expiry by
requesting new signatures, submit/confirm and export actual validator RPC state.
It should repeat the delayed recovery/continued-release path and refusal cases
without replacing signed bytes or treating successful simulation as confirmation.
Local validator observation, public-network observation, human review and final
production authorities/parameters are distinct unfinished items. Real demand
feedback remains paused by the Founder and unverified; engineering continues.
