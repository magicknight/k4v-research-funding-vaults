#!/usr/bin/env python3
"""Pin E-10 inputs/build bytes and preserve the E-07/E-08/E-09 evidence."""
import hashlib
import json
from pathlib import Path
import runpy
import sys

ROOT = Path(__file__).resolve().parents[1]
runpy.run_path(str(ROOT / 'tools/verify_e09_inputs.py'))
spec = json.loads((ROOT / 'spec/LAUNCH_V6_TEST_ONLY_CANDIDATE_v1.json').read_text())
for path, pin in spec['source_bindings'].items():
    data = (ROOT / path).read_bytes()
    assert len(data) == pin['bytes'] and hashlib.sha256(data).hexdigest() == pin['sha256'], path
if '--sources-only' not in sys.argv:
    for item in spec['sbf_artifacts']:
        data = (ROOT / item['path']).read_bytes()
        assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256'], item['path']
assert (ROOT / 'candidates/launch-vault-v6/src/capacity.rs').read_text().replace('V6', 'V5') == (ROOT / 'programs/launch-vault-v5/src/capacity.rs').read_text()
assert (ROOT / 'candidates/launch-vault-v6/src/governance.rs').read_text().replace('V6', 'V5').replace('v6', 'v5') == (ROOT / 'programs/launch-vault-v5/src/governance.rs').read_text()
print('E-10 exact inputs, unchanged financial kernel and historical preservation: PASS')
