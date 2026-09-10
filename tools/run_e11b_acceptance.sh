#!/usr/bin/env bash
# TEST_ONLY acceptance: no public-chain RPC, wallet export or deployment.
# Pinned toolchain and dependency builds may use network downloads.
set -euo pipefail
cd "$(dirname "$0")/.."
for command in cargo cargo-build-sbf solana-test-validator node python3; do
  command -v "$command" >/dev/null || { echo "Missing prerequisite: $command" >&2; exit 1; }
done
[[ -f spec/LAUNCH_V7_BUILD_IDENTITY_v1.json ]] || { echo 'Missing frozen build identity' >&2; exit 1; }
solana-test-validator --version | grep -F '3.1.10' >/dev/null
mkdir -p target/e11b
sha256sum -c SHA256SUMS
sha256sum -c E11B_SHA256SUMS
python3 tools/verify_e11b_archive.py
node tools/verify_e11b_archived_signatures.mjs
cargo fmt --manifest-path candidates/launch-vault-v7/Cargo.toml -- --check
for profile in disabled test; do
  features=()
  if [[ "$profile" == test ]]; then features=(--features test-profile); fi
  CARGO_TARGET_DIR="${TMPDIR:-/tmp}/k4v-e11b-sbf" NO_DNA=1 cargo build-sbf --tools-version v1.52 \
    --manifest-path candidates/launch-vault-v7/Cargo.toml --sbf-out-dir "target/v7-$profile" \
    -- --locked "${features[@]}" 2>&1 | tee "target/e11b/build-$profile.log"
  python3 - "target/e11b/build-$profile.log" <<'PY'
import sys
from pathlib import Path
text = Path(sys.argv[1]).read_text().lower()
assert not any(s in text for s in ('stack offset', 'overwrites values', 'error:')), 'SBF_DIAGNOSTICS'
PY
done
python3 tools/e11b_pin_build.py > target/e11b/build-identity.json
export CARGO_TARGET_DIR="${TMPDIR:-/tmp}/k4v-e11b-native"
export K4V_E11B_REHEARSAL_OUT="$PWD/target/e11b/raw-rehearsal.json"
export K4V_E11B_PROBE_OUT_DIR="$PWD/target/e11b/probes"
python3 tools/build_launch_v7_idl.py
cargo clippy --manifest-path candidates/launch-vault-v7/Cargo.toml --all-targets --features test-profile --locked -- -D warnings
cargo test --manifest-path candidates/launch-vault-v7/Cargo.toml --features test-profile --locked 2>&1 | tee target/e11b/rust-tests.log
node --test probes/launch_v7_identity.test.mjs clients/launch_v7_local_client.test.mjs clients/launch_v7_bootstrap.test.mjs 2>&1 | tee target/e11b/js-tests.log
python3 tools/verify_e11b_probes.py target/e11b/probes
python3 tools/pack_e11b_rehearsal.py target/e11b/raw-rehearsal.json target/e11b/fresh-bundle.json
PYTHONPATH=src python3 src/e11b_verifier.py target/e11b/fresh-bundle.json > target/e11b/financial-verification.json
PYTHONPATH=src python3 tools/e11b_verify_tests.py 2>&1 | tee target/e11b/python-tests.log
python3 tools/e11b_recorded_rpc.py --bundle target/e11b/fresh-bundle.json --output target/e11b/loopback-replay.json
node tools/e11b_agave_rehearsal.mjs
sha256sum -c SHA256SUMS
sha256sum -c E11B_SHA256SUMS
python3 tools/e11b_pin_build.py
git diff --exit-code -- candidates/launch-vault-v7 idl/launch_vault_v7.json spec/LAUNCH_V7_BUILD_IDENTITY_v1.json
printf 'E11B_LOCAL_ACCEPTANCE_PASS; public deployment, natural 90/180-day completion and human review remain OPEN\n'
