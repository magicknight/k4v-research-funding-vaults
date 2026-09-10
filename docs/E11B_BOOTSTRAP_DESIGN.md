# E-11B bootstrap design: immutable preparation and compact consent

Historical status at E-11A: design and offline signed-wire prototype only.
The implemented descendant is now [E-11B v7](E11B_ENGINEERING_ACCEPTANCE.md).
The proposal below is preserved as design provenance; it is not the live
implementation status. The v6 ABI, program bytes and E-10 evidence are unchanged.

The accepted v6 open_policy needs six signatures and 564 instruction bytes.
It serializes to 1,352 legacy bytes or 1,264 ideal-v0 bytes against a 1,232-byte
packet limit. The four-signature single-operator E-11A fixture is not a repair.

## Proposed two-transaction protocol

1. prepare_policy: the creator pays rent and signs a content-addressed
   preparation containing program identity, creator, mint, Founder, Treasury,
   oracle, spec hash and the complete configuration. It gives nobody policy,
   deposit, withdrawal or upgrade authority. Creation validates the configuration,
   future T0 and mint facts. There is no mutable buffer, update or close/recreate
   operation in this candidate.
2. open_prepared_policy: the creator, Founder, Treasury and three registered
   recovery actors sign one compact transaction referencing the preparation.
   The program verifies preparation owner/discriminator/PDA/content identity,
   all six required actor signatures and exact keys, current mint facts and
   now < T0; then initializes the policy PDA with that exact configuration.
   Preparation is read-only. Existing policy initialization prevents replay.
   No deposit/activation path accepts a preparation in place of a policy.

Clients independently recompute the preparation identity and review its contents
before signing. A changed field implies a different preparation and policy PDA.
The signature binds program and preparation address, and on-chain immutability
must bind that address to the reviewed contents. Transaction-size success alone
does not prove that on-chain half.

The preparer cannot spend the research tokens through preparation, replace
configuration after consent, omit a required signer, reuse a consent on another
program/mint/configuration, or move T0 under old signatures. An actor can decline
to sign; this remains a liveness dependency. Unopened preparations permanently
consume their creator's rent in this first design. A later rent-reclamation
extension would need a separate replay/cancellation design and is not assumed.

## Isolation and implementation acceptance

Implement only in a new test-only program/account namespace and independent
workspace. Default builds must continue to refuse policy creation. Preserve the
v6 source, locks, SBF hashes, raw receipts and six-signer oversize counterexample.

Required evidence before calling the repair complete:
- compiler-generated ABI and independent fixed identity vectors;
- actual serialized packet budgets for all instructions and distinct payer/
  signer layouts, with and without compute-budget instructions;
- signed runtime refusal of missing/substituted roles, forged/mutable/wrong-owner
  preparations, changed config/mint/program, past T0 and replay;
- actual local validator six-role prepare/open/deposit/arm/activate;
- unchanged funding, quota, withdrawal recovery and annual-accounting invariants;
- raw account verification binding preparation and resulting policy;
- separate evidence boundaries for controlled-clock tests, naturally advancing
  validator time, public deployment and accountable human review.

The offline probe uses a fresh dummy program identity and real ephemeral SDK
signatures only. It has no RPC, wallet persistence or transaction-send path.
It measures feasibility of the proposed wire layout; it is not a v7 contract.
