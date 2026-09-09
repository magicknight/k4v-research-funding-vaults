#!/usr/bin/env bash
set -euo pipefail
k4v_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$k4v_root"
k4v_native_target="${K4V_NATIVE_TARGET_DIR:-/tmp/k4v-e09-native-target}"
k4v_sbf_target="${K4V_SBF_TARGET_DIR:-/tmp/k4v-e09-sbf-target}"
test "$(realpath -m "$k4v_native_target")" != "$(realpath -m "$k4v_sbf_target")"
python3 tools/verify_e09_inputs.py
if [[ "${K4V_E09_SKIP_BUILD:-0}" != 1 ]]; then
  for k4v_profile in disabled test; do
    k4v_features=()
    if [[ "$k4v_profile" == test ]]; then k4v_features=(--features test-profile); fi
    k4v_log="/tmp/k4v-e09-${k4v_profile}.log"
    CARGO_TARGET_DIR="$k4v_sbf_target" NO_DNA=1 cargo build-sbf --tools-version v1.52 \
      --manifest-path programs/launch-vault-v5/Cargo.toml --sbf-out-dir "target/v5-${k4v_profile}" \
      -- --locked "${k4v_features[@]}" 2>&1 | tee "$k4v_log"
    python3 - "$k4v_log" <<'PY'
import sys
from pathlib import Path
log = Path(sys.argv[1]).read_text().lower()
assert all(s not in log for s in ('stack offset', 'overwrites values', 'error:')), 'SBF diagnostic gate'
PY
  done
fi
python3 tools/verify_e07_artifacts.py
export CARGO_TARGET_DIR="$k4v_native_target"
export K4V_E09_PROBE_OUT_DIR="$k4v_root/target/e09-probe"
cargo fmt -p launch-vault-v5 -- --check
cargo clippy -p launch-vault-v5 --test e09_clock_probe --features test-profile --locked -- -D warnings
cargo test -p launch-vault-v5 --test e09_clock_probe --features test-profile --locked
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_submission_window_model.py' -v
python3 tools/verify_e09_receipts.py "$K4V_E09_PROBE_OUT_DIR"
echo 'E-09 v5 timing counterexample and bounded-window model PASS; new SBF repair remains E-10'
