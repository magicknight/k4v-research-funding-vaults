#!/usr/bin/env python3
"""Build the candidate IDL from Anchor's compiler output; --check never writes.

No RPC, wallet or deployment is used. Unknown emitted sections fail closed so
this small assembler cannot silently discard a new Anchor schema component.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PROGRAM = ROOT / "programs/upgrade-gate-v1"


def build():
    env = os.environ | {
        "ANCHOR_IDL_BUILD_PROGRAM_PATH": str(PROGRAM),
        "ANCHOR_IDL_BUILD_RESOLUTION": "FALSE",
        "ANCHOR_IDL_BUILD_SKIP_LINT": "FALSE",
    }
    output = subprocess.run(
        ["cargo", "test", "--lib", "--locked", "__anchor_private_print_idl",
         "--features", "idl-build", "--", "--show-output", "--quiet"],
        cwd=PROGRAM, env=env, text=True, stdout=subprocess.PIPE, check=True,
    ).stdout
    sections = re.findall(r"--- IDL begin (\w+) ---\n(.*?)\n--- IDL end \1 ---", output, re.S)
    assert {name for name, _ in sections} == {"address", "program", "errors"}, "Unexpected IDL sections"
    assert len(sections) == 3, "Duplicate or missing IDL sections"
    parts = {name: json.loads(value) for name, value in sections}
    idl = parts["program"]
    idl["address"] = parts["address"].strip('"')
    assert re.fullmatch(r"[1-9A-HJ-NP-Za-km-z]{32,44}", idl["address"]), "Invalid program address"
    idl["errors"] = parts["errors"]
    # Match Anchor's unambiguous module-path shortening for this program.
    encoded = json.dumps(idl)
    paths = set(re.findall(r'"((?:\w+::)+\w+)"', encoded))
    short = [path.split("::")[-1] for path in paths]
    assert len(short) == len(set(short)), "Ambiguous IDL type names"
    for path in paths:
        encoded = encoded.replace(json.dumps(path), json.dumps(path.split("::")[-1]))
    idl = json.loads(encoded)
    for field in ("accounts", "instructions", "types"):
        idl[field].sort(key=lambda value: value["name"])
    assert len(idl["instructions"]) == 5
    assert len(idl["accounts"]) == 1
    return json.dumps(idl, indent=2) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text = build()
    destination = ROOT / "idl/upgrade_gate_v1.json"
    if args.check:
        assert destination.read_text() == text, "Committed IDL differs from compiler output"
        print("Upgrade gate v1 compiler-generated IDL: PASS")
    else:
        destination.write_text(text)
        print(destination.relative_to(ROOT))
