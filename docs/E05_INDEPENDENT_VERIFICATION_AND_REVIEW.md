# E-05 independent verification and review handoff

**Status: author-run engineering acceptance; external review remains open.**
This package connects the frozen E-04 v4 policy, recovery proposals, SPL custody
and the actual upgradeable loader in one reproducible local bank. It adds
read-only Python verification and an RPC exporter. It changes no on-chain
program source, production rights or official mint.

## What can now be checked

| Layer | Evidence and checks | Boundary |
|---|---|---|
| Policy and custody | Raw Borsh and SPL layouts, discriminators, owners, exact lengths, PDAs, bound initial identity, annual inputs, escrow principal, released amounts, reserved quotas and full declared supply | Active dual-pool, one-depositor, five-token-account graph; not an arbitrary portfolio explorer |
| Key recovery | All proposal tombstones from nonce 1 through the current sequence; replay the key/epoch changes, check successor, notice, pending link, report invalidation and withdrawal identity | At most 256 proposals in this review profile; historical transaction signatures and human independence are not proved |
| Loader | Program/ProgramData tags, canonical PDAs, executable flags, actual authorities, pinned executable bytes and zero allocation padding | Exactly the E-04 classic-loader artifacts; unknown binaries fail closed |
| Upgrade gate | Immutable gate code, target authority equal to its PDA, distinct committee, nonce/status/notice, locked buffer owner, exact code length and hash | A legal upgrade can change economics; a waiting period is not a semantic proof |
| RPC | Expected genesis hash, finalized `getMultipleAccounts`, all graph accounts and Clock in one response, Clock slot equal to response context, optional minimum slot | Trust in the chosen RPC server remains; no cryptographic chain proof or public-cluster deployment is claimed |

The Python verifier does not execute Rust, import Rust math or decode through
the generated IDL. Its arithmetic uses integer operations, and it reconstructs
the accounts directly. It reuses the existing pure-Python base58/PDA primitives.
“Independent” describes the implementation path; the author and assistant
produced both it and the tests. This is not an unrelated human security audit.

`src/launch_v4_verifier.py` can also check the frozen E-04 raw fixture, but
reports `program_bytes_verified=false` when loader accounts are absent.
`src/e05_verifier.py` requires the actual loader account graph and pins accepted
code bytes in its own reviewed source. A hash supplied in the input cannot
replace those pins.

## Combined local transaction rehearsal

The new Rust scenario is
`programs/launch-vault-v4/tests/support/e05.rs`. It uses the lifecycle test's
signing helpers in an explicit native-loader mode. That mode never calls
`add_program` or injects a mint, token account, policy, vault or proposal.
Both programs are installed with signed loader buffer writes and deployment
instructions. Signed SPL instructions create the mint/accounts, mint the test
supply and revoke mint authority; freeze authority starts absent. SOL airdrops
and controlled Clock/slots remain local fixture setup.

The supply is 1,000,000,000 whole test tokens at nine decimals. Founder and
Treasury receive exact 300,000,000 and 500,000,000 principal deposits. The
remaining 200,000,000 stays in the source account; this rehearsal does not model
LP issuance, token sales or every 12/8 downstream allocation.

| Checkpoint | What it demonstrates |
|---|---|
| Pending oracle recovery and upgrade | Separate committee keys; two recovery signatures accept a successor; two upgrade signatures bind a locked default-profile buffer |
| Notice minus one second | Recovery, upgrade and pre-cliff Founder release fail; the failed transactions preserve instruction-account data apart from independently advanced read-only Clock |
| Before/after oracle recovery | Existing Founder/Treasury releases remain recorded; original report becomes invalid without resetting any budget, escrow or Treasury approval |
| Before/after upgrade | Real loader CPI changes v4 test-profile ELF to its distinct default-profile ELF after the full notice; every supplied live policy, vault, token and approval byte is unchanged |
| Continued after upgrade | Both original withdrawal authorities continue bounded releases using the current oracle epoch |
| Controller notice and recovery | An absent controller key is recovered after a separate full notice; an old-controller proposal and replayed/cancelled proposals fail |
| Continued after controller recovery | Original beneficiary identities retain withdrawal authority; recovery does not grant that authority to the new controller |
| Annual boundary | A new frozen input interval changes annual accounting without erasing lifetime totals or carrying unused quotas forward |

The fixed example contains **12 checkpoints**, **1,290 submitted signed local
transactions**, **1,278 successful transactions**, and 12 intended refusals.
These counts exclude fixture SOL airdrops and sysvar changes. Final cumulative
releases are 600,000 Founder and 900,000 Treasury whole test tokens. The second
annual interval records 100,000 and 150,000 respectively; all five token
balances still sum to the original supply.

The gate's upgrade is intentionally a *changed-byte* test-profile-to-default
transition. The default build refuses new policies but retains existing-state
instruction semantics. Successful continued withdrawals establish compatibility
for this exact pair only. They do not establish arbitrary future upgrade
compatibility, migration safety, or that default admission makes the target
immutable.

## Reproduce and inspect

Prerequisites are those frozen in E-04: Rust 1.89.0, Solana CLI 3.1.10,
SBF tools v1.52, Python 3 and Node 20. No wallet or RPC endpoint is needed.
From a clean checkout of the E-05 merge commit:

```sh
bash tools/run_e05_local_reproduction.sh
```

The same command is the v4 GitHub Actions job. It builds four exact SBF files,
checks E-04 pins, lints, runs all 52 v4/gate Rust checks, regenerates both IDLs,
checks the identity codec and verifies freshly exported raw state. Native and
SBF targets are separate to prevent build output from overwriting an artifact
under test. `K4V_E05_SKIP_BUILD=1` can reuse existing artifacts, but the exact
hash gate is still mandatory. Fresh results are under `target/`.

To inspect the committed evidence without a Rust toolchain:

```sh
PYTHONPATH=src python3 src/e05_verifier.py examples/e05_rehearsal_bundle.json
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v4*.py' -v
```

`examples/e05_rehearsal_bundle.json` contains the full raw bytes as lossless
zlib/base64 blobs keyed by SHA-256. Repeated account states share a blob.
`tools/pack_e05_rehearsal.py` performs only packaging; the verifier expands and
rehashes every blob before decoding. The checked-in signatures identify a
local run, and future local runs use fresh in-memory keys and new signatures.
No private key is serialized. Reproduction compares the invariants and pinned
program bytes, not randomized account addresses or the bundle file's hash.

The 26 new Python checks cover byte tampering, missing or forged tombstones,
wrong generations, notice boundaries, deficit/delegation, budget mismatch,
mutable gate/bypass authority, substituted ELF/buffer bytes, malformed RPC
responses, wrong network, missing accounts, stale or mixed-bank slots, and
missing/reordered rehearsal checkpoints. RPC tests use responses derived from
the local runtime export and mocked transport. They are not a successful live
public-RPC run.

## Read-only RPC entry point

For a separately authorized and deployed test candidate, prepare a JSON map
of semantic account names to reviewed public addresses, including all
`change_1` ... `change_N` tombstones, `clock`, both Program/ProgramData pairs,
the gate and its pending buffer when applicable. There is deliberately no
automatic “official K4V address” discovery from a symbol or ticker.

```sh
PYTHONPATH=src python3 src/launch_v4_rpc_exporter.py \
  --rpc-url YOUR_REVIEWED_HTTPS_RPC_ENDPOINT \
  --addresses reviewed-account-addresses.json \
  --expected-genesis-hash EXPECTED_NETWORK_GENESIS_HASH \
  --min-context-slot MINIMUM_ACCEPTABLE_SLOT \
  --output rpc-observation.json
```

All addresses are independently reconciled against state and PDAs. A change
that makes the address map incomplete fails verification; update the map after
review and repeat the complete single-bank read. A snapshot from an untrusted
RPC server can be fabricated coherently. Use a trusted endpoint or separately
cross-check nodes and finalized transaction history for deployment acceptance.
The exporter does not label structural checks as chain authentication.

## Handoff and remaining work

The structured scope, pins and acceptance checklist are in
`spec/E05_REVIEW_SCOPE_v1.json`; the local result is in
`evidence/E05_LOCAL_VALIDATION_2026-09-09.json`.

An external reviewer should record their name, organization if relevant,
reviewed commit and tree, toolchain, independently reproduced artifact hashes,
reproduction outcome, severity-ranked findings, fixes reviewed, and explicit
scope exclusions. None of those human-acceptance fields is prefilled with an
AI or CI result. The package is ready to hand off; no invitation has been sent.

Still open:

- Founder/Treasury lost withdrawal-key custody and recovery, including the
  one-person/multiple-role failure case;
- production recovery and upgrade mode, committee custody and identities,
  recovery after two upgrade-committee keys are lost, and incident response;
- independent human review and authenticated deployment/transaction history;
- real annual IRB sources, calendar, later-year governance, final T0 and
  production configuration;
- genuine demand feedback, budget/quotes and issuance readiness.

The next technical tranche can design and test beneficiary withdrawal-key
recovery explicitly, without silently granting that right to the controller
recovery path. Choosing production rights remains a separate owner decision.
