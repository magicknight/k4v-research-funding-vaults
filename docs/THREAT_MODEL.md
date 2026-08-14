# Threat model

The protected property is not price. It is that no actor can cause more
economic circulation than the disclosed covenant permits.

| Threat | Smallest dependency | v0.1 status | Required repair or test |
|---|---|---|---|
| Direct release above cap | Release arithmetic | B1 LOCAL SBF EVIDENCE | Reproduce in CI and independent environment |
| Alternate on-chain release path | Instruction surface plus shared counter | B1 LOCAL SBF EVIDENCE | Publish IDL and independently inspect compiled interface |
| OTC/grant/free transfer after release | Event classification | ESTABLISHED IN MODEL / B1 OUT OF SCOPE | Bind future purpose and integration paths to one counter |
| Collateral or economic-right bypass | Off-chain interpretation plus event input | MODELLED / ORACLE OPEN | Define attestations and conservative default |
| Fake or wash-traded volume | Eligible-volume oracle | OPEN | Source allowlist, related-party exclusion, stale-data fail-close |
| Budget invented by beneficiary | Approval authority | OPEN | Conflict-aware multisig and public approval receipt |
| Upgrade weakens covenant | Loader authority; not B1 instruction state | OPEN | Non-upgradeable deployment or verified revocation plus artifact match |
| Emergency power accelerates release | Emergency instruction set | B1 ABSENT | Preserve two-instruction surface; test pause-only if later added |
| Signer loss or collusion | Multisig operations | OPEN | Recovery design that cannot lower threshold silently |
| Hash/version mismatch | Deployment provenance | OPEN | Reproducible build and on-chain configuration digest |
| Hidden mint or freeze power | SPL mint state | LOCALNET EVIDENCE | Repeat on public testnet and reconcile independently |

The main failure mode of the champion remains an on-chain path that bypasses
the single release counter. B1 removes that path in the current source and the
loaded SBF test surface by exposing exactly one release instruction. This is
local evidence, not an independent audit or public-deployment claim. The
independent verifier route avoids custody and can expose state or accounting
discrepancies; until it reads raw RPC bytes itself, it still depends on snapshot
authenticity and cannot prevent custody failures.
