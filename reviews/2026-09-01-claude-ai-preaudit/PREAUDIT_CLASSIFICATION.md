# Classification and independent location check

## Classification

`INDEPENDENT_MODEL_AI_PREAUDIT`

This is a read-only AI pre-audit produced by Claude Code from a fresh public checkout. It is **not** a human-accountable security audit, a mainnet authorization, a deployment attestation, or proof that any reported deployment/evidence state is current. The findings below retain Claude's own severity/status labels; the independent post-check verifies citation locations and selected code snippets, not exploitability or remediation correctness.

## Source and run integrity

- Public origin: `https://github.com/magicknight/k4v-research-funding-vaults.git`
- Requested and final detached HEAD: `e1afead138fbf56956b298ebae7a97a8ae9ad956`
- Git tree: `e5fb279467a1e9d1f9063508f42be73926c71ba2`
- Git status before and after: clean; staged and unstaged diffs empty.
- Claude Code: `2.1.252`; requested model alias `opus`; exposed primary model `claude-opus-5` (small `claude-haiku-4-5` helper usage also reported).
- Run: non-interactive, `exit 0`, subtype `success`, 73 turns, 1,094,023 ms, no reported permission denial, 0 web-search and 0 web-fetch requests.
- Restrictions: safe mode, restricted mode, strict MCP configuration, slash commands disabled, no Chrome, no session persistence, `dontAsk`, and only `Read,Glob,Grep` tools.
- Blockers: none. No authentication, rate-limit, or context-window blocker was reported.

## Claude-labeled result summary

Claude reported `NOT_READY` for mainnet and counted:

- 0 critical, 0 high;
- 7 medium confirmed: permissionless B2 policy-hash squatting; permissionless B1 vault-PDA dust squatting; unenforced mint freeze-authority state; shared-window starvation/unbounded co-tenancy; single-oracle halt power; immutable/lost policy authority; and retained upgrade authority plus an incomplete B2 immutability caveat;
- 8 low confirmed: next-period approval timing trap; no account version/padding or migration path; unreleasable unchecked authority keys; stranded approval rent; a source-text-only instruction-surface test; unpinned frozen time constants; unauthenticated/non-failing devnet verification; and a Surfpool installed-byte/hash evidence gap;
- 7 informational items plus one confirmed informational CI gap;
- 1 low and 1 informational uncertainty;
- 16 investigated candidates rejected on code evidence.

Important scope qualification: the report itself says it did not query a cluster or verify `ProgramData`, mint authority, evidence hashes/signatures, or live parameters. Therefore K4V-07's deployment-state premise and the live-instance portions of K4V-03/K4V-04 remain repository-assertion-dependent even though Claude placed them in its confirmed table.

## Independent citation-location verification

The lexical verifier extracted 164 unique `file:line` citations:

- 59 exact repository-relative paths;
- 65 uniquely resolved suffix paths;
- 9 uniquely resolved ellipsis paths;
- 31 ambiguous basename/suffix paths that require prose context.

Every one of the 164 had at least one existing matching file with the requested maximum line in range. Separately, the report used 25 unique context-only fragments such as `:48`:

- 24 were contextually resolved to an existing file and an in-range line;
- `probes/b2_devnet_verify.mjs:63-83` is out of range by one because the file has 82 lines. The relevant `out.checks`, `console.log`, and `writeFileSync` code is present at lines 63-82, so this is a citation-boundary defect rather than missing substantive evidence.

Because 65 of 189 unique citation forms (31 ambiguous, 9 ellipsis, 25 context-only) are not exact repository-relative paths, the prompt's exact-citation formatting requirement was only partially satisfied. See `CITATION_VERIFICATION.json` and `CONTEXT_FRAGMENT_RESOLUTION.tsv` for the full machine-readable check.

## Validation boundary

No repository code, build, test, probe, RPC call, signing operation, transaction, deployment, or push was performed. Static location checks do not establish that a candidate is reachable, economically material, or correctly remediated. A human security review plus executable adversarial tests is still required before any mainnet decision.
