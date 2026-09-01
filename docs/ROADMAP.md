# Public-good engineering roadmap

This is the public engineering subset of the project. It is not a K4V launch
plan, an offer, a legal route or mainnet authorization. Commercial K4V remains
only a disclosed possible case study.

## Frontier state

`TARGET`:

> A reusable, independently reproducible Solana capability for purpose-bound
> research-funding vaults whose current program hash, state, authorities and
> negative behavior can be checked without trusting the deployer.

`CURRENT_NODE`:

```text
B1+B2 public-devnet receipts
  -> R3 local full-scale transaction/RPC/Squads integration (PASS)
  -> blank-host same-user agent reproduction + hash-gate repair (PASS, bounded)
  -> architecture findings + adversarial re-tests (CURRENT)
  -> unrelated human reproduction + accountable security review
  -> tagged integration and incident guide
```

`CHAMPION`: preserve the USD 30,000 / 16-week public-good program, now starting
from the stronger pre-funding B1/B2 evidence rather than from a paper design.

`ALTERNATIVE`: a narrow USD 10,000 / 8-week package that freezes the current
receipt/verifier interface and excludes production oracle, purpose-policy and
independent-review claims it cannot fund.

`PROBE`: one integrated test mint at full economic scale, followed by a
one-command reproduction issue that an unrelated person can run without any
private applicant artifact.

`FLOORS`: public devnet is not production; author-run decoders and CI are not
independent review; probe-generated multisig members establish key-loss
resilience rather than independent governance; an upgradeable program is not
called immutable.

## Strongest result now

- B1 is deployed on public devnet, has a verified build and can be reconstructed
  from public accounts.
- B2 is deployed on public devnet at
  `2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w` with the locally tested bytes.
- A Squads 2-of-3 vault PDA opened the policy, co-signed two deposits, approved a
  future need and proposed an oracle rotation.
- Two surviving members replaced a declared-lost member; the post-replacement
  key set approved another need while the multisig address, vault PDA and B2's
  stored authority remained unchanged.
- Four intended refusals landed on chain. A read-only decoder checked thirteen
  state/account properties from devnet.
- The first purpose-release window opens 2026-09-19 if devnet retains the
  accounts and a fresh volume report is supplied.
- R3 locally creates and reconciles the full `1e18` raw supply through signed
  transactions, deposits the aligned 80%, replaces a Squads member, and executes
  a post-replacement purpose release under the unchanged vault PDA. The
  read-only verifier passes nineteen RPC/PDA/membership/transaction checks.
- A public-only xv4 clean-room run reproduced every functional R3 result and
  exposed a missing equality check between the candidate's frozen SBF hash and
  the freshly compiled file. The comment-only source-span drift and runner were
  repaired; a second public-only clone of published commit `173c856` passed with
  `sbf_byte_reproducible=true` and exact expected/observed `081b6c...` hashes.
- Claude Opus completed an `INDEPENDENT_MODEL_AI_PREAUDIT`: no critical/high or
  confirmed theft path; seven medium design/DoS/governance findings; mainnet
  verdict `NOT_READY`. Human-accountable review remains open.

This remains author-produced public-devnet evidence. No unrelated party has
reproduced or reviewed B2, no production parameters or authority ceremony exist,
and no release has succeeded on a public cluster.

## Champion: USD 30,000 / 16 weeks

| Milestone | Weeks | Amount | Acceptance artifact | Pre-funding state |
|---|---:|---:|---|---|
| M1 — Covenant and threat model | 1–3 | USD 3,000 | Frozen integer spec, receipt schema, bypass taxonomy, CI | Strong draft and executable reference evidence exist |
| M2 — Solana vault implementation | 3–8 | USD 9,000 | Beneficiary and purpose vaults on localnet/devnet | B1/B2 public-devnet beta evidence exists; production integration remains open |
| M3 — Adversarial and full-scale harness | 7–11 | USD 7,000 | Authority, oracle, scale, OTC, collateral, threshold and signer-loss tests | `PASS — AUTHOR-RUN LOCAL`; R3 full-scale transaction/RPC/Squads integration and 12-case coverage pass; unrelated acceptance remains R4 |
| M4 — Independent reproduction/security preflight | 11–14 | USD 8,000 | Clean-room reproduction and independent report with repairs | Operational AI-run evidence + AI pre-audit exist; architecture decisions and human-accountable review remain open |
| M5 — Release and integration guide | 14–16 | USD 3,000 | Tagged source, checksums, deployment, governance and incident guide | Open |

Amounts are planning inputs, not vendor quotes. Pre-funding work is evidence that
reduces technical uncertainty; it is not a grant milestone invoice or an
independent acceptance. A funded budget must name the owner of each milestone
and separate applicant labor, external engineering and independent review.

## Closed construction: one mint, full scale

R3 removed the scaled-fixture ambiguity with the following integrated result:

1. the public test binding selects 9 decimals for local evidence only;
2. signed transactions create exactly 1,000,000,000 whole test tokens;
3. 30/50/12/8 and both full B2 deposits reconcile on that mint;
4. mint and freeze authorities end revoked;
5. all twelve negative cases have named coverage;
6. a real Squads 2-of-3 replaces one member and releases with the new key set;
7. a read-only verifier reconstructs RPC state, PDAs, membership and signatures.

The executable acceptance contract, negative cases, numeric representation
floor and required receipt are frozen in
[`FULL_SCALE_ONE_MINT_R3.md`](../spec/FULL_SCALE_ONE_MINT_R3.md).

If decimals are 9, the two aligned deposits are `300,000,000,000,000,000` and
`500,000,000,000,000,000` base units. If another decimal count is selected, the
test derives exact equivalents rather than copying these literals.

The next decisive construction is the R4 repair transaction: classify and test
the seven medium pre-audit mechanisms, repair the accepted defects, then obtain
one unrelated human/accountable rerun. The same-user agent and Claude results
advance the evidence without being relabelled as human independence.

## Independence without a personal network

Mechanical reproduction and adversarial review are different work.

### Open reproduction interface

- one clean-clone command from public inputs to a machine-readable verdict:
  `bash tools/run_r3_local_reproduction.sh`;
- pinned compiler, container and RPC assumptions;
- expected values and expected failures;
- a public issue template recording fork commit, environment, transcript,
  discrepancies, conflicts and compensation;
- no applicant-supplied snapshot on the acceptance path.

The interface passed an author-run end-to-end execution on 2026-08-28. That is
`PROMISING_ROUTE_DEVELOPMENT`, not independent acceptance. The R4 gate requires
an unrelated report through the dedicated R3 clean-room issue template.

Anyone may run that interface. A report is independent only when its controller
is unrelated to the author and the relationship is disclosed.

### Procured security preflight

The funded champion obtains written scopes and quotes through open procurement;
it does not assume the Founder already knows a reviewer. Reviewers are paid for
documented work and conclusions, never for a positive verdict. Acceptance
requires report, repair window and re-test.

If nobody volunteers and no review is funded, this milestone remains `OPEN`.
Silence is not approval and does not authorize production.

## Upgrade-governance statement

The current program hash enforces the documented behavior. While an upgrade
authority exists, that authority can replace the program and the project must
disclose who controls it. Vault-instance parameters being frozen does not make
an upgradeable program immutable.

Public claims therefore separate:

- behavior under the current verified program hash;
- frozen account/config fields;
- the exact upgrade authority and controller;
- Founder-held key-loss resilience versus independent control;
- a later authority transfer or revocation, if and when it occurs.

The word `immutable` is reserved for a program whose authoritative record shows
that no upgrade authority exists.

## Success and validation

Success requires a reusable capability and a route another developer can
actually consume, not merely more documentation. Validation requires
deterministic and full-scale tests, exact artifact provenance, public negative
receipts, explicit upgrade semantics and unrelated reproduction/security
preflight before any production claim.
