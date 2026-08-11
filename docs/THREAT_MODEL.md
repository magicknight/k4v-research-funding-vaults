# Threat model

The protected property is not price. It is that no actor can cause more
economic circulation than the disclosed covenant permits.

| Threat | Smallest dependency | v0.1 status | Required repair or test |
|---|---|---|---|
| Direct sale above cap | Release arithmetic | ESTABLISHED IN MODEL | Port exact integer semantics on chain |
| OTC/grant/free transfer bypass | Event classification | ESTABLISHED IN MODEL | Bind every program release path to one counter |
| Collateral or economic-right bypass | Off-chain interpretation plus event input | MODELLED / ORACLE OPEN | Define attestations and conservative default |
| Fake or wash-traded volume | Eligible-volume oracle | OPEN | Source allowlist, related-party exclusion, stale-data fail-close |
| Budget invented by beneficiary | Approval authority | OPEN | Conflict-aware multisig and public approval receipt |
| Upgrade weakens covenant | Program/config authority | OPEN | Immutability or monotonic constraints plus time lock |
| Emergency power accelerates release | Emergency instruction set | OPEN | Pause-only property tests |
| Signer loss or collusion | Multisig operations | OPEN | Recovery design that cannot lower threshold silently |
| Hash/version mismatch | Deployment provenance | OPEN | Reproducible build and on-chain configuration digest |
| Hidden mint or freeze power | SPL mint state | LOCALNET EVIDENCE | Repeat on public testnet and reconcile independently |

The main failure mode of the champion is an on-chain path that bypasses the
single release counter. The independent verifier route avoids custody and can
still expose discrepancies, but it cannot prevent them.
