# Initialization security repair — 2026-09-09

Status: IMPLEMENTED / LOCAL_VERIFICATION_PASS / NOT_PUBLICLY_DEPLOYED.

Local evidence: [66 Rust/SBF tests, Python and client checks](../evidence/INITIALIZATION_REPAIR_LOCAL_VALIDATION_2026-09-09.json).
The exact-head CI and merge result are recorded on [PR #15](https://github.com/magicknight/k4v-research-funding-vaults/pull/15); merge requires all seven jobs to pass.

The Founder, currently the sole project owner, explicitly authorized website
merge/deployment and vulnerability repair on 2026-09-09. That authorization
covers these code changes and their merge after verification. It does not create
additional people, independent reviewers, treasury signers or audit evidence.
No public-chain transaction, upgrade, token issuance or capital collection is
part of this repair.

## Changes and acceptance

| Finding | Repair | Required regression |
|---|---|---|
| K4V-01 | Bind B2 policy identity to signing creator, mint, program and specification | Another signer cannot initialize the published victim policy/market or attach a victim vault; their own namespace remains separate; honest creation and deposit succeed |
| K4V-02 | Require B1 beneficiary signature at initialization | Unsigned 240-unit dust attack fails without leaving accounts or moving tokens; the intended deposit succeeds at exactly those addresses; one owner can sign both roles |
| K4V-03 | Both programs require classic SPL mint authority and freeze authority to be absent at deposit | All four authority combinations for B1 and both B2 vault kinds; rejection leaves principal, vault accounts and B2 vault count unchanged; revocation through SPL enables deposit; subsequent freeze and authority reactivation fail |

B2 keeps the eight-instruction surface and account layouts. `open_policy`'s
first 32-byte argument is now the underlying **specification** hash. The stored
identity and PDA digest are computed as:

```
SHA256(UTF8("k4v-policy-authority-v1")
       || program_id[32] || creator[32] || mint[32] || policy_spec_hash[32])
```

All keys are raw bytes, not base58 text. The domain and fixed input widths
make the encoding unambiguous. The account constraints derive this identity
using the actual required creator signer, so a copy of the public specification
or digest cannot reserve somebody else's namespace. The Rust and JavaScript
implementations share `spec/POLICY_IDENTITY_VECTOR_v1.json` for interoperability
checks. A new declaration must publish both the underlying specification hash
and its bound digest, plus the authoritative program, creator and mint keys.

B1 keeps its existing seeds and instruction argument layout, but the beneficiary
account is now a signer. Consent can come from the same key as the depositor or
from a valid PDA signer through CPI; a second human is not required by this code.
Classic SPL Token's revoked mint/freeze authorities cannot be reinstated by their
former controller. No Token-2022 compatibility claim is made.

## Artifact and compatibility boundary

- `spec/R3_TEST_ONLY_CANDIDATE_v2.json` pins the newly built B1/B2 files. CI must
  match those exact SHA-256 values, run SBF attack regressions, and exercise the
  existing offline RPC and real-loader paths. Loader chunk count is checked
  against the pinned B1 byte length and the existing 900-byte chunk size.
- `R3_TEST_ONLY_CANDIDATE_v1.json`, prior signed receipts, the old SBF hashes and
  the original exploit record remain historical evidence. PR #14 is the
  vulnerable baseline; the repair branch includes it and changes the tests to
  require rejection on the new programs.
- Previously deployed devnet programs/accounts are untouched. These changes
  protect initialization under new bytes; they neither reclaim old squatted
  accounts nor unfreeze old locked tokens. Existing release logic is unchanged.
- The old `b2_devnet_probe.mjs` and `squads_authority_probe.mjs` are historical
  clients for the old deployment. Use their matching historical revision; they
  are not launch clients for this candidate. Current local R3 clients derive
  the bound identity and revoke mint authorities before depositing.
- A structural RPC/account verifier is not a substitute for checking the exact
  program hash, intended keys, amount and published bound identity.

## Remaining work

K4V-04 shared-window allocation can still starve the beneficiary. Its adversarial
six-period tests remain and demonstrate that unresolved behavior. K4V-05 oracle
recovery, K4V-06 policy-controller recovery and K4V-07 production upgrade control
remain separate design/review items. None is silently classified as fixed.

The accepted 180-day Founder design is a separate engineering change. This
repair retains the historical 730-day test cliff, release rates, shared ceiling,
notice periods and recovery behavior so that security evidence is not mixed
with a change in financial rights. The v2 test candidate retains the historical
26-open-parameter test binding; it is not the production candidate and does not
supersede the internal 180-day design or its 25 open production values.

Author/AI execution is not independent security review. Passing these checks
will establish these bounded repairs on the tested bytes, not production
readiness or that every possible vulnerability has been found.
