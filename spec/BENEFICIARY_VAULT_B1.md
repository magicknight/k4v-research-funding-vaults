# Beneficiary Vault B1

Status: IMPLEMENTED PROTOTYPE / PUBLIC CI + LOCAL TRANSACTION RPC VALIDATED

B1 is the smallest custody mechanism that can replace a beneficiary's verbal
promise to release slowly. It is intentionally narrower than the full
purpose-bound covenant: one beneficiary, one SPL mint, one initial deposit,
one frozen cliff, one frozen rate, and one shared release counter.

## Protected property

For every successful `release(amount)` transaction:

~~~text
now >= cliff_end_ts
released_this_period + amount <= monthly_cap
released_total + amount <= deposited_amount

monthly_cap = floor(floor(deposited_amount * annual_release_bps / 10000) / 12)
period_index = floor((now - cliff_end_ts) / 2592000)
~~~

`annual_release_bps` is frozen between 1 and 500. The cliff is at least
63,072,000 seconds (730 days). Periods are fixed 2,592,000-second windows
(30 days) beginning at `cliff_end_ts`. When the period index increases,
`released_this_period` resets to zero; unused capacity is never added to a
later period.

The fixed-second convention is not being represented as a calendar-month or
calendar-year implementation. That semantic bridge remains open.

## PDA graph

~~~text
state = PDA(
  "beneficiary-vault",
  beneficiary,
  mint,
  policy_hash,
  program_id
)

token_vault = PDA("beneficiary-token", state, program_id)
token_vault.authority = state
~~~

`policy_hash` is a non-zero 32-byte digest chosen by the depositor. It binds a
vault address to an externally published policy artifact. B1 checks the hash
shape and freezes it; it does not decide whether the referenced prose is true
or legally effective.

## Instruction surface

### `deposit(amount, annual_release_bps, cliff_seconds, policy_hash)`

- creates both PDAs exactly once;
- rejects zero amount, zero policy hash, a rate above 500 bps, or a cliff below
  730 days;
- computes and freezes the monthly cap;
- transfers `amount` base units from the depositor's token account into the
  PDA-owned token vault;
- records the depositor, beneficiary, mint, timestamps, bumps, and mint
  decimals.

Because state creation uses `init` and there is no top-up instruction, the
initial balance is unambiguous in B1.

### `release(amount)`

- requires the frozen beneficiary to sign;
- permits a destination token account only when its owner is that beneficiary
  and its mint matches the frozen mint;
- rejects calls before the cliff;
- updates one counter shared by every B1 release path;
- rejects a period overflow or lifetime deposit overflow;
- signs the SPL transfer with the state PDA.

There are no update, configure, close, migrate, emergency-release, alternate
destination, or administrative transfer instructions.

## Immutability boundary

The source contains no configuration path capable of shortening the cliff or
raising the cap. That does **not** by itself make an upgradeable deployment
immutable: a deployment can replace program logic if an upgrade authority
exists. Any future public deployment must disclose the loader state and either
use a non-upgradeable deployment or verifiably revoke the upgrade authority
before claiming that the covenant is immutable.

The program ID in this repository is a test identity for reproducible local VM
loading. It is not a mainnet or devnet deployment address.

## Independent verification

`src/beneficiary_vault_verifier.py` takes an exported snapshot and independently
recomputes:

- both PDA addresses and bumps;
- the monthly cap and minimum cliff;
- period ordering and currently releasable amount;
- beneficiary-vault token mint and authority bindings;
- any custody deficit below `deposited_amount - released_total`;
- unsolicited token surplus separately, without increasing release entitlement.

It uses only the Python standard library and produces a deterministic SHA-256
receipt. `src/beneficiary_vault_rpc_exporter.py` now fetches and authenticates
raw state, token, and Clock accounts through read-only JSON-RPC. It checks
owners, exact lengths, discriminator, canonical PDAs, and token delegate,
close-authority, native and frozen fields before passing the snapshot here.
The deterministic live Surfpool fixture injects bytes. A separate loopback-only
probe now creates mint and token accounts, deposit state, and one exact-cap
release through signed transactions with in-memory signers, then passes those
raw RPC accounts through the same exporter and verifier. The program itself is
loaded with `surfnet_writeProgram`, so real loader deployment remains open.

## B1 acceptance

B1 is accepted only when all of the following pass against the compiled SBF
artifact:

1. one-time deposit creates the expected state and token PDAs and transfers the
   full deposit;
2. release before the cliff is rejected without changing balances;
3. an exact-cap release succeeds after the cliff;
4. one additional base unit in the same period is rejected;
5. a later period permits only a fresh cap, not accumulated unused capacity;
6. the Rust and Python PDA vectors agree;
7. the independent verifier rejects tampered cap, cliff, bump, and token
   conservation snapshots;
8. the opt-in loopback probe simulates every signed transaction, observes the
   expected pre-cliff failure, sends setup/deposit/release, and reconstructs
   the resulting raw RPC state with the independent verifier.

Security review, real loader deployment, public-testnet deployment, program
immutability, purpose approvals, market-capacity input, and legal analysis are
not B1 acceptance claims.
