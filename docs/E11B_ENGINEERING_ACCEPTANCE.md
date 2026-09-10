# E-11B: compact bootstrap engineering acceptance

This is a separate **TEST_ONLY** `launch-vault-v7` candidate. It does not migrate
or upgrade v6, change any official 180-day entitlement, select production
parameters, deploy to a public chain, or constitute independent human review.

## Exact acceptance evidence

The initial full acceptance at `bd7e34333ef422d73facdaf24f5363f63c575194`
passed in [run 34450900533](https://github.com/magicknight/k4v-research-funding-vaults/actions/runs/34450900533).
It checked out the PR merge `6c251a3929e24abdae2d6df5322a3a7dddda40f3`.
The accepted generated source was published byte-for-byte at
`b6cdeb168c06d49c2a2f2e506efe762ca88b10f8`; file hashes and original public
local-test receipts are in `evidence/e11b/source-publication.json` and its archive.
The E11B workflow on any later commit must rebuild and replay this exact source;
a successful earlier run is not a claim that an untested descendant passed.

The initial run passed 72 Rust tests, 21 JavaScript tests and six additional
Python test methods. It verified 142 bounded-submission probes (44 accepted,
98 expected refusals), 669 native-loader financial transactions (658 accepted,
11 expected refusals), 11 financial/account checkpoints and 55 loopback replay
reads. These probes and transactions are not extra independent test methods.

Actual Agave 3.1.10 accepted 15 client transactions to finality, with one raw
preparation verification and four raw policy/account checkpoints. Six separate
role keys and a separate seventh fee-payer signature initialized the policy.
The delayed recovery proposal was accepted 16 seconds after its lower bound
and retained the full 7,776,000-second notice. Six expected refusal paths were
checked, including bootstrap replay, pre-cliff release and both expiry classes.

Pinned SBF identities (CLI 3.1.10, platform tools v1.52):

| Profile | Bytes | SHA-256 |
|---|---:|---|
| Default, admission disabled | 539120 | `2d1b6268b8305e57c2b4a9b830873ee5e7488091b7aabec9b7f9e94bc2618d9c` |
| TEST_ONLY | 553624 | `df1011597eeda9e2013d648d3f3be8e840a4bcab8a0e3143e2a9c161cd0f3b67` |

The program ID in the build metadata is a TEST_ONLY program, not an official
K4V mint or production address.

## The repair

The former six-signer initialization could not fit the 1,232-byte packet.
The new program has no `open_policy(config, ...)` entry. It exposes:

1. `prepare_policy(config, spec_hash, identity)`: the creator signs and funds a
   725-byte content-addressed preparation. The complete configuration and actor
   identities are bound. No policy, custody or withdrawal rights exist yet.
2. `open_prepared_policy()`: creator, Founder, Treasury and three designated
   recovery roles sign compact consent. The preparation is read-only. The
   program verifies owner, discriminator, PDA, identity, exact actor keys,
   current mint facts and future T0, then creates the original financial state.

There is no update, close or close/recreate instruction for a preparation.
A changed configuration needs different preparation/policy addresses, so old
signatures cannot be reused. Unused preparations retain rent permanently in
this version. An actor's refusal to sign remains a liveness dependency.

The default build rejects preparation and confirmation, including otherwise
valid prepared state supplied in an explicit fault-injection test. Only the
`test-profile` build can open policies. Production values are not invented.

## Evidence boundaries

| Layer | What is established in its scope | Exclusion |
|---|---|---|
| Pinned builds and compiler ABI | Disabled/test SBF; 19 instructions and 7 account layouts | Public deployment or production review |
| Signed runtime tests | Missing/substituted signatures; forged preparation; T0; authorities; replay; prefunding; inherited financial/recovery rules | Naturally elapsed 90/180-day operation |
| Compiled SDK wire tests | 933-byte preparation, 828-byte six-role consent; 38 layouts across 19 instructions and payer variants; all fit | Semantic execution of every synthetic size-only sample |
| Native-loader financial rehearsal | Signed loader/mint/SPL transactions, delayed admissions, successful recovery and continued withdrawals into year two | Natural Clock: this layer explicitly controls Clock |
| Independent raw decoder | Preparation/config/policy binding, exact code/loader authority, history and one final RPC response | External chain authentication or human audit |
| Actual loopback Agave | Natural Clock, six distinct role keys, separate payer, finalized prepare/open/deposit/arm/activate and raw account verification | Six independent people, public deployment or successful natural 90-day recovery |

The actual-node opening was **924 bytes with the seventh fee-payer signature**;
828 is the six-signature payer-shared layout. With a compute-budget instruction,
the separate-payer layout is 964 bytes. All three numbers must stay distinct.

All fixture keys are controlled by one operator. This is key separation, not
independent governance. The archived transcript can be signature-checked again;
that does not make archived RPC finality a fresh observation of a public chain.

## Reproduction

Use a clean checkout of the exact recorded commit, Node 20, Python 3.12, Rust,
Agave/Solana 3.1.10 and pinned v1.52 platform tools. Install JavaScript packages
with `npm ci --ignore-scripts`, then run:

```sh
bash tools/run_e11b_acceptance.sh
```

The runner makes only loopback chain calls. Dependency/toolchain downloads can
use the network. The validator uses temporary local keys and airdrops. Program
bytes start in genesis; a genuine signed loader transaction revokes the temporary
upgrade authority before policy creation. This is not described as loader
program deployment; native-loader deployment is a separate financial rehearsal.

The command requires the committed build identities and must fail on drift.
Compiler ABI generation must leave committed input unchanged. The original
checksum manifest is preserved as `evidence/e11b/parent-SHA256SUMS`; only the three
explicitly superseded navigation documents change in the current legacy list.
`E11B_SHA256SUMS` covers the new source and handoff. Old programs and receipts
remain byte-identical.

## Still open

Natural-Clock execution after full long notice/cliff, any new public-chain
rollout, named human reproduction/security review, production annual data and
release parameters, actual signer/upgrade choices, issuer/legal/payer decisions
and funding remain separate open gates. No outreach or interview was sent.
There is no official mint, launch date or new fundraising cash.
