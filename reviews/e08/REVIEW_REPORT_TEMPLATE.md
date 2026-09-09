# E-08 v5 external review report — unfilled

Status: NOT REVIEWED. This file is a handoff template, not acceptance.

| Field | Reviewer to complete |
|---|---|
| Name and accountable role | |
| Organization and relationship to author | |
| Independence limitations | |
| Exact E-08 source commit and tree | |
| Scope manifest and SHA256SUMS digests | |
| Review dates and toolchain | |
| Both rebuilt v5 SBF sizes and SHA-256 | |
| Full local reproduction command, logs and result | |
| Independent source analysis beyond running tests | |
| Live network/genesis and observed slot, if actually reviewed | |
| Transaction-history/signature authentication, if performed | |
| Excluded scenarios and assumptions | |

Read `docs/E08_V5_READ_ONLY_AND_REVIEW.md` and the exact frozen
`spec/E08_REVIEW_SCOPE_v1.json`. Do not insert keys, credentials or private
applicant records. Review the exact checkout, not an unpinned branch later.

Required questions (provide evidence or mark NOT REVIEWED):

1. Do the unchanged v5 program and both exact SBF profiles match the E-07 pins?
2. Are role consent, backup quorum, successor acceptance, pause, notice, cancellation,
   expiry, retired-key rejection and epoch/nonce replay checks sound in SBF?
3. Are custody, T0, used quota, annual intervals and existing approvals continuous
   across both recoveries and subsequent actual token transfers?
4. Does independent decoding require every role/generic tombstone, every approval
   and every necessary permanent key index? Are author epochs and known self-payments
   handled consistently with the contract?
5. Does RPC export bind the reviewed initial/network identity and exact code,
   refuse races/omissions/over-limit graphs, and verify only a single final response?
6. Are the transport fixture, supplied-server observation, authenticated chain
   evidence and independent human review distinguished correctly?
7. Is the exact-clock proposal API deployable reliably? Record the current
   submission-time gap explicitly; do not certify it from controlled-clock tests.
8. Are profile restrictions, oracle truth, human/key independence, missing backup
   quorum, immutable target and unresolved production rights clearly disclosed?

For each finding record ID, severity, affected commit/file, assumptions,
reproduction or counterexample, impact, proposed repair and retested commit.
Separate reproduced attacks, design concerns and unsupported-profile cases.

Final recommendation: TO BE COMPLETED BY REVIEWER.

Outstanding findings and explicit residual risks: TO BE COMPLETED BY REVIEWER.
