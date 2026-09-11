# Bounded persistent-ledger and backup recovery

This adds an actual Agave process restart and an offline full-run copy/restore
to the existing v7 evidence tooling. It retains the original signed bootstrap
ledger and newly generated local test signing keys, reloads them from disk,
and verifies the same application accounts before continuing signed capacity
reports. It runs in the foreground and stops every validator it starts.

The commands use **TEST_ONLY** v7, one local controller holding all fixture
keys, and numeric loopback RPC. No production role, deployment, stable host,
background service or natural 90/180-day observation is established.

## Run and verify

Prerequisites: Python 3.10+, Node 20+, the locked npm dependencies and Agave
3.1.10. From a clean checkout:

```sh
npm ci --ignore-scripts
python3 -B tools/e11c_prepare_fixture.py --extract-program
python3 -B -m unittest discover -s tests -p test_soak_backup.py -v
node tools/soak_persistent.mjs acceptance --directory target/persistent-soak
python3 -B tools/verify_soak_persistent.py target/persistent-soak/public
node tools/verify_soak_persistent_signatures.mjs target/persistent-soak/public
```

The output directory must not exist. Ports 19599/19699 and dynamic ports
19700–19800 must be free. Budget several minutes for natural T0 and finality,
and several GB of free disk for the original run, backup and restored run.
The runner uses no reset or warp argument and does not rewrite Clock.

The acceptance sequence is:

1. Start with the pinned SBF in genesis, revoke the actual loader upgrade
   authority by signed transaction, create the mint/source/destinations, open
   the six-role policy, deposit both pools, arm and naturally reach T0.
2. Submit the first capacity report, export the whole raw account graph and
   record an observer sample. Prove that a backup of the running ledger is
   refused. Stop the owned validator process with SIGTERM.
3. Restart the same ledger without initialization flags. Compare all account
   records except Clock, require nondecreasing numeric slot/time, and query
   all previous signed transaction statuses. Submit a second report, observe
   and stop the node.
4. Under Agave's ledger lock, copy every regular run file and directory, and
   rebase validated internal aliases into the new root. Record SHA-256,
   length, mode and each alias's root-relative meaning;
   retain test keys at mode 0600 in a mode-0700 directory. Verify the copy
   against an externally retained index digest. Runtime IPC sockets are
   listed as omitted and are recreated by the node.
5. Restore into a new directory, start from that restored ledger, compare the
   same application state, check old finalized transaction signatures/slots,
   reload test keys and finalize a third capacity report. Stop the node.

Successful acceptance includes 13 finalized client transactions, five raw
account checkpoints, three observer samples, and three process start/stop
pairs. These are runtime acceptance targets; a printed target or unit-test
result alone is not a successful actual-node receipt.

## Ledger retention and private state

Agave test-validator defaults to `--limit-ledger-size 10000` shreds. During
development this pruned early bootstrap transaction history even though the
application accounts survived restart. The new runner sets an explicit
1,000,000,000,000-shred ceiling and checks `minimumLedgerSlot` and
`getFirstAvailableBlock` both remain 0, plus the full client signature list.
This large ceiling does not preallocate storage, guarantee affordable storage
for months, or make retention infinite. The host's storage budget, growth
measurements, backup rotation and alerting remain prerequisites for a long run.

History readiness is also checked separately from account readiness: immediately
after startup an old signature can temporarily be absent from the RPC response.
The driver waits up to 90 seconds for each historical status to become finalized;
an error, wrong slot or timeout still fails acceptance. Agave fastboot requires
absolute account-hardlink paths, so internal symlinks are validated within the
source tree and rebased to the backup/restore tree, never left pointing at the
original run. Regular file bytes are not rewritten.

The backup lock follows the pinned [Agave ledger lock implementation](https://github.com/anza-xyz/agave/blob/v3.1.10/validator/src/lib.rs).
The real-node test requires live-copy refusal before any backup target is
created. External or broken symlinks, special files, modified/missing/extra backup files,
wrong index digests, overlapping directories and existing restore targets
are refused. Partial failed copies remain for diagnosis; no cleanup command
silently removes them.

`run/`, `backup/` and `restored/` contain **unencrypted local test keys**. The
ledger also contains Agave validator, voter and faucet keys. They are not
production custody, must never be reused on public networks, and are excluded
from the uploaded artifact. Store an actual off-host backup and its head
digest separately only after selecting the host, storage and custody plan.

Only `public/` is packaged by CI: selected raw observations, public manifest,
signed envelopes, historical status responses, the read-only journal, and a
bounded acceptance receipt. The private ledger copy itself is retained on the
local host during a run; ephemeral CI storage disappears after the job.
An offline public-bundle check redecodes the accounts and journal and verifies
signature bytes. It cannot independently redo the private ledger restore or
authenticate an earlier RPC server's statements.

## A later bounded continuation

After a successful run, the newest ledger is `restored/`. A separate process
can load it, query the retained signed history, append one fresh capacity report
using the saved test keys, sample the journal and stop:

```sh
node tools/soak_persistent.mjs resume --directory target/persistent-soak/restored
```

Each signed attempt is fsynced before submission, and the state checkpoint is
atomically replaced only after finality. The tool does not blindly resubmit
an ambiguous attempt: an existing attempt/result filename or an inconsistent
report sequence fails closed. An interrupted bootstrap before `state.json`
exists retains its keys and attempt files but needs manual reconciliation;
this is not an automatic arbitrary-crash recovery state machine. A stale
`.driver-lock` also needs inspection after confirming its owning process has
stopped. Never restart both the original and restored fork simultaneously.

For stopped-run backups, `tools/soak_backup.py` also provides `backup`, `verify`
and `restore` commands (`--help`). Verification and restoration require the
separately retained `--expect-head`; no target is overwritten. Backups cover
the full stored run at a planned stop, not a power-loss-consistency guarantee.

No maturity state is preloaded in this new run, but it ends shortly after T0.
Founder funds remain under the 180-day cliff. Existing E11C mature-state
continuation remains a separate history. A complete natural path from this
bootstrap to maturity, human security review, independent governance and
production readiness remain open.
