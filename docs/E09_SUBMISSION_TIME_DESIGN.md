# E-09: bounded submission windows and full notice from admission

Status: DESIGN AND EXECUTABLE MODEL PASS; existing v5 liveness gap reproduced.
This adds tests against frozen v5 SBF and an executable timing model. It does
not implement a new SBF program, migrate v5 or establish public deployment.

## Reproduced problem

V5 requires the signed `created_at` argument to equal `Clock.unix_timestamp`
at execution. A transaction can cross a second boundary after signing. Editing
that argument without new signatures invalidates the signed message.

The new `programs/launch-vault-v5/tests/e09_clock_probe.rs` exercises unchanged
E-07 SBF with actual signatures. It signs first, advances Clock, then submits:

| Probe, both roles and normal/recovery modes | Observed behavior |
|---|---|
| No clock advance, four cases | Accepted |
| Advance 1/30/300 seconds, twelve cases | `ProposalClock`; policy/key/mint/proposal unchanged |
| Edit signed timestamp, four cases | Signature failure; state unchanged |
| Re-sign new exact timestamp, four cases | Accepted at that exact time |
| Fresh blockhash/signatures reuse consumed nonce, four cases | Canonical proposal already exists; state unchanged |

Three Rust tests submit 32 target proposal transactions: 12 successes and 20
expected refusals, excluding setup and fee airdrops. This PREPARED-policy probe
injects program/mint, controls Clock and deliberately retains a valid local
blockhash across the delay. It does not transfer research funds or repeat the
native-loader financial rehearsal. The 300-second controlled Clock jump isolates
the application check; it does not prove a real blockhash lives that long.

Solana signatures cover the transaction message; delivery/confirmation happen
after signing, and blockhash expiration is a separate constraint. See the
[official transaction reference](https://solana.com/docs/core/transactions) and
[confirmation guide](https://solana.com/developers/cookbook/transactions/confirmation).

## Selected TEST_ONLY semantics

| Option | Consequence | Decision |
|---|---|---|
| Exact-clock equality | Reproduced cross-second failure | Keep as frozen v5 counterexample |
| Accept old creation time and start notice there | Withholding consumes notice before on-chain visibility | Reject |
| Chain chooses start with no admission deadline | Full notice, no application bound on withholding | Reject for this candidate |
| Signed bounded interval; chain chooses actual start | Delivery tolerance, bounded withholding, full notice | Selected for next isolated candidate |

Replace the signed `created_at` argument with `valid_from` and `valid_until`:

```text
0 <= valid_until - valid_from <= MAX_SUBMISSION_WINDOW
valid_from <= actual_chain_clock <= valid_until
accepted_at   := actual_chain_clock
created_at    := accepted_at
execute_after := accepted_at + CHANGE_NOTICE
expires_at    := execute_after + EXECUTION_WINDOW
```

Both admission endpoints are inclusive. Execution starts at `execute_after`
and ends strictly before `expires_at`; explicit expiry cleanup starts at
`expires_at`. The frozen model uses maximum interval **300 seconds**, notice
**90 days**, and execution window **30 days**, all TEST_ONLY values. No production
rights or parameters are selected here.

The model uses nonnegative i64 times and checks the latest possible start:
`valid_until <= I64_MAX - NOTICE - EXECUTION_WINDOW`. A future scheduled interval
is allowed, with early admission rejected. Actual human signing time is not
observable or asserted. Zero-width intervals are valid but retain exact-clock
fragility. Every valid admission gets the entire notice, even at the final
allowed second; a relayer cannot backdate the start.

Every required signer approves one program/policy identity, role, epoch, next
nonce, predecessor, successor, normal/recovery mode and both bounds. The frozen
constants are in the model's domain-separated digest. Any changed interval
requires fresh acceptance by all required current/guardian and successor keys.
The fee payer cannot edit or extend it under old signatures.

Current-key/2-of-3 authority, successor consent, role pause, cancellation,
expiry, permanent nonce use and known-key recipient exclusions retain their
E-06 semantics. Competing intents share one canonical role/nonce account, so
only one can be admitted. Cancelled/expired proposals retain consumed nonces;
a new nonce receives a new full notice.

There is no pre-admission cancellation operation: an already signed intent
can still be used within its interval while its nonce and epoch remain current.
Adding revocation before admission would require a separate rights design.

## Model and evidence boundary

`src/submission_window_model.py` wraps frozen E-06 authority logic. Immutable
`Intent` and `Admission` records separate signed bounds from actual start.
Execution/cancellation/expiry reconstruct that binding. Signer-to-digest maps
are symbolic attestations, not Ed25519 verification. Program/policy names are
symbolic; no real genesis check or public-chain provenance is implemented.

Twenty-one Python tests cover 16 role/mode/delay combinations, both deadline
boundaries, 128 signer subsets (32 accepted), every intent field, partial
re-signing, invalid/future/overflow times, races, cancelled/expired nonce reuse,
stale epochs, full notice, expiry boundaries, pause/cancel rules and tampered
reconstruction. Preserving opaque financial bytes and approvals is not a new
contract's token-transfer accounting proof.

```bash
bash tools/run_e09_local_reproduction.sh
```

The command builds both pinned v5 profiles, checks E-07 hashes, runs three SBF
counterexample tests and 21 Python tests, and verifies `target/e09-probe/`
receipts. `K4V_E09_SKIP_BUILD=1` only reuses SBF after mandatory hash verification.
CI performs fresh builds. E-07/E-08 frozen inputs remain unchanged; the v5
package only gains a separate integration test.

## E-10 implementation acceptance

Implement in an isolated candidate with admission disabled by default:

1. Use distinct program/account namespaces and identity domain, binding the
   maximum submission window and fixed notice/execution constants.
2. Replace one i64 instruction time with two i64 bounds. Store both alongside
   actual creation/notice/expiry. Extending the 156-byte v5 proposal adds 16
   bytes; final generated ABI/IDL and identity vectors must confirm the layout.
3. Check role/nonce/epoch/predecessor/successor, interval and overflow atomically.
   Start full notice from runtime Clock. Refusal preserves policy/proposal/key/
   custody state, apart from transaction fees charged to the payer.
4. Submit already signed messages after a controlled delay to prove the repair.
   Cover both roles/modes, exact bounds and +1 second rejection, missing/changed
   signatures, stale epochs, nonce races/cancel/expiry. Test blockhash lifetime
   and application interval separately; simulation does not guarantee landing.
5. Repeat partial withdrawal -> delayed recovery -> continued withdrawal ->
   annual boundary on new SBF, preserving principal, T0, used budgets, original
   Treasury announcements and known-key self-payment exclusions.
6. Update independent raw decoding and complete-history export for the new
   identity/layout; reconstruct signed bounds and actual start, pin exact new
   program bytes, and publish reproducible evidence for review.

A client must never replace a blockhash or instruction under old signatures.
After expiry, re-read state and collect signatures on a fresh message; durable
nonce transport still has to satisfy the application interval. Operational
delivery/confirmation, public-network evidence, human review and production
rights remain separate unfinished work.
