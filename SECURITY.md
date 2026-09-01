# Security policy

Version 0.1 is an executable specification and test fixture, not production
custody software. Do not use it to hold or release real assets.

## 2026-09-01 independent-model pre-audit

A public-only, read-only Claude Opus pre-audit of commit `e1afead` is preserved
under [`reviews/2026-09-01-claude-ai-preaudit/`](reviews/2026-09-01-claude-ai-preaudit/).
It reported no critical or high-severity path and no confirmed theft,
double-spend or above-schedule release path. It reported seven medium
design/DoS/governance findings, eight low findings and a `NOT_READY` mainnet
verdict. The medium set includes permissionless namespace squatting, mint
freeze-authority lockup, shared-window starvation, oracle/authority liveness and
upgrade-authority disclosure.

This is `INDEPENDENT_MODEL_AI_PREAUDIT`, not a human-accountable audit. Claude
did not build, execute, query RPC or verify live deployment state. The separate
xv4 run supplies operational execution evidence; accountable external review,
architecture decisions, repairs and adversarial re-tests remain required before
any production claim.

The recorded 2026-08-09 JavaScript probe used @solana/web3.js 1.x and
@solana/spl-token 0.4.14 in an isolated localnet run. Their present npm audit
tree contains known advisories, including a high-severity bigint-buffer
advisory. Those packages are not repository dependencies and the probe is not
a production execution path. A future runnable probe must migrate SDKs or use
a repaired toolchain and pass a fresh dependency audit.

Report suspected defects privately to zhihua@k4cell.com. Include the version,
input, expected decision, observed decision, and a minimal reproduction. Do
not send private keys, seed phrases, identity documents, or live credentials.

Security reports that affect a published claim will be acknowledged in the
issue tracker after sensitive details are contained. Negative results and
scope reductions will remain visible in release notes.
