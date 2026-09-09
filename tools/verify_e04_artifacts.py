#!/usr/bin/env python3
"""Read-only exact binding gate for E-04's four TEST_ONLY build profiles."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
candidate = json.loads((ROOT / "spec/E04_TEST_ONLY_CANDIDATE_v1.json").read_text())
assert candidate["status"] == "TEST_ONLY"
assert candidate["production_admission_approved"] is False
assert candidate["public_chain_deployed"] is False
assert candidate["cliff_seconds"] == 180 * 86_400
assert candidate["key_change_notice_seconds"] == candidate["upgrade_notice_seconds"] == 90 * 86_400
assert candidate["recovery_threshold"] == candidate["upgrade_threshold"] == 2
assert candidate["recovery_member_count"] == candidate["upgrade_member_count"] == 3
for artifact in candidate["artifacts"]:
    data = (ROOT / artifact["path"]).read_bytes()
    assert len(data) == artifact["bytes"], artifact["path"]
    assert hashlib.sha256(data).hexdigest() == artifact["sha256"], artifact["path"]
    print(f'{artifact["path"]}: exact bytes PASS')
