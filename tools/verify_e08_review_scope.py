#!/usr/bin/env python3
"""Verify exact review inputs and the local-only example manifest."""
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from e08_recorded_rpc import expand_bundle, manifest_for  # noqa: E402

scope = json.loads((ROOT / "spec/E08_REVIEW_SCOPE_v1.json").read_text())
for group in ("preserved_e07_files", "e08_review_inputs"):
    for path, pin in scope[group].items():
        data = (ROOT / path).read_bytes()
        assert len(data) == pin["bytes"] and hashlib.sha256(data).hexdigest() == pin["sha256"], path
snapshot = expand_bundle(json.loads((ROOT / "examples/e07_rehearsal_bundle.json").read_text()))[-1]
assert json.loads((ROOT / "examples/e08_LOCAL_REPLAY_ONLY_manifest.json").read_text()) == manifest_for(snapshot)
assert all(v is None for v in scope["reviewer"].values()), "Template must not claim human acceptance"
print("E-08 exact review inputs, E-07 preservation and local-only manifest: PASS")
