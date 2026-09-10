#!/usr/bin/env bash
# Signed local transactions and loopback replay only; no wallet file or public RPC.
set -euo pipefail
k4v_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$k4v_root"
k4v_native_target="${K4V_NATIVE_TARGET_DIR:-/tmp/k4v-e10-native-target}"
k4v_sbf_target="${K4V_SBF_TARGET_DIR:-/tmp/k4v-e10-sbf-target}"
test "$(realpath -m "$k4v_native_target")" != "$(realpath -m "$k4v_sbf_target")"
python3 tools/verify_e10_artifacts.py --sources-only
if [[ "${K4V_E10_SKIP_BUILD:-0}" != 1 ]]; then
  for k4v_profile in disabled test; do
    k4v_features=()
    if [[ "$k4v_profile" == test ]]; then k4v_features=(--features test-profile); fi
    k4v_log="/tmp/k4v-e10-${k4v_profile}.log"
    CARGO_TARGET_DIR="$k4v_sbf_target" NO_DNA=1 cargo build-sbf --tools-version v1.52 \
      --manifest-path candidates/launch-vault-v6/Cargo.toml --sbf-out-dir "target/v6-${k4v_profile}" \
      -- --locked "${k4v_features[@]}" 2>&1 | tee "$k4v_log"
    python3 - "$k4v_log" <<'PY'
import sys
from pathlib import Path
log = Path(sys.argv[1]).read_text().lower()
assert all(s not in log for s in ('stack offset', 'overwrites values', 'error:')), 'SBF diagnostic gate'
PY
  done
fi
python3 tools/verify_e10_artifacts.py
export CARGO_TARGET_DIR="$k4v_native_target"
export K4V_E10_REHEARSAL_OUT="$k4v_root/target/e10-fresh-raw.json"
export K4V_E10_PROBE_OUT_DIR="$k4v_root/target/e10-probe"
cargo fmt --manifest-path candidates/launch-vault-v6/Cargo.toml -- --check
cargo clippy --manifest-path candidates/launch-vault-v6/Cargo.toml --all-targets --features test-profile --locked -- -D warnings
cargo test --manifest-path candidates/launch-vault-v6/Cargo.toml --features test-profile --locked
python3 tools/build_launch_v6_idl.py --check
node --test probes/launch_v6_identity.test.mjs
python3 tools/verify_e10_probes.py "$K4V_E10_PROBE_OUT_DIR"
python3 tools/pack_e10_rehearsal.py "$K4V_E10_REHEARSAL_OUT" target/e10-fresh-bundle.json
PYTHONPATH=src python3 src/e10_verifier.py target/e10-fresh-bundle.json > target/e10-fresh-verification.json
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v6*.py' -v
python3 tools/e10_recorded_rpc.py --bundle target/e10-fresh-bundle.json --output target/e10-fresh-loopback.json
echo 'E-10 v6 signed submission-window repair, financial continuity and raw/HTTP verification PASS'
