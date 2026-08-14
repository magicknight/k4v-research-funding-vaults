# Purpose-Bound Research-Funding Vaults

This repository turns “we will release tokens slowly and only for a stated
purpose” from a verbal promise into public, testable code.

The strongest current capability is B1: a two-instruction Anchor program whose
PDA-owned SPL token vault enforces a beneficiary cliff and a frozen,
non-carrying period cap. Its compiled SBF artifact has been executed inside
LiteSVM against real System and SPL Token CPIs.

The broader, chain-independent covenant remains alongside B1. It models
separate beneficiary and purpose vaults, approved need, joint market capacity,
and circulation-equivalent bypasses such as OTC, collateral, grants, and
transfers of economic rights. B1 does not pretend those broader gates are
already on chain.

## What is established

- The compiled B1 program passes five LiteSVM integration tests covering
  deposit custody, cliff rejection, exact-cap/cap-plus-one behavior, genuine
  no-carry behavior, and beneficiary-owned destinations.
- Rust unit tests cover the exact two-floor rate formula, period boundaries,
  and a cross-language PDA vector.
- The Python layer passes 30 deterministic tests; an additional opt-in live
  Surfpool test exercises the actual JSON-RPC boundary.
- An opt-in loopback-only Surfpool probe simulates and sends signed setup,
  deposit, and capped-release transactions with in-memory signers, observes
  the expected `CliffActive` rejection, and then reproduces the resulting raw
  RPC accounts through the independent exporter/verifier.
- Identical requests produce identical decision receipts; changed inputs
  change the receipt hash.
- The standard-library verifier recomputes both PDAs, bumps, cliff, cap,
  counters, token bindings, and token conservation without controlling funds.
- The read-only RPC exporter checks account owners, exact lengths, the Anchor
  discriminator, canonical PDAs, Clock sysvar, and classic SPL Token layout;
  it rejects delegated, closable, frozen, or native vault-token states.
- Unsolicited direct token transfers cannot raise the frozen lifetime release
  entitlement; the verifier reports that excess as surplus while still
  rejecting any custody deficit.
- Under the recorded Agave 4.2.0 localnet run, an SPL mint with
  1,000,000,000 whole units, a 30/50/12/8 fixture, and permanently revoked mint
  and freeze authorities was realizable.

## What is not established

- No public-chain or production Solana vault deployment exists in this branch.
- The code has not received an independent security audit.
- The repository program ID is a test identity, not a deployment address.
- The deterministic live-RPC fixture still uses injected bytes. The separate
  transaction probe creates mint, token, vault and release state through
  signed local transactions, but loads the SBF program with Surfpool's
  local-only cheatcode rather than a loader deployment. Neither is a public
  cluster or production-deployment claim.
- B1 uses fixed 30-day periods and the initial deposit as its cap basis. It is
  not yet semantically identical to calendar-month/year-start accounting.
- The recorded transaction signatures are localnet evidence, not public-chain
  explorer proofs.
- This release does not establish legal eligibility, mainnet readiness,
  liquidity, scientific claims, token demand, or token value.

## Quick start

Python 3.10+ and only the standard library are required for the covenant.

~~~sh
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py' -v
PYTHONPATH=src python3 src/covenant_cli.py examples/k4v_release_request.json
PYTHONPATH=src python3 src/beneficiary_vault_verifier.py examples/beneficiary_vault_snapshot.json
PYTHONPATH=src python3 src/beneficiary_vault_rpc_exporter.py \
  --rpc-url http://127.0.0.1:8899 \
  --program-id PROGRAM_ID --vault-state VAULT_STATE
~~~

The B1 Rust tests require Rust 1.89 and Solana CLI 3.1.10. Build the SBF
artifact before running the workspace tests:

~~~sh
NO_DNA=1 cargo build-sbf --manifest-path programs/beneficiary-vault/Cargo.toml \
  --sbf-out-dir target/deploy
cargo test --workspace --locked
~~~

See [WEEK1_B1_RUNBOOK.md](docs/WEEK1_B1_RUNBOOK.md) for exact scope, toolchain,
expected tests, and artifact commands.

To syntax-check the recorded non-mainnet Solana probe:

~~~sh
npm ci
npm run check
~~~

The probe creates ephemeral keys in memory and accepts only localnet, devnet,
or testnet. It rejects mainnet configuration. Its historical JavaScript
dependencies are intentionally not installed by default because their current
npm dependency tree contains known advisories. See [probes/README.md](probes/README.md).

## Frontier map

- **Champion:** extend the proven B1 custody core into a purpose vault without
  creating a second release counter or an acceleration authority.
- **Independent alternative:** the implemented read-only RPC adapter can
  reconstruct a B1 snapshot and receipt without controlling custody.
- **Decisive probe:** replace the local program-loading cheatcode with a real
  loader deployment, reproduce the same transaction/RPC receipt on a public
  devnet, and obtain an independent preflight. Reserve
  `solana-test-validator` for loader or validator-fidelity checks that Surfpool
  does not emulate.
- **Reliable core:** this executable covenant, receipt format, threat model,
  B1 source and local SBF tests, fixed-supply probe, and recorded local evidence.

See [ROADMAP.md](docs/ROADMAP.md) for the proposed five-milestone public-good
program and [COMMERCIAL_DISCLOSURE.md](docs/COMMERCIAL_DISCLOSURE.md) for the
K4V conflict boundary. The B1 program contract is specified in
[BENEFICIARY_VAULT_B1.md](spec/BENEFICIARY_VAULT_B1.md).

## License

Licensed at your option under either the MIT License or Apache License 2.0.
