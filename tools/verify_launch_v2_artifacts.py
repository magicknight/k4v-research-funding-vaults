#!/usr/bin/env python3
"""Read-only exact binding gate for both TEST_ONLY v2 build profiles and IDL."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
candidate = json.loads((ROOT / "spec/LAUNCH_V2_TEST_ONLY_CANDIDATE_v1.json").read_text())
assert candidate["status"] == "TEST_ONLY"
assert candidate["production_admission_approved"] is False
assert candidate["public_chain_deployed"] is False
assert candidate["cliff_seconds"] == 180 * 86_400
assert candidate["period_seconds"] == 30 * 86_400
for artifact in candidate["artifacts"]:
    data = (ROOT / artifact["path"]).read_bytes()
    assert len(data) == artifact["bytes"], artifact["path"]
    assert hashlib.sha256(data).hexdigest() == artifact["sha256"], artifact["path"]
    print(f'{artifact["path"]}: exact bytes PASS')
