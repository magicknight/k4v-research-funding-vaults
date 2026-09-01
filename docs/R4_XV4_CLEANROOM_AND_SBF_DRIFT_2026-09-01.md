# R4 xv4 clean-room run and frozen-SBF drift

> State: `FUNCTIONAL REPRODUCTION PASS / BYTE BINDING FAIL FOUND / LOCAL REPAIR PASS`
>
> No public Solana transaction, production key, official mint or mainnet
> authorization was used.

## Isolated run

A Codex subagent controlled by the same user started from a new directory on
TPU VM `xv4`, a host with no prior K4V clone or Solana/Rust/Node toolchain. It
cloned only the public repository at
`e1afead138fbf56956b298ebae7a97a8ae9ad956`, installed checksummed user-local
tools and ran:

```text
NO_DNA=1 bash tools/run_r3_local_reproduction.sh
```

The source was clean before and after. The command exited zero and reported:

```text
transaction_rpc_valid      true
squads_valid               true
read_only_verifier_valid   true
supply                     1000000000000000000
conserved_total            1000000000000000000
policy_released_this_period 3000000000000000
```

Environment: Node 20.20.2, npm 10.8.2, host Rust/Cargo 1.98.0, Anza/Solana CLI
3.1.10, SBF Rust 1.89 under platform-tools v1.52, and Surfpool 1.5.0. The Surfpool archive matched
the pinned SHA-256 `5b20a3b4...d15887ddc`. The full transferred evidence bundle
passed its own top-level manifest on xv4 and again after download.

The run is classified
`OPERATIONALLY_ISOLATED_SAME_USER_CODEX_SUBAGENT_NOT_UNRELATED_HUMAN`. It closes
an environment-isolation probe, not the human-accountable review gate.

## Smallest failed node

The runner built SBF SHA-256
`2584dbb17dc6785344690ed168e5872041e9f5342ad5b08076e392184dbf1c49`,
while `spec/R3_TEST_ONLY_CANDIDATE_v1.json` freezes
`081b6c166fa63bac07abfa026f4c16f3c1eeb2d480e09a8912c65b5b2aea8bcb`.
The original runner recorded both artifacts but never compared them, so it
incorrectly emitted a top-level `PASS` despite byte-identity failure.

## Exact cause

Cold builds with the same v1.52 platform tools reproduce both hashes solely by
source state:

- parent `2c9fe5d67d3bc861774d142cbd5227791afddebf` -> frozen `081b6c...`;
- `e1afead` -> `2584db...`;
- `e1afead` with only the corrected oracle comment compressed from three lines
  back to two -> frozen `081b6c...`, byte-identical.

Commit `e1afead` repaired a stale oracle comment by expanding it from two source
lines to three. Anchor/Rust retains source locations in generated error/event
metadata. The two 367,264-byte programs differ at exactly six bytes: five SBF
source-line immediates increment by one and one `.data.rel.ro` location changes
from line 115 to 116. No state-transition logic changed. Platform-tools v1.51
is excluded: its Rust 1.84.1 cannot compile the locked dependency graph.

## Repair

The live repair preserves the corrected meaning in two comment lines, pins
`--tools-version v1.52` plus Cargo `--locked`, fails immediately unless the
built SBF hash equals the candidate, checks both transaction and Squads receipts
against that hash, and emits expected/observed hashes plus
`sbf_byte_reproducible` in `RESULT.json`. The same binding is added to CI.

A post-repair local full reproduction passed with both hashes equal to
`081b6c...` and `sbf_byte_reproducible=true`. A new public-only xv4 clean clone
of the repair commit remains the publication acceptance test.

## Artifact hashes

```text
initial transferred bundle manifest  1162d6f48133a5683853379a46da421237c3345a6388e2ab368dc5ed3e087bd5
initial run transcript                14c813e74021deb52e9a0b76172fe24025d615f9524c78e4081dd73e3104e89c
initial built SBF                     2584dbb17dc6785344690ed168e5872041e9f5342ad5b08076e392184dbf1c49
frozen and repaired SBF               081b6c166fa63bac07abfa026f4c16f3c1eeb2d480e09a8912c65b5b2aea8bcb
```

`TARGET_GATE: OPEN`  
`R4A: OPERATIONAL ISOLATION FUNCTIONAL PASS / REPAIRED BYTE REPLAY LOCAL PASS`  
`R4B: HUMAN-ACCOUNTABLE SECURITY REVIEW OPEN`
