# E-11B review handoff

Review one exact published commit with its v7 build identities, compiler ABI,
identity vector, `E11B_SHA256SUMS` and `evidence/e11b/source-publication.json`.
Do not substitute v6 bytes, the offline prototype or an unrelated current branch.

The report must name the reviewer, independence/conflicts/compensation, checkout
commit/tree, actual commands, rebuilt SBF sizes/hashes, environment, findings
and retest scope. The human gate stays OPEN without that record. Author/AI/CI
evidence does not fill it.

Run `bash tools/run_e11b_acceptance.sh` after installing the pinned prerequisites.
Compare your new run with the archive; do not call an archived author run your
own reproduction. Challenge preparation immutability/rent prefunding, every
required role signature with a separate fee payer, config/program substitution,
replay/cancellation, disabled admission, mint authorities and T0 at both stages.
Check that the old direct initialization entry is absent from the compiled ABI.

Then verify both pools' allocation/annual accounting, shared-capacity reservation,
approved treasury need/notice, withdrawal-key recovery, old-key rejection,
cancellation/expiry and continued withdrawals. Preparation must not authorize
custody or restart financial counters. The separate Python decoder must bind
preparation, policy, complete histories, actual loader control and exact bytes.

Finally check actual Agave transactions, finality, immutable signed messages,
packet limits, Clock/RPC consistency and refusal state invariance. Natural long
periods and controlled-Clock boundary tests are separate claims. Six separate
fixture keys are not six independent people.

Production ownership/recovery/upgrade choices and annual market-data truth are
not selected by these fixtures. Issuer, legal, procurement, real demand and
funding are outside this engineering review. Nothing here establishes K4 science
or investment returns.
