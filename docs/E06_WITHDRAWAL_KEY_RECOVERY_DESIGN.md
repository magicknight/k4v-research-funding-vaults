# E-06: beneficiary withdrawal-key custody and recovery

2026-09-09 · **TEST_ONLY DESIGN + EXECUTABLE AUTHORITY MODEL**

The existing v4 program can recover its controller and oracle, but cannot
replace its Founder/Treasury withdrawal authorities. E-06 freezes a reviewable
candidate for those two roles. It is **not an on-chain repair, migration,
deployment or production decision**. E-04 program bytes and E-05 receipts are
unchanged. The model contains no wallet, RPC client, token transfer or SBF.

## Why this is a separate right

A recovery quorum can ultimately redirect the affected role's future permitted
withdrawals. Keeping principal and release limits fixed does not make that
power economically harmless. Controller recovery therefore confers no
beneficiary recovery authority. Each beneficiary must consent to its own
registered recovery scope when a new policy is created, signing the full
identity/configuration commitment. Registration cannot be appended to an
already funded v4 policy without a separately reviewed migration.

| Role | Day-to-day authority | Recovery scope in this test profile |
|---|---|---|
| Founder | Current Founder signer; destination owned by that signer | Its own pre-registered 2-of-3 committee and accepting successor |
| Treasury | Current Treasury signer approves future needs and authorizes releases to fixed third-party recipients | Its own pre-registered 2-of-3 committee and accepting successor |
| Controller / oracle | Existing v4 governance scope | No automatic power over either beneficiary role |
| Upgrade committee | Existing external gate scope | No new bypass in E-06; an authorized future code upgrade remains a separate trust assumption |

Distinct keys are not evidence of distinct humans. One person may hold the
day-to-day keys and all backups. Separate devices and failure domains then
provide only operational redundancy. No guardians have been appointed, no
secrets generated or copied, and no human independence is claimed here.

## Frozen candidate semantics

`src/withdrawal_recovery_model.py` is the executable authority reference.
`spec/E06_WITHDRAWAL_RECOVERY_TEST_ONLY_v1.json` records its limits and bindings.
All numbers below are **test choices**, not adopted production rights.

| Operation | Consent and state transition |
|---|---|
| Register | Initial Founder and Treasury sign their respective registrations. Exactly three distinct nonempty backup keys per role; a current operating key cannot be its own backup. Committees are fixed in this profile. Roles may share an operator or explicitly registered backup keys, without claiming independent control. |
| Normal rotation | Current role key plus successor sign one fully bound proposal. |
| Lost/compromised-key recovery | Two distinct keys from that role's registered committee plus successor sign. The missing current key is not required. |
| Bind | Domain, policy, role, monotonic role nonce, current epoch/key, successor, mode, creation time, execution time and expiry are committed together. Signatures must cover the exact proposal in the future implementation. |
| Wait | Exactly 90 days. Only the affected role is paused immediately: Founder withdrawals, or Treasury approvals and withdrawals. Other roles keep their existing rules. No overwrite and no emergency fast path. |
| Execute | Anyone may execute from `created_at + 90 days` inclusive until `created_at + 120 days` exclusive. Increment only that role's authority epoch; consume the nonce, retain history and terminal proposal. |
| Cancel | Successor or that role's 2-of-3 quorum may cancel. The current key may also cancel a normal rotation, but cannot veto recovery. No current-key/admin/controller cancellation bypass. |
| Expire | Anyone may close at or after day 120. It does not rotate authority. Until execution/cancellation/expiry is explicitly processed, the role stays paused. A new proposal consumes a new nonce and restarts the full 90 days. |
| Resume | Cancellation/expiry restores the old key's permitted operations; execution enables the successor. This makes expiry a liveness choice with a compromise risk, not an emergency security guarantee. |

The model requires nondecreasing action timestamps and bounded integer inputs.
Nonce/epoch/time overflow fails. A retired operating key cannot be reused for
that role. No committee rotation or recovery-of-recovery escape exists in this
profile. Losing two backups **and** the current key is unrecoverable. A surviving
current key can still perform normal rotation; it cannot repair a lost committee
through an undocumented route.

The pause is deliberate: a stolen operating key cannot continue withdrawing
after a valid recovery proposal lands. It cannot recover assets withdrawn before
that transaction. A malicious quorum can repeatedly pause or eventually take
over its role, and a malicious accepting successor can cancel and cause delay.
Expiry bounds one proposal, not a determined quorum's aggregate denial of service.

## Identity, custody and the financial boundary

The candidate separates stable policy/role identity from the current signing
key. Initial identities, canonical vault/token PDAs, original depositor, mint,
principal, official T0, 180-day Founder cliff, fixed periods, annual inputs,
report generations, consumed amounts and lifetime totals must survive recovery.
No new deposit entitlement, new vault identity, catch-up credit, carried quota,
refunded principal or changed oracle report is created by a key change.

The Python model preserves an **opaque financial-state witness** byte for byte
and tests this against an independently verified E-05 raw-account graph. This
establishes an authority transition contract, not a Solana account migration or
enforcement of financial math by this new model. `authorize_release` returns
`financial_kernel_required=true` and `transfer_executed=false`. It is deliberately
not a complete release operation: the eventual instruction must atomically check
real signatures, SPL ownership/mint/recipient binding, lifecycle, T0/cliff,
fresh oracle generation, approved remaining need, principal and all period,
annual and reserved shared-capacity limits before CPI and accounting updates.

E-07 must use an **isolated new TEST_ONLY program/account namespace**, admission
disabled by default, and explicit current authority/epoch fields. V4's existing
`has_one`, `owner_for` and destination-owner checks cannot be made recoverable by
changing only a client. No existing v4 account is reinterpreted or upgraded by
E-06. Shared operating keys, if supported, must still produce distinct role-bound
identities/PDAs; the symbolic model does not prove that ABI property.

A stable external multisig/custody PDA is another architecture: earlier B2
experiments show a member change under a fixed authority. That does not establish
this profile's per-role 90-day freeze, successor acceptance or safe forwarding.
E-06 chooses explicit role authority fields for the **next isolated experiment**,
without silently adopting multisig custody or any production migration.

## Treasury approvals survive, recipients do not move

An existing approval retains period, exact recipient token address and owner,
need, consumed amount, creation time, original approving key and its epoch.
The recovered Treasury key may continue executing it under the unchanged
financial checks and original notice. The old approving signature grants no
ongoing withdrawal right. Expired-period approvals do not roll forward.

The profile prohibits Treasury payments to known initial/current/retired
beneficiary keys, either role's registered guardians, and pending successors.
Conversely, a historical approved recipient owner cannot become a successor,
even after its approval is spent or expired. Eligibility is checked at proposal,
execution, approval and release. This conservative, permanent key separation
prevents a known recipient from becoming an apparent third party through key
rotation. It can require a fresh operating key for legitimate users; an eventual
implementation must retain a queryable history, not scan an unbounded account
list in one transaction.

**Unresolved real-world identity boundary:** the same human can create an
undisclosed fresh wallet. Public-key exclusion does not enforce beneficial-owner
independence, detect all self-dealing or prove a scientific expense is real.
An explicit test preserves this counterexample. Related-party review and actual
expense evidence remain external work; recovery does not solve them.

## Threat cases and evidence

| Case | Model result / residual limit |
|---|---|
| Controller, stranger, one backup or duplicate signature tries recovery | Rejected; role-specific quorum and successor acceptance required |
| One operating key serves Founder, Treasury and Controller and is lost | Founder/Treasury can recover independently through their registered scopes; Controller is untouched by this model |
| Early execution, stale epoch, cancelled nonce, cross-policy/role digest | Rejected without input-state mutation |
| Old key tries to veto recovery or use a retired signature | Rejected; normal-rotation cancellation remains possible |
| Pending recovery used to restart budgets or rewrite Treasury recipient | No financial witness mutation; approval fields retained; recipient/key collisions rejected |
| Successor also lost or never executed | Cancellation/permissionless expiry closes proposal; no shortened re-proposal |
| Two backups lost or quorum malicious | No hidden rescue; irreversible loss or takeover/denial of service remains possible |
| Fresh wallet controlled by the same beneficiary | Undetectable by key comparison; explicit accepted counterexample |

Run without a wallet or Rust toolchain:

```sh
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_withdrawal_recovery_model.py' -v
```

There are 22 authority tests, including all 128 subsets of a seven-key signer
universe, four terminal paths for both roles/modes, and binding to verified E-05
raw bytes. GitHub's existing deterministic-reference job discovers them. These
are author-run model tests; independent human review and actual E-06 transactions
have not occurred. The existing E-05 review scope remains its original scope.

## E-07 implementation acceptance

1. Freeze a new account/ABI and cross-language identity vector with per-role
   authority, epochs, fixed recovery registration, proposal tombstones and
   retained key/recipient indexes; no v4 migration in this tranche.
2. Carry the existing financial kernel into the isolated candidate. Register
   full-policy consent before deposits; reject unregistered or post-funding
   registration. Pre-T0 cancellation and refunds must remain available to their
   original authorized actors despite beneficiary pauses; cancelled policies
   cannot rotate or release. Define PREPARED/ARMED behavior explicitly.
3. Run signed SBF transactions for both-role recovery, threshold and notice
   boundaries, current/successor consent, freeze/cancel/expiry, stale signatures,
   role/account substitution, known self-payment and missing history accounts.
4. Fund both pools, partially release, cross 180-day/period/annual boundaries,
   recover and continue releasing. Compare raw principal/counters/approvals and
   test cliff, cap-plus-one, no-carry and rejected-transaction state equality.
5. Extend independent raw decoding and the reproducible review package to the
   exact new accounts and code. A passing model alone cannot mark the withdrawal
   vulnerability repaired in SBF or on a public chain.

Production people, quorum, wait/freeze/expiry rules, emergency/loss policy and
upgrade authority still need Owner selection and accountable review. No token
issuance, website deployment, outreach, purchase or public-chain transaction is
part of this design tranche.
