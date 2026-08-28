#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# One-command R3 local reproduction. No public transaction or persistent key.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
offline_port="${K4V_R3_OFFLINE_PORT:-19399}"
offline_ws_port="${K4V_R3_OFFLINE_WS_PORT:-19400}"
squads_port="${K4V_R3_SQUADS_PORT:-19199}"
squads_ws_port="${K4V_R3_SQUADS_WS_PORT:-19200}"
run_stamp="$(date -u +%Y%m%dT%H%M%SZ)"
out_dir="${K4V_R3_OUT_DIR:-$repo_root/target/r3-reproduction/$run_stamp}"
candidate="$repo_root/spec/R3_TEST_ONLY_CANDIDATE_v1.json"
offline_pid=""
squads_pid=""

cleanup() {
  if [[ -n "$offline_pid" ]]; then kill "$offline_pid" 2>/dev/null || true; fi
  if [[ -n "$squads_pid" ]]; then kill "$squads_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT INT TERM

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 2
  }
}

for command_name in cargo cargo-build-sbf curl jq node npm sha256sum surfpool; do
  need "$command_name"
done

if curl --fail --silent "http://127.0.0.1:$offline_port" >/dev/null 2>&1; then
  echo "offline RPC port $offline_port is already occupied" >&2
  exit 2
fi
if curl --fail --silent "http://127.0.0.1:$squads_port" >/dev/null 2>&1; then
  echo "Squads RPC port $squads_port is already occupied" >&2
  exit 2
fi

mkdir -p "$out_dir"
cd "$repo_root"

jq -e '
  .artifact_status == "PUBLIC_TEST_BINDING_NOT_PRODUCTION" and
  .mainnet_authorized == false and
  .official_mint == null and
  .open_parameter_count == 26 and
  .test_vector.cluster == "local_surfpool_only"
' "$candidate" >/dev/null

NO_DNA=1 cargo build-sbf \
  --manifest-path programs/purpose-vault/Cargo.toml \
  --sbf-out-dir target/deploy
NO_DNA=1 cargo fmt --all -- --check
NO_DNA=1 cargo clippy --workspace --all-targets --locked -- -D warnings
NO_DNA=1 cargo test --package purpose-vault --locked
npm ci --ignore-scripts
npm run check
npm audit --omit=dev

NO_DNA=1 surfpool start --ci --offline --no-deploy \
  --host 127.0.0.1 --port "$offline_port" --ws-port "$offline_ws_port" \
  >"$out_dir/offline-surfpool.log" 2>&1 &
offline_pid="$!"
for _ in $(seq 1 60); do
  if curl --fail --silent -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' \
    "http://127.0.0.1:$offline_port" | grep -q '"ok"'; then
    break
  fi
  sleep 0.5
done
curl --fail --silent -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' \
  "http://127.0.0.1:$offline_port" | grep -q '"ok"'

K4V_SURFPOOL_RPC="http://127.0.0.1:$offline_port" \
K4V_LOCAL_TRANSACTION_SEND_CONFIRMED=1 \
K4V_R3_CANDIDATE_CONFIG="$candidate" \
K4V_R3_RECEIPT_OUT="$out_dir/r3-transaction-rpc.json" \
NO_DNA=1 cargo run --locked --package purpose-vault \
  --example r3_full_scale_rpc_probe \
  >"$out_dir/r3-transaction-rpc.log" 2>&1
jq -e '
  .valid == true and
  .mainnet_authorized == false and
  .candidate_config.open_parameter_count == 26 and
  .mint.supply == "1000000000000000000" and
  .mint.mint_authority == null and
  .mint.freeze_authority == null and
  .reconstruction.conserved_total == "1000000000000000000" and
  .reconstruction.policy_released_this_period == "3000000000000000"
' "$out_dir/r3-transaction-rpc.json" >/dev/null

NO_DNA=1 surfpool start --ci --network devnet --no-deploy \
  --host 127.0.0.1 --port "$squads_port" --ws-port "$squads_ws_port" \
  >"$out_dir/squads-surfpool.log" 2>&1 &
squads_pid="$!"
for _ in $(seq 1 60); do
  if curl --fail --silent -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' \
    "http://127.0.0.1:$squads_port" | grep -q '"ok"'; then
    break
  fi
  sleep 0.5
done
curl --fail --silent -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' \
  "http://127.0.0.1:$squads_port" | grep -q '"ok"'

K4V_SURFPOOL_RPC="http://127.0.0.1:$squads_port" \
K4V_R3_CANDIDATE_CONFIG="$candidate" \
K4V_R3_SQUADS_RECEIPT_OUT="$out_dir/r3-squads.json" \
NO_DNA=1 node probes/r3_full_scale_squads_probe.mjs \
  >"$out_dir/r3-squads.log" 2>&1
K4V_SURFPOOL_RPC="http://127.0.0.1:$squads_port" \
K4V_R3_SQUADS_RECEIPT_IN="$out_dir/r3-squads.json" \
K4V_R3_SQUADS_VERIFY_OUT="$out_dir/r3-squads-verified.json" \
NO_DNA=1 node probes/r3_full_scale_squads_verify.mjs \
  >"$out_dir/r3-squads-verified.log" 2>&1

jq -e '.valid == true and .checks.unsafe_javascript_number_rejected_before_encoding == true' \
  "$out_dir/r3-squads.json" >/dev/null
jq -e '.valid == true and ([.checks[]] | all)' \
  "$out_dir/r3-squads-verified.json" >/dev/null

sha256sum \
  "$candidate" \
  target/deploy/purpose_vault.so \
  "$out_dir/r3-transaction-rpc.json" \
  "$out_dir/r3-squads.json" \
  "$out_dir/r3-squads-verified.json" \
  >"$out_dir/SHA256SUMS"

jq -n \
  --arg schema "k4v-r3-clean-room-reproduction/v1" \
  --arg checked_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg out_dir "$out_dir" \
  --slurpfile transaction "$out_dir/r3-transaction-rpc.json" \
  --slurpfile squads "$out_dir/r3-squads.json" \
  --slurpfile verified "$out_dir/r3-squads-verified.json" \
  '{
    schema: $schema,
    checked_at: $checked_at,
    result: "PASS",
    epistemic_status: "LOCAL_REPRODUCTION_NOT_INDEPENDENT_BY_ITSELF",
    out_dir: $out_dir,
    no_official_mint: true,
    mainnet_authorized: false,
    transaction_rpc_valid: $transaction[0].valid,
    squads_valid: $squads[0].valid,
    read_only_verifier_valid: $verified[0].valid,
    supply: $transaction[0].mint.supply,
    conserved_total: $verified[0].conserved_total,
    policy_released_this_period: $verified[0].policy_released_this_period,
    next_gate: "unrelated operator transcript and procured security review"
  }' >"$out_dir/RESULT.json"

echo "R3 CLEAN-ROOM REPRODUCTION PASS"
echo "Artifacts: $out_dir"
cat "$out_dir/RESULT.json"
