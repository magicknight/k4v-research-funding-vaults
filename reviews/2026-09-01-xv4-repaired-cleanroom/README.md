# xv4 repaired public clean-clone reproduction

This directory preserves the repaired rerun performed on TPU VM `xv4` from a
new public clone of commit `173c8561c99c1ed10b29f7bdde65579e14c39fe6`.
The source directory and build outputs from the first run were not reused. The
already checksummed user-local toolchain was reused so the repair test could
focus on source and acceptance behavior.

Result:

```text
exit_code                  0
transaction_rpc_valid      true
squads_valid               true
read_only_verifier_valid   true
sbf_byte_reproducible      true
expected_sbf_sha256        081b6c166fa63bac07abfa026f4c16f3c1eeb2d480e09a8912c65b5b2aea8bcb
observed_sbf_sha256        081b6c166fa63bac07abfa026f4c16f3c1eeb2d480e09a8912c65b5b2aea8bcb
supply                     1000000000000000000
conserved_total            1000000000000000000
policy_released_this_period 3000000000000000
```

No public Solana transaction, production key, official mint or mainnet action
was used. The fresh clone was clean after the run. The command ran entirely on
local Surfpool ledgers; the Squads leg fetched a public devnet program into its
local fork.

Classification:

`REPAIRED_PUBLIC_CLEAN_CLONE_SAME_USER_TPU_RERUN`

This establishes operational environment isolation and repaired byte-level
reproducibility. It is still controlled by the same user and executed through a
Codex workflow, so it is not an unrelated human reproduction or a
human-accountable security audit.

`RUN_TRANSCRIPT.txt` is the terminal transcript with ANSI control sequences
removed. `REPRODUCTION_SHA256SUMS.txt` is the runner's original manifest; its
absolute paths name the remote xv4 run. This directory's own
`MANIFEST_SHA256.txt` is directly verifiable after cloning.
