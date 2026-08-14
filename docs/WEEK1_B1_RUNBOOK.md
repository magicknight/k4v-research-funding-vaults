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
deployment authority, and public deployment are not part of Week 1 acceptance.
Read-only RPC reconstruction and the local transaction-produced RPC probe are
public B1-beta evidence, not production-readiness claims.

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

## Read-only RPC bridge

The exporter requires only an RPC URL, program id, and vault-state PDA. It
fetches the state, derived token account, and Clock sysvar with `getAccountInfo`,
validates their owners and raw binary layouts, and emits the existing snapshot
schema:

~~~sh
PYTHONPATH=src python3 src/beneficiary_vault_rpc_exporter.py \
  --rpc-url http://127.0.0.1:8899 \
  --program-id PROGRAM_ID \
  --vault-state VAULT_STATE
~~~

The opt-in integration test uses an offline Surfpool 1.5.0 process and no
wallet, deployment, signature, or transaction:

~~~sh
NO_DNA=1 surfpool start --ci --offline --no-deploy --port 18999 --ws-port 19000
K4V_SURFPOOL_RPC=http://127.0.0.1:18999 PYTHONPATH=src \
  python3 -m unittest discover -s tests -p 'test_*.py' -v
~~~

Surfpool 1.5.0 accepts hex account data for `surfnet_setAccount` and compares
the time-travel argument as milliseconds while exposing Unix seconds in the
Clock sysvar. The test records those observed compatibility rules explicitly.

## Transaction-produced RPC bridge

The opt-in Rust probe keeps every signer in memory, rejects non-loopback RPC
URLs, loads the test SBF with Surfpool's local-only `surfnet_writeProgram`, and
then uses real signed transactions to create the mint and token accounts, mint
1,000,000,000 base units, deposit them into B1, and release the exact monthly
cap after a time jump. Every successful transaction is simulated before its
send gate. The pre-cliff release is simulation-only and must fail with
`CliffActive`.

Start the same offline surfnet and run:

~~~sh
K4V_SURFPOOL_RPC=http://127.0.0.1:18999 NO_DNA=1 \
  cargo run --locked --package beneficiary-vault \
  --example b1_rpc_transaction_probe
~~~

At each `AWAITING_SEND` line, inspect the preceding simulation and enter only
the displayed `SEND_<STAGE>` command. For isolated CI, the repository workflow
sets `K4V_LOCAL_TRANSACTION_SEND_CONFIRMED=1` on a loopback Surfpool process;
do not copy that setting into a public-cluster or wallet workflow.

After the probe prints `RESULT_JSON`, pass its `vault_state` to the read-only
exporter above. Acceptance requires `valid=true`, no reasons, zero surplus,
`released_total=4,166,666`, vault balance `995,833,334`, and beneficiary
balance `4,166,666`.

## Real-loader transaction bridge

The stronger probe removes `surfnet_writeProgram` from the execution path. The
historical keypair for the committed test Program ID was deliberately
destroyed, so test setup first places a 36-byte uninitialized loader-owned
Program account at that address. This fixture is public and non-secret. All
remaining installation work is performed by signed upgradeable-loader
transactions: create the buffer, write all 252 SBF chunks, and invoke
`DeployWithMaxDataLen` to create ProgramData and make the Program executable.

Start a fresh ephemeral Surfpool process:

~~~sh
NO_DNA=1 surfpool start --ci --daemon --offline --no-deploy \
  --port 18999 --ws-port 19000 --airdrop-amount 0 --db :memory:
K4V_SURFPOOL_REAL_LOADER=1 NO_DNA=1 \
  cargo run --locked --package beneficiary-vault \
  --example b1_real_loader_probe
~~~

The probe stops at each send gate unless the exact displayed `SEND_<STAGE>`
text is entered. CI may set `K4V_LOCAL_REAL_LOADER_SEND_CONFIRMED=1` only on
the isolated loopback surfnet. Acceptance requires the declared Program ID,
the canonical ProgramData PDA, loader ownership, an exact SBF SHA-256 of
`1a8b331a0c67368de6f2a67c34133b591021260f4de00246c2f0fa05cc04c9b5`,
252 write transactions, expected `CliffActive`, exact-cap release, and an
independent RPC verifier `PASS`.

## Evidence boundary

The local `.so` hash identifies one build, but ordinary Solana builds are not
assumed reproducible across machines. A production claim requires a container
verified build, an on-chain executable hash comparison, and disclosed loader
authority. LiteSVM is the appropriate fast Week 1 mechanism test. Both
deterministic injected-state RPC reconstruction and transaction-produced local
RPC state now pass. The stronger local probe also passes with a real loader
instruction path and byte-exact ProgramData. The Program account itself is a
local fixture because its historical signer is unavailable. Public-cluster
deployment with a newly declared, retained program signer, a verified build,
and independent security review remain open.
