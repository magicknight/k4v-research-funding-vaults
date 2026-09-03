# Security findings triage — 2026-09-01

> Source review: `reviews/2026-09-01-claude-ai-preaudit/`
>
> State: `AI PREAUDIT RECEIVED / K4V-01..04 PROBES CONFIRMED 2026-09-03 / FOUNDER DECISIONS OPEN / HUMAN ACCOUNTABILITY OPEN / MAINNET NOT READY`

This triage does not promote Claude's labels to final project verdicts. It
preserves the strong surviving result—no confirmed theft, double-spend,
above-schedule release, critical or high-severity path—while routing each
reported mechanism to a repair, adversarial test or explicit design decision.

## Medium findings

| ID | Project state | Smallest node | Required next move |
|---|---|---|---|
| K4V-01 | `CONFIRMED ON CURRENT BYTES / FOUNDER DECISION OPEN` | B2 policy/market PDA namespace omits creator | A stranger opened the published digest, froze authority/oracle/ceiling=1, and blocked the intended operator. Choose creator-bound seeds or an authority-bound digest before production |
| K4V-02 | `CONFIRMED ON CURRENT BYTES / FOUNDER DECISION OPEN` | B1 vault PDA namespace permits dust squatting | 240 base units at 500 bps occupy `(beneficiary, mint, policy_hash)`; the independent verifier accepts the dust vault. Choose depositor-bound seeds or beneficiary co-signature |
| K4V-03 | `CONFIRMED ON CURRENT BYTES / FOUNDER DECISION OPEN` | Programs do not enforce mint authority end state | Deposit succeeded with a live freeze authority; `FreezeAccount` then locked B1 and both B2 vaults. Decide whether freeze and mint authorities must be absent on chain before any deposit |
| K4V-04 | `CONFIRMED ON CURRENT BYTES / FOUNDER DECISION OPEN` | Shared policy window is competitive, not reserved per vault | Purpose-first zeroes the beneficiary for six periods when `hard_ceiling = purpose cap`, and leaves only 416,667 of 1,250,000 under the published devnet ceiling. Choose reservation/pro-rata semantics or an explicit competitive-headroom disclosure |
| K4V-05 | `KNOWN FAIL-CLOSED POWER / DESIGN DECISION OPEN` | One oracle can halt release; fastest rotation is 90 days | Quantify halt/recovery windows; choose quorum reporters, emergency rotation, or retain and disclose deliberate fail-closed behavior |
| K4V-06 | `KNOWN GOVERNANCE POWER / DESIGN DECISION OPEN` | Policy authority cannot rotate and controls oracle rotation | Model loss/compromise with a real multisig controller; choose noticed authority transfer or deliberately permanent authority |
| K4V-07 | `DOCUMENTATION REPAIR APPLIED / PRODUCTION GATE OPEN` | Instruction immutability was not always separated from loader upgradeability | B1/B2 specs now state the boundary; production still requires exact controller disclosure and transfer/revocation decision |

No architecture-changing fix is authorized merely by this table. K4V-01 to
K4V-06 alter PDA identity, depositor/beneficiary signing, accepted mints,
allocation fairness or long-lived recovery powers. The 2026-09-03 LiteSVM
probes close the executable-test node for K4V-01..04
(`evidence/K4V_ADVERSARIAL_PROBES_2026-09-03.json`) without repairing the
bytes. A Founder-reviewed route-change transaction is still required; a silent
patch is not.

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
R4A frozen-byte replay after repair         PUBLIC CLEAN-CLONE PASS, same-user bounded
R4A K4V-01..04 adversarial probes           CONFIRMED ON CURRENT BYTES
R4B independent-model static preaudit       PASS, bounded
R4B human-accountable security audit        OPEN
mainnet readiness                           NOT_READY
```

`TARGET_GATE: OPEN`  
`MAIN_OPEN_BRIDGE: Founder decides K4V-01..06, repair accepted descendants, then obtain human-accountable re-review`
