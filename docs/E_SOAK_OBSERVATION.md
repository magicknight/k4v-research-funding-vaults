# E-SOAK-01: persistent local observation journal

This tranche supplies the recording and restart checks needed for a future long
run. It does not deploy a persistent validator or complete 90/180 days. The
observer submits **no transactions**. Its only RPC methods are `getGenesisHash`
and finalized `getMultipleAccounts`, through the frozen v7 exporter. Only
numeric HTTP loopback endpoints are accepted; redirects and proxies are disabled.

## What is recorded

Each accepted observation binds the genesis hash, full policy identity, original
T0/config, exact v7 program and loader authority, complete approvals and recovery
history, all vaults and token destinations, and Clock from one final RPC response.
Every observation is decoded again during offline replay. An RPC failure is an
ERROR event, not a missing line silently presented as uptime.

`run.json` binds the initial reviewed manifest, declared origin, current Git
commit, whether the observer source worktree is clean, actual Python source
hashes, gap policy and program SHA-256. The source hashes remain authoritative
when developing in a dirty checkout. Use a clean exact commit for an accepted run.

`journal.jsonl` has sequence numbers, previous-record hashes, run binding,
observer session and host boot IDs, host wall/boottime clocks, and raw-account
references. `objects/` holds content-addressed full account envelopes, including
the raw bytes. Stable program and account data are stored once. Records and new
objects are fsynced before their references are committed. A single-writer file
lock also protects replay from concurrent writes. Partial records, altered
objects, changed decoder code and incompatible manifests stop replay/resume;
the tool never truncates or repairs the original evidence automatically.

Supply `--expect-head` from an independently retained earlier receipt when
resuming. A hash chain alone cannot detect replacement of the whole history or
deletion of an unanchored suffix. Back up the directory and save its latest
head digest outside the observation host. No backup or external publication is
performed automatically by this tool.

## Commands

From the exact repository checkout, with an existing v7 local validator and a
reviewed manifest (the observer does not create either):

```sh
python3 tools/soak_observer.py init \
  --directory /srv/k4v-observations/run-001 \
  --manifest /srv/k4v-inputs/manifest.json \
  --origin signed-bootstrap-declared

python3 tools/soak_observer.py record \
  --directory /srv/k4v-observations/run-001 \
  --manifest /srv/k4v-inputs/manifest.json \
  --rpc-url http://127.0.0.1:19599 \
  --samples 0 --interval-seconds 600

python3 tools/soak_observer.py verify \
  --directory /srv/k4v-observations/run-001
```

Paths above are examples, not provisioned locations. The recorder runs in the
foreground; SIGINT/SIGTERM requests a clean STOP after any in-flight bounded RPC.
Finite `--samples` counts attempts, including failures; exit status is nonzero
if an attempt fails. The journal keeps failures across subsequent sessions.
To extend approval periods or destinations, stop recording, review an updated
manifest, and resume with it and the previous `--expect-head`. Identity may not
change; existing destinations and approval periods may not be removed. The
frozen exporter's 100-account limit still applies and must not be bypassed by
dropping history.

`verify` success means journal structure and supplied account bytes verify.
Inspect its `observation_errors`, `anomalies`, session count and observed spans;
`valid: true` does not mean uninterrupted uptime or successful long-duration
execution. Slot/time rollback, bank/wall divergence, host-clock discontinuity,
host reboot and excessive observation gaps are reported. Observer restarts are
visible as distinct sessions, even when they close cleanly. Validator restarts
under the same genesis cannot be established from these sampled reads alone.

## Bounded acceptance

```sh
PYTHONPATH=src python3 -m unittest discover -s tests -p test_soak_journal.py -v
python3 tools/e11c_prepare_fixture.py --extract-program
npm ci --ignore-scripts
node tools/soak_agave_smoke.mjs
```

The actual-node smoke additionally needs Agave `3.1.10` on PATH. It runs the
unchanged E11B signed six-role bootstrap, observes at least four fresh account
graphs in two observer sessions, verifies the externally passed journal head,
and lets the original short rehearsal stop its validator. It refuses stale
smoke output or an existing bootstrap manifest. Use a fresh checkout for another
run. The program is extracted from verified frozen evidence, not rebuilt here;
E11B remains the separate reproducible-build acceptance.

This smoke tests **observer restart**, not validator restart, restored ledgers,
host reboot or natural 90/180-day maturity. No role secret is written by it.
The `signed-bootstrap-declared` field is an origin declaration, not an
independently authenticated claim; the smoke binds the actual E11B receipt
separately. All summaries keep the natural-soak, continuous-maturity,
independent-human-review and production-ready verdicts false.

## Before starting a durable run

The host, storage and operating responsibility are not selected. A complete
deployment still needs a dedicated loopback validator with persistent ledger,
startup arguments and genesis retained; frozen source/SBF hashes; clock
discipline and monitoring; shutdown/restart/restore receipts; sufficient storage
for the full ledger and backups; and explicitly TEST_ONLY signing custody for
scheduled recovery, expiry and year-two transactions. The current E11B short
driver deletes its temporary ledger and does not save its private keys, so it
must **not** be installed as the long-running service.

A new persistent bootstrap/transaction driver and its restart/restore acceptance
are separate follow-on work once the host and custody plan are concrete. No
existing production key, public-chain endpoint, paid hosting or human role is
selected by E-SOAK-01. A naturally reached 180-day cliff would establish only
that milestone; year-two behavior requires its own actual elapsed timeline.

The eventual decision requires the retained bootstrap transactions, complete
ledger and clock/host records, actual mature recovery/withdrawal receipts, and
accountable review alongside this journal. The observer intentionally cannot
issue that decision just because a timestamp crosses a duration threshold.
