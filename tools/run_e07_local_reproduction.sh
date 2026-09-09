#!/usr/bin/env bash
# Local signed transactions only; no RPC endpoint, wallet file or deployment.
set -euo pipefail
k4v_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$k4v_root"
k4v_native_target="${K4V_NATIVE_TARGET_DIR:-/tmp/k4v-e07-native-target}"
k4v_sbf_target="${K4V_SBF_TARGET_DIR:-/tmp/k4v-e07-sbf-target}"
if [[ "$(realpath -m "$k4v_native_target")" == "$(realpath -m "$k4v_sbf_target")" ]]; then
  echo 'Native and SBF targets must be separate'
  exit 1
fi
if [[ "${K4V_E07_SKIP_BUILD:-0}" != 1 ]]; then
  for k4v_profile in disabled test; do
    k4v_features=()
    if [[ "$k4v_profile" == test ]]; then k4v_features=(--features test-profile); fi
    k4v_log="/tmp/k4v-e07-${k4v_profile}.log"
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
export K4V_E07_REHEARSAL_OUT="$k4v_root/target/e07-fresh-raw.json"
cargo fmt -p launch-vault-v5 -- --check
cargo clippy -p launch-vault-v5 --all-targets --features test-profile --locked -- -D warnings
cargo test -p launch-vault-v5 --features test-profile --locked
python3 tools/build_launch_v5_idl.py --check
node --test probes/launch_v5_identity.test.mjs
python3 tools/pack_e07_rehearsal.py "$K4V_E07_REHEARSAL_OUT" target/e07-fresh-bundle.json
PYTHONPATH=src python3 src/e07_verifier.py target/e07-fresh-bundle.json > target/e07-fresh-verification.json
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v5*.py' -v
echo 'E-07 local SBF recovery, financial continuity and independent raw verification PASS'
