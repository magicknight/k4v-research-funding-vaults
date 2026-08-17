# Purpose-Bound Research-Funding Vaults

This repository turns “we will release tokens slowly and only for a stated
purpose” from a verbal promise into public, testable code.

The strongest current capability is B1: a two-instruction Anchor program whose
PDA-owned SPL token vault enforces a beneficiary cliff and a frozen,
non-carrying period cap. Its exact compiled SBF has now been installed by real
upgradeable-loader transactions on an offline Surfpool process, followed in
one account history by signed deposit and capped-release transactions and an
independent raw-RPC reconstruction.

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
- A stronger opt-in probe replaces `surfnet_writeProgram` with 252 signed
  loader writes and `DeployWithMaxDataLen`. It verifies the loader-created
  ProgramData owner, upgrade authority, and byte-for-byte SBF hash before
  executing the same B1 lifecycle.
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
- The program is deployed on **Solana devnet** at
  `BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM`. Its Program and ProgramData
  accounts were created by finalized loader transactions signed by a retained
  program keypair — not pre-placed as a fixture. Dumping the program back off
  the cluster reproduces the frozen artifact byte for byte. A public deposit of
  1,000,000,000 base units sits in the PDA-authorized vault under an on-chain
  monthly cap of 4,166,666 and a cliff span of exactly 63,072,000 seconds, and a
  pre-cliff release is refused with `CliffActive`. See
  `evidence/B1_PUBLIC_DEVNET_VALIDATION_2026-08-16.json`, and reproduce it
  yourself below.
- The deployed program is a **verified build**. A clean clone of this repository
  at `bc89d5a`, rebuilt inside the pinned container image
  `solanafoundation/solana-verifiable-build:3.1.10`, produces an artifact whose
  SHA-256 is byte-identical to the released one, and `solana-verify` reports
  `Program hash matches` against the devnet program. Four independent sources
  now yield the same bytes: a developer host, the GitHub CI runner, the pinned
  container, and the deployed program dumped back off the cluster.

## Reproduce the devnet receipt yourself

You do not need anything from us. Two public addresses are the only inputs, and
nothing on this path consumes a file we supply.

### The one-command version

With Docker and [`solana-verify`](https://github.com/solana-foundation/solana-verifiable-build)
installed, this clones the repository itself, rebuilds inside a pinned
container, and compares the result against the deployed program:

~~~sh
solana-verify verify-from-repo \
  https://github.com/magicknight/k4v-research-funding-vaults \
  --program-id BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM \
  --commit-hash bc89d5a45ac47ce400eb0f41b8be5b8440c04756 \
  --library-name beneficiary_vault \
  --base-image solanafoundation/solana-verifiable-build:3.1.10 \
  -u https://api.devnet.solana.com
~~~

Expected: `Program hash matches ✅`, with both hashes reading
`b3644045c7d949c7e90d11f41a0fa130e698136b88da07d8dfebb512fa9cf8d7`.

The `--base-image` pin is required, not cosmetic: the default image ships a
cargo that cannot parse dependencies requiring `edition2024`, and the build
fails outright without it.

It will then ask whether to upload verification data on-chain. Answer `n` — you
are checking this program, not publishing to it.

### The on-chain verification record

The program authority has published a verification PDA at
`HTJcacoyd5j1EzRaJBoLHh3HmXQsUzfw8UvzeBvvZxiV`, readable by anyone:

~~~sh
solana-verify get-program-pda \
  --program-id BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM \
  --signer 6by73aSGoWtvtfy6pn49v4yL3ecAQSFh3FZgcWLKwgfE \
  -u https://api.devnet.solana.com
~~~

It records the repository URL, the commit hash, and the build arguments —
including the `--base-image` pin, without which the build fails. So the record
carries everything needed to re-run the check, and needs no account, no explorer
and no contact with us.

Two honest limits. First, the uploader is the program's own authority, so the
record is an **assertion by the author**, not a third-party attestation; the
checking still comes from you re-running the build. Second, this does **not**
produce a "verified" badge on Solana Explorer: that badge is driven by the
OtterSec status API, which is mainnet-only and reports this devnet program as
`is_verified: false`. The same mechanism would produce the badge on mainnet.

### The step-by-step version

~~~sh
# 1. Are the installed bytes the ones this repository builds?
solana program dump BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM ondevnet.so \
  --url https://api.devnet.solana.com
sha256sum ondevnet.so
# expect d6a38fe400766267f0435ba776bae871d8db8ecac032cfc7c5771f9e1dad0312

# 2. Does the custody state hold up on its own terms?
PYTHONPATH=src python3 src/beneficiary_vault_rpc_exporter.py \
  --rpc-url https://api.devnet.solana.com \
  --program-id BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM \
  --vault-state GZAPjDUxETFYvCFYeJc33sdSxRSozBkJs68hqviQzyze > snapshot.json
PYTHONPATH=src python3 src/beneficiary_vault_verifier.py snapshot.json
~~~

Acceptance: `valid=true`, empty `reasons`, `token_surplus_amount=0`,
`expected_monthly_cap=4166666`, `released_total=0`, vault balance equal to the
deposit, and `cliff_end_ts - genesis_ts = 63072000`.

**Reports of any kind are welcome, including negative ones.** Open an issue
using the *Reproduction report* template. A run that disagrees with the numbers
above is more useful to us than a run that agrees, and it will be recorded as
found rather than quietly fixed.

Devnet is documented as subject to ledger resets. If the accounts have been
wiped, that is a property of devnet, not a retraction — the signatures, slots
and hashes are frozen in the evidence file.

## What is not established

- **No unrelated third party has yet reproduced the devnet receipt.** The path
  above is published and needs no permission, but publishing a method is not the
  same as someone having run it.
- The code has not received an independent security audit.
- No mainnet or production deployment exists, and no production key ceremony has
  been performed. The devnet upgrade authority is a retained test key.
- The post-cliff release has never executed on a public cluster and cannot
  before 2028-08-15: the 730-day minimum cliff is a locked invariant and a
  public cluster has no time control. Exact-cap release remains local,
  time-controlled evidence from the Surfpool real-loader probe. No
  shortened-cliff build exists, and none will be published to stand in for it.
- Surfpool is an offline local runtime. Its local transaction signatures have no
  explorer value; only the devnet signatures do.
- B1 uses fixed 30-day periods and the initial deposit as its cap basis. It is
  not yet semantically identical to calendar-month/year-start accounting, and no
  real month boundary has been crossed on any cluster.
- The purpose-bound treasury vault, IRB/oracle, LP, fee routing and multisig are
  specified but not implemented or deployed.
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

The real-loader probe additionally requires an offline Surfpool 1.5.0 process
on `127.0.0.1:18999`. It creates no key files and rejects execution unless the
explicit local send gate is present:

~~~sh
NO_DNA=1 surfpool start --ci --daemon --offline --no-deploy \
  --port 18999 --ws-port 19000 --airdrop-amount 0 --db :memory:
K4V_SURFPOOL_REAL_LOADER=1 K4V_LOCAL_REAL_LOADER_SEND_CONFIRMED=1 \
  cargo run --locked --package beneficiary-vault \
  --example b1_real_loader_probe
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
- **Decisive probe:** repeat the now-working real-loader transaction/RPC
  receipt on public devnet with a newly declared program address whose signer
  exists, then obtain an independent preflight.
- **Reliable core:** this executable covenant, receipt format, threat model,
  B1 source and local SBF tests, fixed-supply probe, and recorded local evidence.

See [ROADMAP.md](docs/ROADMAP.md) for the proposed five-milestone public-good
program and [COMMERCIAL_DISCLOSURE.md](docs/COMMERCIAL_DISCLOSURE.md) for the
K4V conflict boundary. The B1 program contract is specified in
[BENEFICIARY_VAULT_B1.md](spec/BENEFICIARY_VAULT_B1.md).

## License

Licensed at your option under either the MIT License or Apache License 2.0.
