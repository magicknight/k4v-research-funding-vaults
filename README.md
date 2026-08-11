# Purpose-Bound Research-Funding Vaults

This repository turns “we will release tokens slowly and only for a stated
purpose” from a verbal promise into a public, testable object.

The strongest current artifact is a chain-independent integer covenant with
deterministic SHA-256 decision receipts. It models separate beneficiary and
purpose vaults, a cliff, monthly non-carrying caps, an approved-need gate, a
joint market-capacity cap, and circulation-equivalent bypasses such as OTC,
collateral, grants, and transfers of economic rights.

The repository also contains a Solana fixed-supply probe and a recorded Agave
localnet run. The dynamic on-chain vault program is the main open bridge.

## What is established

- The reference covenant passes 14 deterministic unit tests.
- Identical requests produce identical decision receipts; changed inputs
  change the receipt hash.
- Under the recorded Agave 4.2.0 localnet run, an SPL mint with
  1,000,000,000 whole units, a 30/50/12/8 fixture, and permanently revoked mint
  and freeze authorities was realizable.

## What is not established

- No production Solana vault program exists in this release.
- The code has not received an independent security audit.
- The recorded transaction signatures are localnet evidence, not public-chain
  explorer proofs.
- This release does not establish legal eligibility, mainnet readiness,
  liquidity, scientific claims, token demand, or token value.

## Quick start

Python 3.10+ and only the standard library are required for the covenant.

~~~sh
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py' -v
PYTHONPATH=src python3 src/covenant_cli.py examples/k4v_release_request.json
~~~

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

- **Champion:** implement immutable or monotonic Solana beneficiary and
  purpose vault programs with reproducible adversarial tests.
- **Independent alternative:** publish a read-only verifier that reconstructs
  compliance from disclosed accounts and receipts without controlling funds.
- **Decisive probe:** implement the smallest PDA-based beneficiary cliff and
  prove that every release path is rejected before the cliff.
- **Reliable core:** this executable covenant, receipt format, threat model,
  fixed-supply probe, and recorded local evidence.

See [ROADMAP.md](docs/ROADMAP.md) for the proposed five-milestone public-good
program and [COMMERCIAL_DISCLOSURE.md](docs/COMMERCIAL_DISCLOSURE.md) for the
K4V conflict boundary.

## License

Licensed at your option under either the MIT License or Apache License 2.0.
