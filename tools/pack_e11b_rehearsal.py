#!/usr/bin/env python3
"""Lossless E-11B packaging using the existing content-addressed blob format."""
import argparse
import json
from pathlib import Path
from pack_e05_rehearsal import pack as pack_blobs


def pack(raw):
    if raw['schema'] != 'K4V-E11B-LOCAL-REHEARSAL-v1':
        raise ValueError('RAW_SCHEMA')
    adapted = {k: v for k, v in raw.items() if k not in ('schema', 'snapshots')}
    adapted.update(schema='K4V-E05-RAW-REHEARSAL-v1', checkpoints=raw['snapshots'])
    result = pack_blobs(adapted)
    result['schema'] = 'K4V-E11B-REHEARSAL-BUNDLE-v1'
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('raw', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = pack(json.loads(args.raw.read_text()))
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(f"Packed {len(result['checkpoints'])} E-11B checkpoints / {len(result['blobs'])} raw blobs")
