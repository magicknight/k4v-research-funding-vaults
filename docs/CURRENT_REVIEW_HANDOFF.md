# Current v7 review scope: E11B, E11C and bounded E-SOAK recovery

Status: material ready for review; **no named independent human review accepted**.

Start with the [offline evidence walkthrough](EVIDENCE_DEMO.md) for a wallet-free,
network-free introduction to the archived initialization and continuation results.
It is a presentation of rechecked historical bytes, not a fresh execution or review.

Review the exact commit supplied with the handoff and record its Git tree. The
canonical TEST_ONLY v7 program remains 553,624 bytes with SHA-256
`df1011597eeda9e2013d648d3f3be8e840a4bcab8a0e3143e2a9c161cd0f3b67`.
No production annual inputs, recovery people, release rights or upgrade choice
are selected by these tests. The default program still disables admission.

| Review unit | Exact entry and evidence | What its success establishes |
|---|---|---|
| E11B | `bash tools/run_e11b_acceptance.sh`; `E11B_SHA256SUMS`; `docs/E11B_REVIEW_HANDOFF.md` | Rebuilt v7, role signatures, financial regressions, actual local initialization |
| E11C archive | `sha256sum -c E11C_SHA256SUMS`; `python3 tools/verify_e11c_archive.py` | Offline replay of the fixed author-run mature-state evidence |
| E11C fresh run | `.github/workflows/e11c-continuation.yml` | Actual Agave execution from two explicitly preloaded mature states |
| E-SOAK-01 | `docs/E_SOAK_OBSERVATION.md`; journal tests; `tools/soak_agave_smoke.mjs` | Durable sampled observation records, offline revalidation, observer restart on a short-lived real node |
| E-SOAK-02 | `docs/E_SOAK_PERSISTENT.md`; `tools/soak_persistent.mjs`; dedicated workflow | Finite signed bootstrap, same-ledger node restart, locked full-run backup/restore, retained test-key continuation; acceptance requires the exact run's receipt |

An old R3/B2 or Squads-only screen is a different scope and cannot sign off the
current v7 recovery/initialization/accounting code. An AI service may contribute
findings but cannot supply the named-human responsibility absent a real person
who explicitly accepts and performs that role. No vendor is retained by this
document.

For the journal, challenge changed genesis, policy or SBF, omitted account
history, concurrent writers, interrupted writes, storage failures, stale-bank
replay, clock jumps, unclosed observer sessions, deleted journal tails and
untrusted local RPC. Confirm that raw bytes are redecoded and that no uptime,
natural-soak or scientific conclusion follows from a self-reported timestamp.
Test loss/replacement of the host and backups separately before accepting a
durable operational deployment.

For the persistent driver, review retention settings, startup/history readiness,
internal snapshot-link relocation, source/restore byte equality, backup-head
pinning, live-copy refusal, key-file permissions, ambiguous signed attempts and
the public-artifact allowlist. Full ledgers and test-key backups are private
local runtime state, not CI uploads. Planned process restarts do not establish
power-loss recovery, off-host durability or a natural maturity history.

Use the existing E11B threat checklist for mint authority, preparation consent,
substitution/replay, role recovery, accounting continuity and loader control.
Preserve each finding, exact reproduction, severity, fix commit and retest. Do
not treat archived author/CI results as the reviewer's own reproduction.

## Required returned report

The reviewer must provide the following in a dated report; blank fields mean the
relevant gate remains open:

1. Human name, accountable organization or contracting identity, qualifications,
   conflicts/independence and compensation; distinguish any AI assistance.
2. Checkout commit/tree, all reviewed paths, tested SBF sizes/hashes, environment,
   commands and raw logs. Label archived replay and fresh execution separately.
3. Findings with scope/severity, runnable reproduction, expected/observed behavior,
   affected rights, remediation and exact retest evidence.
4. Explicit exclusions: natural long history, production inputs and role custody,
   public deployment, issuer/legal/payer, demand and scientific validation.
5. What was accepted, what remains unresolved, sign-off identity and date. A clean
   automated test report alone does not complete this human acceptance.

Before procurement, obtain a written quote for **this exact scope**, including
currency, tax, deliverables, one or more retests, validity period, invoice identity,
payment timing, failure/cancellation costs and maximum liability. A historical
quote for older code is not an accepted current quote. Sending an inquiry,
accepting a quote, appointing signers and making payment are separate actions;
none is performed by publishing this technical handoff.
