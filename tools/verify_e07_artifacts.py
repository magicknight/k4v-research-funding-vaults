#!/usr/bin/env python3
"""Fail closed on changed E-07 build bytes or changed historical program sources."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = json.loads((ROOT/'spec/LAUNCH_V5_TEST_ONLY_CANDIDATE_v1.json').read_text())
for item in spec['sbf_artifacts']:
    data = (ROOT/item['path']).read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256'], item['path']
for path, digest in spec['preserved_e04_source_sha256'].items():
    assert hashlib.sha256((ROOT/path).read_bytes()).hexdigest() == digest, path
assert (ROOT/'programs/launch-vault-v5/src/capacity.rs').read_text().replace('V5', 'V4') == (ROOT/'programs/launch-vault-v4/src/capacity.rs').read_text()
print('E-07 exact SBF pins, unchanged financial kernel and historical source preservation: PASS')
