#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Generate a deterministic purpose-bound vault decision receipt."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from purpose_bound_vault import decision_receipt, request_from_dict


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("request", type=Path, help="JSON release request")
    args = parser.parse_args()
    data = json.loads(args.request.read_text(encoding="utf-8"))
    print(json.dumps(decision_receipt(request_from_dict(data)), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
