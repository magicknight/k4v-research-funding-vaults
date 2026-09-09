# E-05 external review report — unfilled template

Status: NOT REVIEWED. Copy this template for a real review; do not treat its
presence as reviewer acceptance or an audit certificate.

| Field | Reviewer to complete |
|---|---|
| Name and accountable role | |
| Organization, if relevant | |
| Relationship to the author and independence limitations | |
| Exact source commit and tree | |
| Review start/end dates | |
| Environment and toolchain | |
| Reproduced SBF hashes | |
| Local reproduction command and result | |
| Source review performed beyond running tests | |
| Live RPC/chain-history review, if any | |
| Excluded scenarios | |

Inspect `docs/E05_INDEPENDENT_VERIFICATION_AND_REVIEW.md` and
`spec/E05_REVIEW_SCOPE_v1.json` before choosing a scope. Do not insert private
keys, RPC credentials, private applicant files or unrelated personal data.

For each finding, record its ID, severity, affected commit/file, assumptions,
reproduction or counterexample, impact, proposed repair, and retested commit.
Distinguish possible design risks from reproduced attack paths.

Required acceptance questions:

1. Do all four SBF artifacts rebuild to the E-04 frozen hashes?
2. Does the independent Python path reject substituted state and code bytes?
3. Does actual target loader authority belong to the immutable gate PDA?
4. Are epoch/replay and notice rules consistent across Rust and raw decoding?
5. Do the live funding accounts survive the exact tested upgrade unchanged,
   and do subsequent releases respect the same principal and budget limits?
6. Are the single-depositor graph, fixed committee, lost withdrawal-key gap,
   oracle truth assumption and arbitrary future upgrade risk stated correctly?
7. What evidence is still needed before any production adoption?

Final recommendation: TO BE COMPLETED BY REVIEWER.

Outstanding findings and explicit residual risks: TO BE COMPLETED BY REVIEWER.
