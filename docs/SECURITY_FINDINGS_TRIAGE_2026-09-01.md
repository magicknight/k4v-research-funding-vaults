# Security findings triage — 2026-09-01

> Source review: `reviews/2026-09-01-claude-ai-preaudit/`
>
> State: `AI PREAUDIT RECEIVED / HUMAN ACCOUNTABILITY OPEN / MAINNET NOT READY`

This triage does not promote Claude's labels to final project verdicts. It
preserves the strong surviving result—no confirmed theft, double-spend,
above-schedule release, critical or high-severity path—while routing each
reported mechanism to a repair, adversarial test or explicit design decision.

## Medium findings

| ID | Project state | Smallest node | Required next move |
|---|---|---|---|
| K4V-01 | `REPAIRABLE / PRODUCTION BLOCKER CANDIDATE` | B2 policy/market PDA namespace omits creator | Add an adversarial squatting test; choose creator-bound seeds or an authority-bound policy digest before production |
| K4V-02 | `REPAIRABLE / PRODUCTION BLOCKER CANDIDATE` | B1 vault PDA namespace permits dust squatting | Add the reported 240-base-unit test; choose depositor-bound namespace or beneficiary co-signature |
| K4V-03 | `REPAIRABLE / PRODUCTION BLOCKER CANDIDATE` | Programs do not enforce mint authority end state | Test a frozen vault token; decide whether both mint and freeze authority must be absent before any deposit |
| K4V-04 | `DESIGN DECISION OPEN` | Shared policy window is competitive, not reserved per vault | Run reverse-order and multi-period starvation probes; either add reservations/pro-rata semantics or disclose competitive headroom explicitly |
| K4V-05 | `KNOWN FAIL-CLOSED POWER / DESIGN DECISION OPEN` | One oracle can halt release; fastest rotation is 90 days | Quantify halt/recovery windows; choose quorum reporters, emergency rotation, or retain and disclose deliberate fail-closed behavior |
| K4V-06 | `KNOWN GOVERNANCE POWER / DESIGN DECISION OPEN` | Policy authority cannot rotate and controls oracle rotation | Model loss/compromise with a real multisig controller; choose noticed authority transfer or deliberately permanent authority |
| K4V-07 | `DOCUMENTATION REPAIR APPLIED / PRODUCTION GATE OPEN` | Instruction immutability was not always separated from loader upgradeability | B1/B2 specs now state the boundary; production still requires exact controller disclosure and transfer/revocation decision |

No architecture-changing fix is authorized merely by this table. K4V-01 to
K4V-06 alter PDA identity, depositor/beneficiary signing, accepted mints,
allocation fairness or long-lived recovery powers. They require executable
adversarial tests and a Founder-reviewed route-change transaction rather than a
silent patch.

## Low and informational repairs

The lower-risk set is independently useful and should be taken in a separate
mechanical hardening pass:

- reject or redesign immediately-next-period approvals that can never mature;
- add version/reserved-space pins before changing account layouts;
- reject obviously unreleasable authority keys or require the authority to sign;
- decide and disclose the approval-account rent/retention policy;
- replace source-text instruction counting with IDL/discriminator checks;
- pin every declared time constant with literal regression assertions;
- authenticate owner, discriminator, PDA and verdict exit status in the legacy
  B2 devnet verifier;
- bind loaded/runtime program bytes to the reported SBF hash;
- reconcile model `market_capacity_bps=0`, evidence commitment level, stale
  deployment prose, per-period ceiling wording and boundary-value cases.

The 2026-09-01 frozen-SBF repair already closes one tooling gap found by the
separate xv4 execution: a clean-room run can no longer pass unless the built SBF
equals the test candidate and both receipts carry the same hash.

## Surviving core

- exact 1e18 local test supply and 30/50/12/8 reconciliation;
- mint/freeze revocation in the R3 test graph;
- full Founder/Treasury B2 deposits and zero staging balances;
- cap, cliff, notice, approval, freshness and shared-window enforcement under
  the current bytes;
- Squads member replacement with stable vault PDA;
- read-only R3 verifier, negative cases and functional blank-host reproduction;
- public-devnet B1/B2 evidence and explicit no-mainnet boundary.

## Current gates

```text
R4A operational environment reproduction  PARTIAL PASS
R4A frozen-byte replay after repair         LOCAL PASS / PUBLIC CLEAN RERUN OPEN
R4B independent-model static preaudit       PASS, bounded
R4B human-accountable security audit        OPEN
mainnet readiness                           NOT_READY
```

`TARGET_GATE: OPEN`  
`MAIN_OPEN_BRIDGE: adversarially test and decide K4V-01..06, repair accepted descendants, then obtain human-accountable re-review`
