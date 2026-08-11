# Solana probe status

solana_fixed_supply_probe.cjs is the exact JavaScript mechanism used for the
recorded 2026-08-09 Agave localnet evidence. It creates ephemeral payer, mint,
and owner keys; mints the 30/50/12/8 fixture; revokes mint and freeze
authorities; and reconciles the resulting accounts.

The historical run used @solana/web3.js 1.x and @solana/spl-token 0.4.14.
As of this release, npm audit reports known transitive vulnerabilities in that
dependency family. The source and evidence are preserved for reproducibility,
but the vulnerable packages are deliberately not installed by package.json.
Do not use this script with production keys or a mainnet endpoint.

The repair path is either:

1. migrate the probe to the current Solana client and token packages, then
   repeat localnet and public testnet reconciliation; or
2. implement the same fixture with a pinned, independently verified Agave CLI
   toolchain that has no affected JavaScript dependency path.

This dependency failure is local to the historical runner. It does not alter
the Python covenant, the receipt format, or the already recorded localnet
state; it does block treating this JavaScript file as a supported deployment
tool.
