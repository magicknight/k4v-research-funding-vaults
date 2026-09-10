# A runnable evidence walkthrough

This entry is for a developer or grant reviewer who wants to see what the
current vault tooling can demonstrate without installing a wallet or validator.
It is **offline replay of archived author-run evidence**, not an interactive
custody application, a new live execution or independent human review.

## Run from a clean, exact checkout

Requires Python 3.10 or newer and this repository with its committed archives.
No package installation, RPC credentials, wallet, key or network request is
needed after checkout. Select and record the exact Git commit yourself.

```sh
git rev-parse HEAD
python3 tools/evidence_demo.py
python3 tools/evidence_demo.py --format html --output /tmp/k4v-evidence-demo.html
```

Open the generated HTML locally. Output files are created exclusively; choose
a new filename if one already exists. On any verification error the command
exits nonzero and creates no success report. HTML includes no scripts, forms,
remote fonts or external assets. A saved HTML report is not a trust anchor:
rerun from the checkout you independently selected.

The tool verifies all three existing checksum manifests before running the
unchanged E11B and E11C archive verifiers. It presents initialization, the
financial lifecycle, key recovery, expiry/year-two accounting and refusal
invariants. Machine output keeps the verifier results and their scope limits.

## Three-minute walkthrough outline (not a recorded video)

1. Explain the concrete use case: a project wants its founder and purpose-bound
   treasury release restrictions to be inspectable from account bytes.
2. Run the command and show initialization plus the financial checkpoints.
3. Show recovery preserving balances/budgets, and constrained continuation.
4. Show the separate expiry/year-two scenario and refused-operation invariant.
5. End on the limitations: E11C starts from preloaded mature state; no natural
   90/180-day history, human security sign-off, production profile or token launch.

The original initialization and E11C continuation are separate histories. Do not
combine their counts into a single end-to-end live run. Raw-account checking is
not an independent cryptographic finality proof or a fresh signature replay.

For fresh local execution and human review, use
[the current handoff](CURRENT_REVIEW_HANDOFF.md). For persistent observation use
[the E-SOAK instructions](E_SOAK_OBSERVATION.md); no long-lived service is started
by this demo. The v7 production build still rejects policy admission by default.

Development of this walkthrough predates the 14 September 2026 hackathon; it
must be disclosed as prior work if used in a competition submission.
