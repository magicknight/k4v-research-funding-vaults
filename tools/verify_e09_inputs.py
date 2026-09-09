#!/usr/bin/env python3
"""Verify preserved E-07/E-08 inputs and the exact E-09 design/model."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
e08 = json.loads((ROOT / 'spec/E08_REVIEW_SCOPE_v1.json').read_text())
e09 = json.loads((ROOT / 'spec/E09_SUBMISSION_WINDOW_TEST_ONLY_v1.json').read_text())
for bindings in (e08['preserved_e07_files'], e08['e08_review_inputs'], e09['source_bindings']):
    for path, pin in bindings.items():
        data = (ROOT / path).read_bytes()
        assert len(data) == pin['bytes'] and hashlib.sha256(data).hexdigest() == pin['sha256'], path
print('E-09 design/model pins and unchanged E-07/E-08 inputs: PASS')
