#!/usr/bin/env bash
# Builds and local fixtures only. Never connects to a public RPC or submits there.
set -euo pipefail
k4v_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$k4v_root"
python3 tools/verify_e08_review_scope.py
if [[ "${K4V_E08_TRANSPORT_ONLY:-0}" == 1 ]]; then
  PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_launch_v5_rpc_exporter.py' -v
else
  bash tools/run_e07_local_reproduction.sh
fi
mkdir -p target
python3 tools/e08_recorded_rpc.py --output target/e08-loopback-replay.json
echo 'E-08 read-only export and exact-candidate review handoff PASS; human acceptance remains OPEN'
