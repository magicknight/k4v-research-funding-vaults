# Security policy

Version 0.1 is an executable specification and test fixture, not production
custody software. Do not use it to hold or release real assets.

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
