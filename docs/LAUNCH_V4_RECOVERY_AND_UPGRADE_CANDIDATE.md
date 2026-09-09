# E-04 TEST_ONLY recovery and upgrade candidate

This tranche adds a separate `launch-vault-v4` and an external
`upgrade-gate-v1`. It does not upgrade the historical B1/B2/v2/v3 accounts,
choose production recovery rights, or deploy to a public chain.

## Frozen test profile

| Item | TEST_ONLY value / scope |
|---|---|
| Vault program | `5h5iUez8fpHThaQhDdyUSQab9ngRmG2zfMBNx5bGnB9Q` |
| External upgrade gate | `5JVsAM5AnTBdeHsGjC78hcrrv9KfLKVgCNN4RzBiWjmA` |
| Both default artifacts | Reject policy/gate creation |
| Key-change notice | Exactly 90 × 86,400 seconds |
| Recovery committee | Three distinct consented keys, two distinct signatures required |
| Upgrade committee | Separate fixed three-key committee, two distinct signatures required |
| Upgrade notice | Exactly 90 × 86,400 seconds |
| Emergency bypass / silence release floor | None |
| T0, 180-day cliff, annual inputs and reserved-capacity math | Inherited v3 test semantics |

Program addresses derive from explicitly public test seeds in the local loader
tests. They are reproducibility fixtures, never production signing credentials.
The two committees may be held by the same person. Distinct keys demonstrate
key redundancy, not independent human governance.

## Key-change authority and threat model

The immutable creator and initial oracle remain identity inputs. The current
controller and current oracle are separate mutable fields. New identity bytes
also bind all three recovery keys, the 90-day notice and the two-signature
threshold. Recovery members cannot be silently changed after initialization.

| Action | Propose and accept | Cancel | Execute |
|---|---|---|---|
| Normal oracle or controller change | Current controller plus successor signature | Current controller, successor, or recovery quorum | Anyone after full notice |
| Lost-key oracle or controller recovery | Two registered recovery signatures plus successor signature | Successor or recovery quorum; old controller alone has no veto | Anyone after full notice |

The policy admits one pending proposal. Each proposal binds policy, kind,
recovery mode, successor, monotonically increasing nonce, controller/oracle
epochs, creation time and maturity. Consumed/cancelled proposal accounts remain
as tombstones. Nonces are never reused and there is no close-and-recreate path.

A compromised controller can queue a normal change but cannot permanently
occupy the slot: recovery quorum can atomically cancel that proposal and create
a recovery proposal in the same transaction. It cannot make the recovery
mature earlier. A successor can withdraw acceptance by cancelling the pending
proposal. The test profile has no automatic expiry.

These routes do not provide immediate incident containment. Until execution,
the current oracle can still publish reports and the current controller retains
its ordinary powers. A 90-day wait can prolong an outage or a compromised-key
window. Two compromised recovery keys can appoint a replacement after notice.
Insufficient surviving recovery keys means recovery fails; there is no hidden
administrator route.

### Report generations

Oracle replacement increments its epoch, stores the activation timestamp and
marks the old report invalid. It preserves the last observed timestamp and the
global sequence counter. A fresh report must:

- be signed by the current oracle and explicitly name its current epoch;
- have a sequence strictly above the lifetime report sequence;
- be observed no earlier than the new oracle's activation;
- satisfy the existing current-period and freshness rules.

Returning to a previously used oracle public key does not revive an old epoch.
Controller-only replacement leaves valid oracle reports intact.

### Accounting and withdrawal authority

Governance changes never reset T0, config, period/annual/lifetime counters,
escrow principal, beneficiary identity or treasury approval bytes. The ordinary
release path still enforces notice, recipient, cliff and capacity restrictions.
Failed operations roll back their account changes.

**Controller recovery does not recover a lost Founder or Treasury withdrawal
key.** Those beneficiary identities remain frozen. If one key fills all roles,
restoring only its controller role does not restore its withdrawal roles.
Beneficiary custody/recovery remains a separate production design gap. Likewise,
the fixed upgrade committee has no additional recovery route after two of its
three keys are lost.

## External upgrade delay

A document announcing a future upgrade does not constrain a loader authority.
This candidate transfers the target's actual upgrade authority to a PDA owned
by a separate gate program. Gate initialization requires all three committee
signatures and the existing target upgrade authority's signature.

Before taking control, the gate checks its own canonical upgradeable-loader
ProgramData account and requires `upgrade_authority=None`. This is irreversible
for that gate program. A mutable gate is rejected, so its code cannot itself be
replaced to remove the delay.

The target uses the classic upgradeable loader. The gate validates executable
Program state, canonical ProgramData, loader owners and the current authority.
It has no instruction to transfer or revoke target upgrade authority. Thus the
candidate demonstrates a controlled-upgrade route, not a production selection
between immutable and upgradeable targets.

### Buffer and proposal lifecycle

1. Upload a candidate buffer and transfer its loader authority to the gate PDA.
2. Two committee members propose its exact buffer address, code SHA-256 and
   length; the buffer return authority signs and is bound to the proposal.
3. The gate starts a full 90-day clock. No other upgrade can be queued meanwhile.
   The former uploader cannot write the locked buffer.
4. After maturity, anyone can execute the exact nonce. The gate rechecks the
   buffer authority, bytes, length, target state and refund destination, then
   signs the real loader `Upgrade` CPI with its PDA.
5. A committee cancellation returns the buffer authority only to its recorded
   recipient. Reproposal consumes a fresh nonce and restarts the entire notice.

Two committee signatures can also return an unqueued gate-owned buffer to a
specified recipient. This path explicitly rejects the current pending buffer.
There is no buffer-write endpoint or target-authority escape. Execution checks
the hash again even though the pending buffer is locked.

The notice does not prove that an upgrade is safe. The selected target binary
may change its release semantics after a properly authorized upgrade. The gate
enforces who can propose, what bytes are selected and when they may execute;
source review, semantic compatibility and public disclosure remain necessary
production work. Invalid ELF fails in the loader and the proposal, buffer and
target account changes roll back atomically.

## Local validation boundary

Vault lifecycle tests execute SBF and signed SPL transfers in LiteSVM.
Upgrade tests install both programs using signed native-loader buffer writes
and deployment instructions, seal the gate, transfer target authority, and
execute the gate-to-loader CPI. They do not use `add_program` injection for those
upgrade tests. Time and slots are controlled local test sysvars.

The exported loader receipt records actual account headers, code hashes,
proposal/execution signatures and rejected early execution. It is local
runtime evidence, not authenticated public RPC evidence or a human audit.
One separate buffer-corruption test deliberately edits test state to exercise
execution-time hash rejection; the successful receipt does not use that edit.

Compiler-generated interfaces, an independent JavaScript identity codec and
frozen SBF hashes accompany the candidate. The original v3 raw verifier does
not accept the v4 layout. A full independent v4 governance/loader RPC verifier,
whole-system state compatibility review and externally accountable security
report are E-05 work.

## Next tranche

E-05 connects the v4 raw policy/proposal state and real loader state into an
independent verifier and a reproducible review package. It must distinguish
current program bytes, current controller keys, proposal maturity, actual
loader authority and committee limitations. Production annual sources/calendar,
beneficiary custody, committee identities, recovery rights, exact T0 and
upgrade-mode adoption remain open.


## Reproduce

The complete pinned build recipe is
[`.github/workflows/launch-v4.yml`](../.github/workflows/launch-v4.yml).
It builds both default and `test-profile` SBF artifacts for each new program,
rejects stack-offset diagnostics, and compares their exact hashes with
[`spec/E04_TEST_ONLY_CANDIDATE_v1.json`](../spec/E04_TEST_ONLY_CANDIDATE_v1.json).

After those four builds, from the repository root:

```sh
export CARGO_TARGET_DIR=/tmp/k4v-native-target
export K4V_V4_SNAPSHOT_OUT="$PWD/target/v4-raw-snapshot.json"
export K4V_GATE_RECEIPT_OUT="$PWD/target/upgrade-gate-real-loader.json"
cargo test -p launch-vault-v4 -p upgrade-gate-v1 --features test-profile --locked
python3 tools/build_launch_v4_idl.py --check
python3 tools/build_upgrade_gate_v1_idl.py --check
node --test probes/launch_v4_identity.test.mjs
python3 tools/verify_e04_artifacts.py
python3 tools/check_e04_loader_receipt.py target/upgrade-gate-real-loader.json
```

Use a separate native target directory so native builds cannot overwrite SBF
artifacts under test. The two export paths must be absolute because Rust tests
execute with their package directory as the working directory. The receipt
checker verifies header/artifact coherence only, not transaction-history
authenticity.

Local acceptance: 44 v4 Rust checks (39 signed lifecycle/recovery cases), seven
gate checks (six native-loader transaction cases), and four independent identity
checks pass. Earlier programs retain their exact artifact bindings and pass
their regressions. The full scoped receipt is
[`evidence/E04_LOCAL_VALIDATION_2026-09-09.json`](../evidence/E04_LOCAL_VALIDATION_2026-09-09.json).
