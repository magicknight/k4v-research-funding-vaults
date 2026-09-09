# E-08 v5 read-only verification and external review handoff

Status: author-run engineering acceptance; named human review remains OPEN.
The exporter reads the E-07 TEST_ONLY candidate. No v5 public deployment,
official mint, v4 migration, production adoption or new chain transaction is
established by this package.

## What can be checked

`src/launch_v5_rpc_exporter.py` accepts a reviewed manifest with an explicit
genesis hash, fixed v5 program, policy PDA, full identity digest, spec digest,
mint, creator, initial Founder/Treasury keys and initial oracle. The identity also commits
to all three key committees and the entire financial configuration. The
manifest supplies the depositor's source token account, one Treasury destination,
one to eight Founder destinations and **all** Treasury approval periods,
including future and unused approvals. `approval_count` must agree exactly.

The program, ProgramData, both vaults, both custody token accounts, Clock,
complete Controller/oracle change history, complete per-role withdrawal
history and required persistent key indexes are derived canonically. Callers
cannot supply substitute history addresses or suppress cancelled/expired
tombstones. Discovery does not enumerate the whole program: approval periods
and external token addresses must be provided by the reviewer from the policy's
published records. A missing period fails; the exporter never guesses it.

The RPC contract permits at most 100 addresses per `getMultipleAccounts` call,
returns values in request order and includes one response context. See the
[official method reference](https://solana.com/docs/rpc/http/getmultipleaccounts).
Network identity is observed with
[`getGenesisHash`](https://solana.com/docs/rpc/http/getgenesishash).
These observations require trust in the selected server; they are not chain
proofs. `minContextSlot` is a lower bound, not a request to freeze a historical
bank. The implementation therefore uses this bounded protocol:

1. Check the expected genesis; read policy and Clock with finalized commitment.
2. Derive every proposal nonce and supplied approval period; read these with
   policy and Clock. Require unchanged policy bytes. Read successor and recipient
   subjects from the fixed-width records to derive their key indexes.
3. Refuse if the **whole** account graph exceeds 100. Read every account again
   in one finalized response. Require its Clock slot to equal its context slot,
   nondecreasing discovery slots, and unchanged discovered policy/history bytes.
4. Verify only that final response: full initial identity, canonical accounts,
   complete histories, author epochs, notices, persistent history/pending masks,
   current authorities, pauses, custody, annual/period budgets, token conservation
   and the exact immutable E-07 test-profile loader bytes. Recheck genesis.

Discovery bytes never enter the accepted snapshot. Policy/history changes cause
`RPC_DISCOVERY_CHANGED_RESTART_REQUIRED`; retry from the beginning with a reviewed
complete manifest. Lamport donations can occur between reads, and final lamports
are retained. No automatic retry, chunking, partial-history success, signing,
transaction submission, simulation, airdrop or program-wide scan exists here.

The raw verifier allows up to 256 generic proposals, 256 per withdrawal role
and 64 approvals, but these are **not** the exporter capacity. All accounts and
required key indexes must fit the tighter 100-account total. Larger histories
need a separately designed consistent export mechanism and currently fail.

## Reproduce without a public network

```bash
bash tools/run_e08_local_reproduction.sh
```

This verifies pinned review inputs, runs the E-07 SBF reproduction (both exact
builds, signed local execution and independent raw verification), and then serves
all eleven frozen E-07 checkpoints through loopback HTTP to the real exporter.
The replay makes 55 read-only requests: 33 account reads and 22 genesis reads.
Returned bytes and lamports must match the signed-runtime fixtures. Clock and
fee airdrops remain E-07 local-fixture assumptions. A loopback response is **not**
a real validator's RPC bank or a public-chain deployment.

For the isolated transport gate, after separately accepting E-07:

```bash
K4V_E08_TRANSPORT_ONLY=1 bash tools/run_e08_local_reproduction.sh
```

The new RPC suite has 28 tests, including real loopback transport, both roles,
unused future approvals, generic tombstones, missing records, discovery races,
advancing final banks, stale/wrong Clock, byte substitution, wrong initial/network
identity, the 100/101 boundary, required-key overflow, invalid envelopes, duplicate
JSON fields, response limits, redirect refusal and output preservation. Some
negative/generic-history transport fixtures are synthetic and are explicitly
not additional signed-transaction receipts. The 15 existing raw-v5 tests remain
part of the full E-07 reproduction; the combined v5 Python suite has 43 tests.

`examples/e08_LOCAL_REPLAY_ONLY_manifest.json` is bound to the frozen local
rehearsal. Its all-ones genesis is a placeholder, and its addresses are local
test identities. Do not treat it as a network configuration or official mint.

For a separately established compatible deployment, the read-only interface is:

```bash
PYTHONPATH=src python3 src/launch_v5_rpc_exporter.py \
  --rpc-url "$K4V_REVIEW_RPC_URL" \
  --manifest reviewed-v5-manifest.json \
  --min-context-slot 123 \
  --output new-v5-observation.json
```

Use a freshly reviewed manifest and a meaningful minimum slot. No compatible
public v5 endpoint is asserted here. HTTPS is required except explicit loopback
HTTP. Redirects, URL user-info and fragments are rejected. Error output and saved
observations omit the endpoint and credentials. Output creation refuses to
overwrite previous evidence. The observation records its canonical manifest
digest, slots, exact program hash and explicit unresolved authenticity flags.

## Exact candidate and human acceptance

The SBF candidate remains E-07 merge
`bea6f054b092c833f1b6669a751dca0cbc6f251d`, tree
`fa81b78e8a7fcd0ca982d05cbd59206b05304638`. Both builds remain:

| Profile | Bytes | SHA-256 |
|---|---:|---|
| Default, policy creation disabled | 509536 | `8c13d6304001820add80fa3059ec39b8baa4a98b0479e0db72dd9ebc6ec143cc` |
| TEST_ONLY, immutable local target | 528160 | `1dca41034f873882c41cc3eaaecc5b0124ae497f3ae41263a50ffccf9d339f14` |

`spec/E08_REVIEW_SCOPE_v1.json` binds the unchanged E-07 inputs and the new
exporter/reproduction inputs by file size and hash. `SHA256SUMS` covers the
delivered repository files. The reviewer must record the exact E-08 checkout
commit and tree (`git rev-parse HEAD HEAD^{tree}`) in
`reviews/e08/REVIEW_REPORT_TEMPLATE.md`; the manifest cannot embed its own final
commit hash without a circular dependency. Rebuilding hashes and running tests
is distinct from independent source review. No reviewer is named or contacted,
and the blank report contains no acceptance.

Review the on-chain authority and financial rules, independent decoder, RPC trust
boundary and operational recovery together. Required residual limits include:

- `created_at == Clock.unix_timestamp` on the current proposal instruction is
  an exact-clock TEST_ONLY API. Signing before a real transaction lands may cross
  a second boundary and fail. Local controlled-clock success does not establish
  reliable real-network submission. E-09 will address the time-binding design
  and test delayed submissions before changing a candidate.
- Only ACTIVE dual-pool, one-depositor, one Treasury destination and a complete
  retained token-balance graph are accepted. Legitimate downstream transfers,
  burns, closed destinations, multiple Treasury recipients, PREPARED/ARMED/CANCELLED
  states and larger histories can be outside this verifier's profile. A profile
  rejection alone is not an on-chain vulnerability.
- RPC can fabricate an internally consistent response and genesis. Neither
  signatures nor transaction history nor economic report truth are authenticated.
- Single-person multiple keys do not establish independent humans or prove that
  a recipient is unrelated. Loss of two role-backup keys can prevent recovery.
  v5's immutable local target does not adopt the v4 upgrade gate or production
  emergency/upgrade rights.
- Actual annual inputs, production rights, accountable review, real demand
  feedback and actual cash/fee quotations remain outside engineering acceptance.

The next autonomous engineering task is E-09's bounded submission-time work.
E-R1 is ready for a named reviewer; F-02 awaits real feedback invitations/replies.
Neither those contacts nor production choices have been completed by this package.
