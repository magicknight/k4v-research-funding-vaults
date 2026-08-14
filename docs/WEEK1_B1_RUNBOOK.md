# Week 1 B1 runbook

This runbook reproduces the first constructive milestone: compile the
beneficiary vault to SBF, execute its custody and release rules, and verify a
disclosed state snapshot independently.

## Frozen scope

- Anchor program: `programs/beneficiary-vault`
- instructions: `deposit`, `release`
- SPL Token program, not Token-2022
- minimum cliff: 730 days
- period: fixed 30-day windows after cliff
- maximum annual rate: 500 basis points
- no carry, top-up, reconfiguration, close, migration, pause, or emergency
  release instruction

Purpose approval, a market-capacity oracle, calendar/year-start accounting,
RPC account export, deployment authority, and public deployment are not part of
Week 1 acceptance.

## Tested environment

~~~text
OS: Ubuntu 22.04-compatible glibc 2.35 host
Rust: 1.89.0 (pinned by rust-toolchain.toml)
Solana CLI: 3.1.10
Anchor crates: 1.1.2
Anchor CLI: 1.1.2 source build (not required by the commands below)
LiteSVM: 0.10.0
Python: 3.10+
~~~

The official Anchor prebuilt CLI requires a newer glibc on this host, so the
local CLI was compiled from source. The program and tests do not depend on the
Anchor CLI binary; they use `cargo build-sbf` and Cargo directly.

## Reproduction

Install the Solana CLI and Rust, then run from the repository root:

~~~sh
NO_DNA=1 cargo build-sbf --manifest-path programs/beneficiary-vault/Cargo.toml \
  --sbf-out-dir target/deploy
cargo fmt --all -- --check
cargo test --workspace --locked
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py' -v
PYTHONPATH=src python3 src/beneficiary_vault_verifier.py \
  examples/beneficiary_vault_snapshot.json
npm ci
npm run check
sha256sum -c SHA256SUMS
~~~

`target/deploy/beneficiary_vault-keypair.json` is an ephemeral build product and
is ignored. The committed `declare_id!` value is a test identity used to load
the SBF program in LiteSVM. Do not treat either as a production program key.

## Expected decisive tests

The LiteSVM suite must report five passing tests:

1. deposit creates both PDAs and moves the complete deposit into the
   PDA-authorized SPL account;
2. pre-cliff release and cap-plus-one fail, while exact-cap releases succeed;
3. unused first-period capacity does not raise the second-period cap;
4. a token destination not owned by the beneficiary is rejected;
5. a deposit whose cliff is one second below the frozen minimum is rejected
   atomically.

The Rust unit suite must also match the Python verifier's published PDA vector.
The Python suite must reject a shortened cliff, forged cap or bump, broken
token conservation, and must produce deterministic, input-sensitive receipts.

## Evidence boundary

The local `.so` hash identifies one build, but ordinary Solana builds are not
assumed reproducible across machines. A production claim requires a container
verified build, an on-chain executable hash comparison, and disclosed loader
authority. LiteSVM is the appropriate fast Week 1 mechanism test; a Surfpool
JSON-RPC run and raw-account export remain the next probe. A full validator is
reserved for loader or runtime-fidelity checks that the surfnet does not
emulate.
