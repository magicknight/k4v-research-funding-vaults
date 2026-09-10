#!/usr/bin/env python3
"""Record or replay a local v7 observation journal; never sends a transaction."""
import argparse
import json
from pathlib import Path
import signal
import sys
import threading
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from soak_journal import Journal, LoopbackClient, ORIGINS, initialize, locked, read_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init")
    init.add_argument("--manifest", type=Path, required=True)
    init.add_argument("--origin", choices=ORIGINS, required=True)
    init.add_argument("--max-gap-seconds", type=int, default=900)
    init.add_argument("--tolerance-seconds", type=int, default=30)
    record = commands.add_parser("record")
    record.add_argument("--rpc-url", required=True)
    record.add_argument("--manifest", type=Path, required=True)
    record.add_argument("--samples", type=int, default=1, help="0 runs in the foreground until SIGINT/SIGTERM")
    record.add_argument("--interval-seconds", type=int, default=600)
    replay = commands.add_parser("verify")
    for sub in (init, record, replay):
        sub.add_argument("--directory", type=Path, required=True)
    for sub in (record, replay):
        sub.add_argument("--expect-head", help="An independently retained previous head digest")
    args = parser.parse_args()
    if args.command == "init":
        initialize(args.directory, read_json(args.manifest), args.origin,
                   args.max_gap_seconds, args.tolerance_seconds)
        with locked(args.directory):
            print(json.dumps(Journal(args.directory).summary()))
        return 0
    if args.command == "record" and (args.samples < 0 or args.interval_seconds < 1):
        parser.error("samples must be nonnegative and interval must be positive")
    with locked(args.directory):
        journal = Journal(args.directory, args.expect_head)
        if args.command == "verify":
            print(json.dumps(journal.summary(), indent=2))
            return 0
        client = LoopbackClient(args.rpc_url)
        manifest = read_json(args.manifest)
        journal.check_manifest(manifest)
        stop = threading.Event()
        signal.signal(signal.SIGINT, lambda *_: stop.set())
        signal.signal(signal.SIGTERM, lambda *_: stop.set())
        session, attempts, successes = str(uuid.uuid4()), 0, 0
        journal.append("START", session)
        try:
            while not stop.is_set() and (args.samples == 0 or attempts < args.samples):
                successes += int(journal.observe(client, manifest, session))
                attempts += 1
                print(json.dumps(journal.summary()), flush=True)
                if args.samples == 0 or attempts < args.samples:
                    stop.wait(args.interval_seconds)
        finally:
            journal.append("STOP", session)
            print(json.dumps(journal.summary()), flush=True)
        return 0 if successes == attempts and attempts > 0 else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({"valid": False, "error": str(error)}), file=sys.stderr)
        raise SystemExit(1) from error
