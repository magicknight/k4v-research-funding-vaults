# E11C: actual-validator continuation from explicitly preloaded mature fixtures

Status: IMPLEMENTATION IN PROGRESS / NO ACCEPTANCE CLAIM.
Parent: accepted E11B PR #26, main 376e72069cb99a5b1b21c3fcfff95b773395388f.
The parallel E11B draft #27 is superseded and must not replace the frozen v7.

This tranche targets actual Agave execution after long contractual intervals,
without pretending that a CI job naturally elapsed 90 or 180 days. Two separate
local-validator runs start from explicitly preloaded, complete application-state
fixtures constructed by the existing signed native-loader financial rehearsal.
Only an isolated copy of the test fixture's START time and named insecure test
keys is parameterized. Program source, SBF, identity rules, periods, quotas,
notice lengths, upgrade authorities and all production inputs remain unchanged.

Scenario R preloads the point immediately before executing two mature recovery
proposals. Agave must execute both proposals with signed, finalized transactions,
then accept Founder and Treasury withdrawals by the new keys. Original keys,
stale epochs, old destinations and excessive withdrawals must be rejected.
Scenario Y preloads an expired pending proposal and an existing year-two budget.
Agave must reject late execution, expire the proposal, accept a fresh oracle
report, enforce year-two accounting and permit bounded continued withdrawals.

Raw accounts are exported through the frozen full-history v7 RPC exporter and
independently decoded. Authority changes must preserve principal, T0, identity,
budgets and used counters; only authorized releases may move tokens. All setup
account bytes, origins, signatures, code hashes and differences are recorded.

Evidence labels are mandatory: application_state_preloaded=true;
fixture_clock_controlled=true; actual_validator_clock_override=false;
natural_90_180_day_soak=false; independent_human_governance=false.
The two validator scenarios are separate fixtures, not one uninterrupted history.
No public RPC, public deployment, real funds, website change or outreach occurs.
Human security review, production choices and launch/funding gates remain open.

Solana's documented account-file mechanism is used for preload:
https://solana.com/developers/cookbook/development/using-mainnet-accounts-programs
