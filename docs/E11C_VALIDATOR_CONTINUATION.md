# E11C: actual-validator continuation — accepted, bounded evidence

Status: ACTUAL LOCAL VALIDATOR CONTINUATION PASS; production readiness remains open.
Parent: accepted E11B PR #26 / canonical v7. Duplicate E11B draft #27 was closed without merging.

The accepted push run is **34459155355**, source commit
`da817b36212d233fb8b6816002f4017ddbe8f6a1`, source tree
`150fa995e2acc83cfc71326d6d4fe8cdd9575477`.
Its downloaded artifact digest and permanent evidence archive are pinned in
[`E11C_ACCEPTED_2026-09-10.json`](../evidence/E11C_ACCEPTED_2026-09-10.json).
The original program and 90/180-day contractual intervals are unchanged.

## What passed

Two actual Agave 3.1.10 runs completed **11 finalized client transactions and
8 full-history raw-RPC checkpoints**. Recovery has six transactions and four
checkpoints; expiry/year-two has five transactions and four checkpoints.
The application fixtures contain the complete explicitly preloaded mature state.

Recovery executes both mature proposals and accepts withdrawals by the new keys.
Expiry rejects late execution, clears the expired proposal, refreshes the oracle
and permits budget-limited continued withdrawals under the second annual rule.
Old keys, stale epochs, old destinations, quota excess and budget excess are
refused. The independent raw-byte decoder verifies supply conservation,
principal/T0/identity preservation, unchanged counters during authority changes,
and exact accounting changes only for authorized token transfers. Refused
operations leave the observed application account bytes unchanged.

The archive was downloaded and its two raw-byte verifications independently
rerun in a separate local process. This is decoder/process separation, **not a
named independent human security review**. Refusal receipts are explicitly
labelled simulation rejections, not invented finalized failed transactions.

## Reproduce and inspect

Offline, without RPC, keys or downloads:

```sh
sha256sum -c E11C_SHA256SUMS
python3 tools/verify_e11c_archive.py
```

Fresh actual-node execution is provided by `.github/workflows/e11c-continuation.yml`,
using the same pinned validator and frozen v7 program. The one-time artifact
publication permission has been removed; routine CI has contents-read only.

## Exact boundary

`application_state_preloaded=true`, `fixture_clock_controlled=true`,
`actual_validator_clock_override=false`, `natural_90_180_day_soak=false`.
The two validator scenarios are separate fixtures, not one uninterrupted
bootstrap-to-maturity history. These results verify actual execution **from**
mature states, not that a CI job naturally ran for 90 or 180 days.

No public RPC transaction, mint, public deployment, real funds, production-rights
change, website change or outreach occurred. Feedback remains paused and demand
unverified. Natural long-duration continuous-history evidence, human-accountable
review, production annual inputs/rates/rights/actors, issuer/legal/payer and
launch/funding gates remain open. Engineering evidence is not launch permission.
