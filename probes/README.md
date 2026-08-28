# Solana probe status

solana_fixed_supply_probe.cjs is the exact JavaScript mechanism used for the
recorded 2026-08-09 Agave localnet evidence. It creates ephemeral payer, mint,
and owner keys; mints the 30/50/12/8 fixture; revokes mint and freeze
authorities; and reconciles the resulting accounts.

The historical run used @solana/web3.js 1.x and @solana/spl-token 0.4.14. The
current dev-only package pins the closest maintained legacy stack needed by the
Squads v4 SDK. `npm audit --omit=dev` is clean; the full dev tree reports known
transitive advisories. Do not use these scripts with production keys or a
mainnet endpoint.

The repair path is either:

1. migrate non-Squads probes to the current Solana Kit client, and migrate the
   Squads probe when its SDK exposes a compatible Kit surface; or
2. implement the same fixture with a pinned, independently verified Agave CLI
   toolchain that has no affected JavaScript dependency path.

This dependency failure is local to the historical runner. It does not alter
the Python covenant, the receipt format, or the already recorded localnet
state; it does block treating this JavaScript file as a supported deployment
tool.

`r3_full_scale_squads_probe.mjs` is the active local R3-B2 runner. It accepts
only a loopback RPC, uses `bigint` for every raw amount, self-tests that unsafe
JavaScript numbers are rejected, creates no key files, and uses the real Squads
v4 program through a local Surfpool devnet fork. Its companion
`r3_full_scale_squads_verify.mjs` signs nothing and treats the probe receipt only
as an address index; it reconstructs the verdict from RPC account bytes,
canonical PDAs, the Squads member table and transaction statuses.

These two files establish local key-loss resilience and post-replacement
execution. Their three generated members are still controlled by one test
process, so they do not establish independent governance or security review.
