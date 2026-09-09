#!/usr/bin/env bash
# Full E-05 reproduction. All transactions execute in LiteSVM, never public RPC.
set -euo pipefail
k4v_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$k4v_root"
k4v_native_target="${K4V_NATIVE_TARGET_DIR:-/tmp/k4v-e05-native-target}"
k4v_sbf_target="${K4V_SBF_TARGET_DIR:-/tmp/k4v-e05-sbf-target}"
if [[ "$(realpath -m "$k4v_native_target")" == "$(realpath -m "$k4v_sbf_target")" ]]; then
  echo 'Native and SBF build targets must be separate'
  exit 1
fi
if [[ "${K4V_E05_SKIP_BUILD:-0}" != 1 ]]; then
  for k4v_program in launch-vault-v4 upgrade-gate-v1; do
    if [[ "$k4v_program" == launch-vault-v4 ]]; then k4v_prefix=v4; else k4v_prefix=gate; fi
    for k4v_profile in disabled test; do
      k4v_features=()
      if [[ "$k4v_profile" == test ]]; then k4v_features=(--features test-profile); fi
      k4v_log="/tmp/k4v-e05-${k4v_prefix}-${k4v_profile}.log"
      CARGO_TARGET_DIR="$k4v_sbf_target" NO_DNA=1 cargo build-sbf --tools-version v1.52 \
        --manifest-path "programs/$k4v_program/Cargo.toml" \
        --sbf-out-dir "target/${k4v_prefix}-${k4v_profile}" -- --locked "${k4v_features[@]}" 2>&1 | tee "$k4v_log"
      if grep -qi 'Stack offset' "$k4v_log"; then
        echo 'SBF stack-frame overflow is a failing gate'
        exit 1
      fi
    done
  done
fi
# Even an explicitly reused build must match all frozen E-04 byte pins.
python3 tools/verify_e04_artifacts.py
export CARGO_TARGET_DIR="$k4v_native_target"
export K4V_V4_SNAPSHOT_OUT="$k4v_root/target/e05-fresh-v4.json"
export K4V_GATE_RECEIPT_OUT="$k4v_root/target/e05-fresh-loader.json"
export K4V_E05_REHEARSAL_OUT="$k4v_root/target/e05-fresh-raw-rehearsal.json"
cargo fmt --all -- --check
cargo clippy -p launch-vault-v4 -p upgrade-gate-v1 --all-targets --features test-profile --locked -- -D warnings
cargo test -p launch-vault-v4 -p upgrade-gate-v1 --features test-profile --locked
python3 tools/build_launch_v4_idl.py --check
python3 tools/build_upgrade_gate_v1_idl.py --check
node --test probes/launch_v4_identity.test.mjs
python3 tools/check_e04_loader_receipt.py "$K4V_GATE_RECEIPT_OUT"
PYTHONPATH=src python3 src/launch_v4_verifier.py "$K4V_V4_SNAPSHOT_OUT"
python3 tools/pack_e05_rehearsal.py "$K4V_E05_REHEARSAL_OUT" target/e05-fresh-bundle.json
PYTHONPATH=src python3 src/e05_verifier.py target/e05-fresh-bundle.json > target/e05-fresh-verification.json
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v4*.py' -v
echo 'E-05 local transaction rehearsal, independent supplied-byte verification and RPC boundary tests PASS'
