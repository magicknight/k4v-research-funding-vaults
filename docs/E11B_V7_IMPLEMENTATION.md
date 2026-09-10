# E-11B: isolated v7 implementation and acceptance boundary

Status: IMPLEMENTED SOURCE / ACCEPTANCE PENDING. This is not a successful SBF,
Agave or production receipt. Inspect the exact-head E11B workflow, not this file,
for actual compilation and runtime outcomes. No acceptance is inferred from a
queued workflow or a successful source-generation step.

## Build representation

`python3 tools/materialize_e11b_v7.py` produces an independent v7 workspace from
exact, blob-hash-checked frozen v6 sources. The checked-in generator and tests
are the v7 source representation. Generated source is retained as a CI artifact
with a per-file manifest. V6, its lock, code identity and historical evidence
are not rewritten. The v7 program ID is derived from the PUBLIC INSECURE test
seed `[89;32]`, for local tests only; never use it for deployment or real funds.
The inherited financial identity preimage retains its v6 financial-profile tag;
new program ID, all PDA namespaces and account discriminators separate v7.

`prepare_policy` validates the complete config and future T0, revocation of mint
and freeze authorities, and actor/mint/program/config identity, then creates a
content-addressed preparation. It grants no custody or withdrawal authority.
There is no update, close, expiry-delete or re-create operation for preparations.
Unopened preparation rent therefore remains an explicit cost.

`open_prepared_policy` takes an eight-byte instruction and requires the creator,
Founder, Treasury and three configured recovery keys. Preparation owner, account
discriminator, canonical PDA/bump, program, all bound actors, content identity,
current mint facts and future T0 are checked again. The preparation is read-only;
a successful policy initialization prevents replay. Duplicate human control is
not ruled out by distinct keys, and no independent governance is claimed.

Default builds refuse both admission operations. The original `open_policy`
entry point does not exist in v7. Financial handlers are inherited in a new
namespace without changing constants, balances, recovery timing or quota rules.

## Evidence required

The E11B workflow compiles default-disabled and TEST_ONLY SBF, rejects stack
and compiler diagnostics, generates ABI through Anchor's compiler, measures
all instructions with distinct fee payer and optional accounts, compares the
independent JS identity vector to compiled Rust, runs seven new adversarial
bootstrap cases plus inherited financial/recovery cases, and runs actual local
Agave with six distinct keys plus a seventh fee payer. It captures a single-bank
raw account snapshot linking preparation, policy, mint, both vaults and code;
a separate Python decoder checks those bytes. All stages must pass on the exact
reviewed commit before engineering acceptance.

Inherited fixture adaptation is explicit: ordinary financial tests use two
separately signed transactions, not a new oversized transaction. The old v6
identity-squatting test is adapted to preparation/consent. Original v6 tests
remain unchanged. Its original oversize counterexample remains valid for v6.

Runtime acceptance is natural-clock bootstrap only. The financial tests' 90/180-day
recovery/withdrawal evidence uses controlled LiteSVM time and MUST NOT be called
natural long-duration Agave execution. External human review, actual production
parameters/controllers, new public deployment, launch authority and cash remain
OPEN. Feedback/outreach remains paused. No public-chain transaction is sent.
