#!/usr/bin/env python3
"""Lossless, content-addressed compression of raw runtime accounts for review."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import zlib


def pack(raw):
    if raw["schema"] != "K4V-E05-RAW-REHEARSAL-v1":
        raise ValueError("RAW_SCHEMA")
    result = {k: v for k, v in raw.items() if k not in ("schema", "checkpoints")}
    result.update(schema="K4V-E05-REHEARSAL-BUNDLE-v1", blobs={}, checkpoints=[])
    for item in raw["checkpoints"]:
        checkpoint = {k: v for k, v in item.items() if k != "accounts"}
        checkpoint["accounts"] = {}
        for name, a in item["accounts"].items():
            data = bytes.fromhex(a["data_hex"])
            digest = hashlib.sha256(data).hexdigest()
            if digest not in result["blobs"]:
                result["blobs"][digest] = base64.b64encode(zlib.compress(data, 9)).decode()
            checkpoint["accounts"][name] = {k: v for k, v in a.items() if k != "data_hex"}
            checkpoint["accounts"][name]["blob"] = digest
        result["checkpoints"].append(checkpoint)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = pack(json.loads(args.raw.read_text()))
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"Packed {len(result['checkpoints'])} checkpoints, {len(result['blobs'])} distinct raw blobs")
