# Public-good roadmap

## Champion: USD 30,000 / 16 weeks

| Milestone | Weeks | Amount | Acceptance artifact |
|---|---:|---:|---|
| M1 — Covenant and threat model | 1–3 | USD 3,000 | Frozen integer spec, receipt schema, bypass taxonomy, CI |
| M2 — Solana vault prototype | 3–8 | USD 9,000 | Beneficiary and purpose vaults on localnet/devnet |
| M3 — Adversarial harness | 7–11 | USD 7,000 | Authority, oracle, OTC, collateral, threshold and signer-loss tests |
| M4 — Independent reproduction/security preflight | 11–14 | USD 8,000 | Clean-room reproduction and independent report with repairs |
| M5 — Release and integration guide | 14–16 | USD 3,000 | Tagged source, checksums, deployment and incident guide |

Amounts are planning inputs, not vendor quotes. A funded budget must name the
owner of each milestone and separate applicant labor, external engineering,
and independent security review.

## Independent alternative: USD 10,000 / 8 weeks

Freeze M1, implement a narrow beneficiary-cliff prototype, and publish a
read-only verifier plus public test report. This alternative does not claim
the purpose-approval, oracle, or independent-review coverage of the champion.

## Next decisive construction

Complete B1 by running its compiled SBF artifact inside LiteSVM: deposit must
create the state and token PDAs, pre-cliff release must fail, exact-cap release
must succeed, cap-plus-one must fail, and a later period must reset rather than
carry capacity. Then export the resulting account bytes through a read-only RPC
adapter backed by Surfpool and reproduce the Python verification receipt from
a second machine. Use `solana-test-validator` only for loader or
validator-fidelity checks that the surfnet does not emulate.

The following bridge is deliberately next, not silently included in B1:
replace fixed 30-day windows and initial-deposit rate basis with a precisely
specified calendar/year-start mechanism before claiming parity with the full
covenant.

## Success and validation

Success requires a reusable Solana capability, not merely more documentation.
Validation requires deterministic tests, exact artifact provenance, negative
test publication, and independent reproduction before any production claim.
