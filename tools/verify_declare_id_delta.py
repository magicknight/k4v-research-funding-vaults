#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Prove that two B1 SBF builds differ only by their declared program address.

A retained program signer requires a new `declare_id!`, and Anchor compiles the
declared address into the program, so the public-cluster artifact can never be
byte-identical to a previously frozen one. This check replaces that gap with an
exact statement: the two builds must have the same length and must differ in
nothing but whole copies of the 32-byte address.

Usage:
    python3 tools/verify_declare_id_delta.py \\
        --old  path/to/old.so --old-id  <BASE58> \\
        --new  path/to/new.so --new-id  <BASE58>

Exit status is 0 only when the delta is fully accounted for.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def base58_decode_pubkey(value: str) -> bytes:
    number = 0
    for character in value:
        try:
            number = number * 58 + BASE58_ALPHABET.index(character)
        except ValueError as exc:  # pragma: no cover - argument validation
            raise SystemExit(f"invalid base58 character in {value!r}") from exc
    payload = number.to_bytes(32, "big")
    leading = len(value) - len(value.lstrip("1"))
    if leading + len(payload.lstrip(b"\x00")) > 32:
        raise SystemExit(f"{value!r} does not decode to a 32-byte pubkey")
    return payload


def differing_runs(old: bytes, new: bytes) -> list[tuple[int, int]]:
    offsets = [index for index in range(len(old)) if old[index] != new[index]]
    runs: list[tuple[int, int]] = []
    for offset in offsets:
        if runs and offset == runs[-1][0] + runs[-1][1]:
            runs[-1] = (runs[-1][0], runs[-1][1] + 1)
        else:
            runs.append((offset, 1))
    return runs


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", required=True)
    parser.add_argument("--old-id", required=True)
    parser.add_argument("--new", required=True)
    parser.add_argument("--new-id", required=True)
    args = parser.parse_args(argv)

    old = open(args.old, "rb").read()
    new = open(args.new, "rb").read()
    old_id = base58_decode_pubkey(args.old_id)
    new_id = base58_decode_pubkey(args.new_id)

    reasons: list[str] = []
    if len(old) != len(new):
        reasons.append(f"sizes differ: {len(old)} vs {len(new)}")
        runs = []
        accounted = 0
    else:
        runs = differing_runs(old, new)
        # Reassemble the differing bytes into whole 32-byte address copies. A
        # 64-bit immediate load splits the address across discontiguous 4-byte
        # fields, so runs are grouped by consuming 32 differing bytes at a time.
        old_delta = b"".join(old[offset : offset + length] for offset, length in runs)
        new_delta = b"".join(new[offset : offset + length] for offset, length in runs)
        accounted = len(old_delta)
        if accounted % 32:
            reasons.append(f"{accounted} differing bytes is not a multiple of 32")
        copies = accounted // 32
        for index in range(copies):
            chunk = slice(index * 32, index * 32 + 32)
            if old_delta[chunk] != old_id:
                reasons.append(f"copy {index} of the old build is not the old program id")
            if new_delta[chunk] != new_id:
                reasons.append(f"copy {index} of the new build is not the new program id")

    receipt = {
        "schema": "k4v-declare-id-delta-check/v0.1",
        "old": {
            "path": args.old,
            "program_id": args.old_id,
            "size_bytes": len(old),
            "sha256": hashlib.sha256(old).hexdigest(),
        },
        "new": {
            "path": args.new,
            "program_id": args.new_id,
            "size_bytes": len(new),
            "sha256": hashlib.sha256(new).hexdigest(),
        },
        "sizes_equal": len(old) == len(new),
        "differing_byte_count": accounted,
        "differing_run_count": len(runs),
        "program_id_copies": accounted // 32 if accounted % 32 == 0 else None,
        "bytes_differing_outside_the_program_id": 0 if not reasons else None,
        "runs": [{"offset": offset, "length": length} for offset, length in runs],
        "valid": not reasons,
        "reasons": reasons,
    }
    json.dump(receipt, sys.stdout, indent=2, sort_keys=False)
    sys.stdout.write("\n")
    return 0 if not reasons else 1


if __name__ == "__main__":
    raise SystemExit(main())
