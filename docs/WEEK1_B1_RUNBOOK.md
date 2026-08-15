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
`d6a38fe400766267f0435ba776bae871d8db8ecac032cfc7c5771f9e1dad0312`,
252 write transactions, expected `CliffActive`, exact-cap release, and an
independent RPC verifier `PASS`.

The earlier frozen hash `1a8b331a0c67368de6f2a67c34133b591021260f4de00246c2f0fa05cc04c9b5`
belongs to the previous declared address, whose keypair was deliberately
destroyed. It is preserved in `evidence/` as history and is not reproducible
from the current source; see the bridge check below.

## Public-cluster bridge

`b1_public_devnet_probe` installs the same SBF on a public cluster through the
upgradeable loader at an address whose signer is retained, so the Program
account is created by the loader rather than pre-placed as a fixture.

Three refusals are built into the probe rather than left to operator care:

- it aborts unless the connected cluster's genesis hash equals the one declared
  in `K4V_EXPECTED_GENESIS`;
- it refuses the mainnet genesis hash unconditionally, whatever is declared;
- it never broadcasts a release transaction.

The third is not a policy choice. `MIN_CLIFF_SECONDS` is 730 days and a public
cluster has no time control, so the post-cliff release is not reachable there.
Deploying a shortened-cliff build to demonstrate one would be a different
program under a different covenant, and is deliberately not done. What a public
run establishes instead is that the on-chain `monthly_cap` equals `4,166,666`,
the stored cliff length equals the frozen minimum, custody of the full deposit
moved into the PDA-authorized account, and a pre-cliff release is refused with
`CliffActive`. The exact-cap release remains local, time-controlled evidence
from the Surfpool real-loader probe above.

Because the declared address is compiled into the program, a retained signer
requires a new `declare_id!` and therefore a new artifact hash. The bridge
between two frozen hashes is made checkable rather than asserted:

~~~sh
python3 tools/verify_declare_id_delta.py \
  --old old.so --old-id <OLD_BASE58> \
  --new target/deploy/beneficiary_vault.so --new-id <NEW_BASE58>
~~~

It passes only when the two builds have equal length and every differing byte
belongs to a whole copy of the declared address.

Rehearse on a loopback surfnet first, using the real keypair files and the real
declared address, so that only the cluster differs:

~~~sh
solana airdrop 5 "$(solana-keygen pubkey ~/.config/k4v/devnet/payer.json)" \
  --url http://127.0.0.1:18999
K4V_DEVNET_PROBE=1 \
K4V_DEVNET_RPC=http://127.0.0.1:18999 \
K4V_EXPECTED_GENESIS=<surfnet genesis hash> \
K4V_EXPECTED_SBF_SHA256=<frozen hash> \
K4V_PROGRAM_KEYPAIR=~/.config/k4v/devnet/program-b1-devnet.json \
K4V_UPGRADE_AUTHORITY_KEYPAIR=~/.config/k4v/devnet/upgrade-authority.json \
K4V_PAYER_KEYPAIR=~/.config/k4v/devnet/payer.json \
K4V_BENEFICIARY_KEYPAIR=~/.config/k4v/devnet/beneficiary.json \
K4V_DEVNET_STAGE_CONFIRM=LOADER_CREATE_BUFFER,LOADER_WRITE_CHUNKS,LOADER_DEPLOY,SETUP,DEPOSIT \
NO_DNA=1 cargo run --locked --package beneficiary-vault \
  --example b1_public_devnet_probe
~~~

The receipt reports `cluster: local-rehearsal` for a loopback run and
`cluster: public` otherwise, so a rehearsal cannot be mistaken for a public
result. Keypair files must live outside every Git repository; the probe walks
each path's ancestors and refuses if it finds a `.git` directory. Every stage
must be named in `K4V_DEVNET_STAGE_CONFIRM`, so a blanket pre-authorization
flag does not exist. For a public run, replace the RPC URL with the cluster
endpoint and the genesis hash with that cluster's.

## Reproducing a public receipt without trusting the publisher

Given only the program id and the vault-state address from a published receipt,
a third party can rebuild the whole conclusion from public accounts:

~~~sh
PYTHONPATH=src python3 src/beneficiary_vault_rpc_exporter.py \
  --rpc-url https://api.devnet.solana.com \
  --program-id <PROGRAM_ID> --vault-state <VAULT_STATE> > snapshot.json
PYTHONPATH=src python3 src/beneficiary_vault_verifier.py snapshot.json
~~~

Acceptance is `valid=true`, empty `reasons`, `token_surplus_amount=0`,
`expected_monthly_cap=4166666`, `released_total=0`, a vault balance equal to
the deposit, and `cliff_end_ts - genesis_ts = 63072000`. Nothing on that path
consumes a file supplied by the publisher. The installed bytes can be fetched
and hashed independently with `solana program dump` followed by `sha256sum`.

## Evidence boundary

The local `.so` hash identifies one build, but ordinary Solana builds are not
assumed reproducible across machines. A production claim requires a container
verified build, an on-chain executable hash comparison, and disclosed loader
authority. LiteSVM is the appropriate fast Week 1 mechanism test. Both
deterministic injected-state RPC reconstruction and transaction-produced local
RPC state now pass. The stronger local probe also passes with a real loader
instruction path and byte-exact ProgramData. In that probe the Program account
is still a local fixture, because the historical signer of its declared address
was destroyed.

The public-cluster probe closes that node differently: at the new declared
address the signer exists, so the loader creates the Program account itself. A
full loopback rehearsal of that path passes end to end. Public-cluster
execution, a container-verified build, an on-chain executable hash comparison
by an unrelated party, and independent security review remain open.
