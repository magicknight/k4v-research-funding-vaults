# E-11A: bounded local client and actual Agave RPC

Status: implementation under validation. This tranche connects the frozen E-10
v6 SBF to Solana CLI / Agave 3.1.10. It does not change the accepted ABI or the
frozen E-10 bytes. CI must pass before this engineering tranche is accepted.

The client signs one immutable transaction, checks its Ed25519 signatures and
reviewed message hash, rechecks the pinned local genesis, program bytes, policy,
authority epoch, nonce and runtime Clock, simulates the exact signed bytes, sends
those same bytes and reconciles their signature through finalized status.
Expired admission intent or blockhash requires rebuilding and signing again.
An unresolved send remains UNKNOWN; it never automatically re-signs.

## Newly exposed bootstrap limit

With six independent creator / Founder / Treasury / three recovery signatures,
v6 open_policy has 564 instruction bytes. The legacy packet is 1,352 bytes;
even one ideal v0 address lookup table leaves 1,264 bytes. Both exceed the
1,232-byte packet budget. This is a real integration blocker which the earlier
in-process LiteSVM execution did not establish. Adding a compute-budget
instruction increases the size. Coalescing authorities is not a repair for the
independent-signer configuration.

The local rehearsal explicitly uses the existing single-operator profile
(creator = Founder = Treasury, plus three distinct recovery keys): four
signatures and a 1,160-byte bootstrap. This demonstrates the client and actual
RPC path only for that profile. Independent-role bootstrap remains blocked.

## Reproduction and evidence boundaries

Run from a fresh checkout with Node 20, Python 3.12 and Solana CLI 3.1.10:

    npm ci --ignore-scripts
    node --test clients/launch_v6_local_client.test.mjs
    NO_DNA=1 cargo build-sbf --tools-version v1.52 --manifest-path candidates/launch-vault-v6/Cargo.toml --sbf-out-dir target/v6-test -- --locked --features test-profile
    node tools/e11_agave_rehearsal.mjs

The new CI workflow builds and pins the exact SBF and saves JSON observations,
signed transaction bytes (public data), receipts and validator logs as artifacts.
The program is an immutable genesis fixture. Mint, token accounts, policy,
deposits, arm/activate, recovery proposal and cancellation use actual signed
transactions. Test SOL comes from the local faucet. The client never writes its
ephemeral private keys. Agave's own temporary ledger is deleted and excluded
from artifacts.

The read-only E-10 Python exporter independently decodes actual node responses,
including the immutable program bytes, complete proposal/key history and one
finalized response for the full account graph. This is no longer recorded HTTP
response replay. The expected genesis is captured from the newly started local
node, so it is a local test binding, not an external chain identity proof.

The rehearsal covers a naturally delayed signed recovery, full 90-day notice
from actual admission, premature execution refusal, successor cancellation,
stale-nonce refusal, admission-window expiry while its blockhash remains valid,
natural blockhash expiry, and unchanged custody after refused attempts.
It does not wait 90 / 180 days or override Clock, so successful long-duration
recovery and post-cliff withdrawal on Agave remain unproven by this tranche.

## Next work

E-11B must repair independent-role bootstrap in a NEW isolated candidate, with
packet-budget tests for every instruction and full signer layout. Do not alter
v6's frozen ABI in place. A compact consent/open instruction bound to a prepared,
immutable configuration is a design option to review before implementation.
Then rerun independent-role client/RPC setup and the long-duration recovery /
continued-withdrawal path with explicit clock and runtime evidence boundaries.

Human review, production parameter adoption and public deployment remain open.
F-02 feedback and its F-03 dependent work remain paused and demand unverified.
