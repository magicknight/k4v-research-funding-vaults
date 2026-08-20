# Threat model

The protected property is not price. It is that no actor can cause more
economic circulation than the disclosed covenant permits.

| Threat | Smallest dependency | Status (covenant v0.2) | Required repair or test |
|---|---|---|---|
| Direct release above cap | Release arithmetic | B1 PUBLIC SBF + LOCAL REAL-LOADER TRANSACTION RPC EVIDENCE | Reproduce on public devnet and in an independent environment |
| Alternate on-chain release path | Instruction surface plus shared counter | B1 PUBLIC IDL/SBF EVIDENCE | Independently inspect deployed compiled interface |
| OTC/grant/free transfer after release | Event classification | ESTABLISHED IN MODEL / B1 OUT OF SCOPE | Bind future purpose and integration paths to one counter |
| Collateral or economic-right bypass | Off-chain interpretation plus event input | MODELLED / ORACLE OPEN | Define attestations and conservative default |
| Fake or wash-traded volume | Eligible-volume oracle | B2 BOUNDED / DEFINITION OPEN | An inflated report widens the shared window but cannot pass the frozen per-vault caps; a policy may freeze an absolute ceiling. Source allowlist, related-party exclusion and the base-unit aggregation remain off-chain and unspecified |
| Budget invented by beneficiary | Approval authority | OPEN | Conflict-aware multisig and public approval receipt |
| Upgrade weakens covenant | Loader authority; not B1 instruction state | OPEN | Non-upgradeable deployment or verified revocation plus artifact match |
| Emergency power accelerates release | Emergency instruction set | B1 ABSENT | Preserve two-instruction surface; test pause-only if later added |
| Unsolicited direct transfer into vault | SPL Token account is publicly creditable | B1 SAFE SURPLUS / GRIEFING VISIBLE | Keep entitlement based on frozen deposit; report surplus without granting release capacity |
| Signer loss or collusion | Multisig operations | OPEN | Recovery design that cannot lower threshold silently |
| Oracle key loss | Single frozen oracle key | OPEN — DEPOSITS LOCK | B2 has no rotation instruction and no inactivity fallback; a threshold reporter set or a fail-closed silence floor, either of which must not be able to raise a cap |
| Hash/version mismatch | Deployment provenance | OPEN | Reproducible build and on-chain configuration digest |
| Hidden mint or freeze power | SPL mint state | LOCALNET EVIDENCE | Repeat on public testnet and reconcile independently |

The main failure mode of the champion remains an on-chain path that bypasses
the single release counter. B1 removes that path in the current source and the
loaded SBF test surface by exposing exactly one release instruction. This is
local evidence, not an independent audit or public-deployment claim. The
independent verifier route avoids custody and can expose state or accounting
discrepancies. Its RPC adapter now authenticates the account envelope, owners,
binary layouts, PDAs and token escape-hatch fields. Both an injected fixture
and locally signed transaction-produced accounts now pass. A stronger probe
also installs the exact SBF through real upgradeable-loader buffer, write, and
deploy transactions before repeating the lifecycle. Because the committed
test Program ID's signer no longer exists, only its uninitialized Program
account is injected as local setup; ProgramData is loader-created and
byte-checked. The verifier still cannot prevent custody failures, and public
deployment, authority ceremony, and independent review remain open.
