# Claude independent-model AI pre-audit

This directory preserves a read-only static review of public commit
`e1afead138fbf56956b298ebae7a97a8ae9ad956`, produced by Claude Code 2.1.252
with the primary exposed model `claude-opus-5`.

The review found no claimed theft, double-spend, critical or high-severity path.
It reported seven medium design/DoS/governance findings, eight low findings and
a `NOT_READY` mainnet verdict. Findings retain Claude's labels and are not
silently promoted to project verdicts: several describe deliberate fail-closed
or shared-capacity tradeoffs that require a Founder design decision.

Classification is `INDEPENDENT_MODEL_AI_PREAUDIT`. It is not a human-accountable
security audit, legal opinion, deployment attestation or mainnet authorization.
Claude did not build, execute, query RPC or verify live accounts. The separate
xv4 clean-room run supplies execution evidence and is recorded independently.

Files:

- `AUDIT_PROMPT.md` — frozen neutral review request;
- `CLAUDE_AUDIT_REPORT.md` — extracted report;
- `PREAUDIT_CLASSIFICATION.md` — independent classification and limitations;
- `CITATION_VERIFICATION.json` — machine-readable location check;
- `CONTEXT_FRAGMENT_RESOLUTION.tsv` — context-only citation resolution;
- `verify_citations.pl` — verifier used for the location check;
- `RUN_METADATA.json` — source, model, result and hashes.

No production key, transaction or private planning artifact was provided to the
reviewer.
