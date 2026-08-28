# R3 full-scale one-mint acceptance specification

> Status: `PASS — LOCAL FULL-SCALE INTEGRATION / R4 INDEPENDENT REPRODUCTION OPEN`
>
> No mainnet transaction, production key, official K4V mint or launch is
> authorized by this specification.

## Target

Close the largest remaining integration and unit bridge with one test mint:

```text
exactly 1,000,000,000 whole test tokens
  -> 30% Founder / 50% Research Treasury / 12% Genesis / 8% LP
  -> the full 30% and 50% quantities enter the two B2 vaults
  -> both vaults share one policy/capacity window
  -> authority end states and every balance are reconstructed from RPC state
```

The existing B2 devnet fixture uses 9 decimals but deposits only `300,000,000`
and `500,000,000` base units: 0.3 and 0.5 whole test tokens. It establishes the
mechanism and ratios, not full economic scale. This specification does not
relabel that receipt; it defines its successor.

## Current implementation boundary

R3 now has three mutually checking layers. The LiteSVM test
`r3_b_transaction_created_full_scale_graph_reconciles_and_releases` creates the
mint and every token account through signed System/SPL transactions, performs
four separate `MintTo` operations, revokes mint and freeze authority, and then
continues the same Founder and Treasury accounts into B2. The loopback-only
`r3_full_scale_rpc_probe` repeats that construction through Surfpool JSON-RPC
and reconstructs it from standard account bytes. Finally,
`r3_full_scale_squads_probe.mjs` uses the real Squads v4 program on a local
devnet fork: it replaces a 2-of-3 member, approves with the new key set, advances
time, and executes the post-replacement release while B2's stored vault PDA is
unchanged. A separate read-only script validates the RPC state, canonical PDAs,
Squads member table and transaction statuses.

The passing full-scale graph contains:

- one classic-SPL mint account reporting `1,000,000,000,000,000,000` base
  units and 9 decimals;
- Founder/Treasury vault deposits of `300,000,000,000,000,000` and
  `500,000,000,000,000,000`;
- Genesis and LP accounts holding `120,000,000,000,000,000` and
  `80,000,000,000,000,000`;
- no remainder in either transaction-created deposit staging account;
- computed monthly caps `1,250,000,000,000,000` and
  `2,083,333,333,333,333`;
- exact shared-window rejection and success at the remaining headroom under
  `3,000,000,000,000,000` capacity.

The original R3-A fixture remains as an independent arithmetic regression, but
it is no longer the route frontier. The complete author-run result and all
twelve negative-case mappings are frozen in
`evidence/R3_FULL_SCALE_LOCAL_VALIDATION_2026-08-28.json`. The result is local
test evidence, not an unrelated reproduction, security review, production
parameter selection or mainnet authorization.

## Preconditions

1. The candidate configuration names one decimal count and one token program.
2. Every open production parameter remains visibly open; the test may carry an
   explicit `TEST_ONLY` value but never silently promote it to production.
3. The current B2 implementation uses classic SPL Token through
   `Program<'info, Token>`. A classic-token R3 pass does not cover Token-2022.
   Selecting Token-2022 requires a separate implementation/compatibility branch
   and the same acceptance vector against that program.
4. The exact B2 SBF under test is built before any LiteSVM/Surfpool test loads
   it. `cargo test` alone must not be allowed to reuse a stale `.so`.
5. All signers and assets are disposable local/test objects. No production key
   or public transaction appears in R3.

## Exact scale vectors

Let `S = 1,000,000,000 * 10^decimals` base units.

| Pool | Share | Base-unit formula |
|---|---:|---:|
| Founder | 30% | `S * 3000 / 10000` |
| Research Treasury | 50% | `S * 5000 / 10000` |
| Genesis | 12% | `S * 1200 / 10000` |
| LP | 8% | `S * 800 / 10000` |

For the two current decimal candidates:

| Decimals | Total supply | Founder | Treasury | Genesis | LP |
|---:|---:|---:|---:|---:|---:|
| 6 | `1000000000000000` | `300000000000000` | `500000000000000` | `120000000000000` | `80000000000000` |
| 9 | `1000000000000000000` | `300000000000000000` | `500000000000000000` | `120000000000000000` | `80000000000000000` |

Both total-supply vectors fit in `u64`; the selected vector must be recomputed by
the harness rather than copied from this table.

### Numeric representation floor

The 9-decimal quantities exceed JavaScript's exact integer range. Therefore:

- TypeScript/JavaScript literals and arithmetic use `bigint` or a checked BN
  type from the first byte; never construct them as `number` and convert later;
- instruction encoders accept `bigint`/BN directly;
- JSON receipts encode every raw token amount as an unsigned decimal string;
- Rust uses checked `u128` intermediates and checked conversion to `u64`;
- Python uses `int` and compares decimal strings at the JSON boundary;
- a guard test rejects any raw amount above `Number.MAX_SAFE_INTEGER` that has
  passed through a JavaScript `number`.

## Reference full-scale policy vector

This vector scales the current ratio fixture by the selected decimal factor and
remains `TEST_ONLY`, not a production recommendation.

For 9 decimals and an annual ceiling of 500 bps:

```text
founder monthly cap  = 1,250,000,000,000,000
treasury monthly cap = 2,083,333,333,333,333
eligible volume      = 120,000,000,000,000,000
capacity at 250 bps  = 3,000,000,000,000,000
joint cap sum         = 3,333,333,333,333,333
```

Each vault is individually within the capacity while their sum exceeds it by
`333,333,333,333,333`, preserving the exact reason the shared rule exists.

## Construction sequence

### A. Mint and allocation

1. Start a fresh in-memory LiteSVM or offline Surfpool instance.
2. Create one mint under the explicitly selected classic/Token-2022 program and
   decimal count.
3. Create four role-labeled allocation accounts plus the temporary deposit
   accounts required by B2.
4. Mint the exact four allocations and assert their sum equals the mint supply.
5. Create the B2 policy, market account and two vault states against that same
   mint.
6. Deposit the entire Founder and Treasury allocations into B2. Their staging
   accounts must end at zero; the B2 token vaults must hold the full amounts.
7. Move mint/freeze/metadata/upgrade authorities only according to the candidate
   test config and record every end state. Mint authority must end revoked.

### B. Governance and release behavior

8. Use a Squads test authority with its controller classification recorded.
9. Report the test-only eligible-volume vector.
10. Record purpose approvals for future periods, including one before and one
    after a simulated member replacement.
11. Assert beneficiary release before the 730-day cliff rejects.
12. Assert purpose release before its notice/period gates rejects.
13. Advance the local clock; execute one purpose release under the approved need
    and current shared window.
14. Replace one committee member with the surviving threshold and execute a
    post-replacement approval/release without changing the multisig address,
    vault PDA or stored approver.
15. Exercise exact cap, cap+1, joint capacity, stale oracle, zero volume, wrong
    oracle, wrong policy, wrong destination, no-carry and pending-rotation cases.

### C. Independent reconstruction interface

16. Export only RPC account/transaction responses and the public candidate
    config; do not pass an author-written expected-state snapshot to the reader.
17. A read-only verifier derives and checks mint supply, all four allocation
    amounts, vault PDAs/owners, full deposits, caps, counters, approval records,
    authority addresses and token conservation.
18. Rebuild or dump the exact SBF and bind its hash into the receipt.

## Required negative cases

| ID | Attempt | Required result |
|---|---|---|
| R3-N01 | Mint one additional base unit after authority revocation | Reject |
| R3-N02 | Founder or Treasury staging balance remains nonzero after full deposit | Reconciliation fail |
| R3-N03 | Founder release one second before cliff | `CliffActive` |
| R3-N04 | Purpose release before 30-day notice | `NoticePeriodActive` |
| R3-N05 | Either vault exceeds its monthly cap by one | Reject |
| R3-N06 | Individually legal releases jointly exceed the shared window by one | Reject |
| R3-N07 | Carry unused capacity into the next period | Reject or prove fresh-cap-only behavior |
| R3-N08 | Stranger reports volume | Reject |
| R3-N09 | Captured oracle inflates volume past the market gate | Per-vault schedule and test hard ceiling still bind |
| R3-N10 | Lost committee member signs after replacement | Signature absent/insufficient; no role |
| R3-N11 | Upgradeable program is reported as immutable | Receipt validation fail |
| R3-N12 | A raw amount above `2^53-1` passes through JavaScript `number` | Harness fail before transaction construction |

Error names may differ if the implementation evolves; the receipt pins both
the semantic condition and the exact observed error.

## Receipt schema

The R3 receipt must contain:

```text
schema and epistemic status
cluster and genesis hash
candidate-config SHA-256
source commit, toolchain, SBF SHA-256 and byte length
token program, mint, decimals, raw supply
four allocation accounts and raw balances
B2 program, policy, market, vault, token-vault and approval accounts
all authority addresses and controller classifications
successful transaction signatures or local transaction identifiers
expected-refusal identifiers and exact errors
pre/post member sets with stable multisig/vault addresses
raw-amount reconciliation
open production parameters and explicit non-claims
```

All raw quantities are decimal strings. `valid=true` is impossible if any
required field is absent, any amount fails reconciliation, an open parameter is
presented as production, or the authority claim differs from the actual program
metadata.

## Acceptance

The complete R3 is `PASS — LOCAL FULL-SCALE INTEGRATION` only when:

- all four allocation and vault amounts reconcile exactly on one mint;
- all selected quantities fit their on-chain integer types;
- the current compiled B2 SBF executes the full-scale path;
- every required negative case passes;
- the post-replacement path succeeds without changing B2's stored authority;
- the RPC-only verifier accepts with no private expected-state snapshot;
- the receipt says `LOCAL/TEST EVIDENCE`, `NOT INDEPENDENT`, `NO OFFICIAL MINT`,
  `NO MAINNET AUTHORIZATION` and lists every still-open production parameter.

All of these conditions passed on 2026-08-28. The public test binding is
`R3_TEST_ONLY_CANDIDATE_v1.json`; it pins the reproducible local vector while
retaining 26 open production parameters and explicitly does not freeze any of
them. The next gate is R4 unrelated clean-room reproduction and procured review.

This closes the scale/integration node only. It does not close independent
reproduction, security review, production governance, legal issuance, demand,
funding, LP or mainnet gates.
