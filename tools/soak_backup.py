#!/usr/bin/env python3
"""Offline, private TEST_ONLY run backups. Never uploads files or overwrites a target."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat

INDEX = "backup-index.json"
SCHEMA = "K4V-PRIVATE-SOAK-BACKUP-v1"


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def inventory(root):
    files, sockets = {}, []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root).as_posix()
        if rel == INDEX:
            continue
        mode = path.lstat().st_mode
        if stat.S_ISLNK(mode):
            # Agave snapshots use absolute internal account-hardlink aliases.
            # Index relative targets; rebase absolute links on copy for Agave,
            # whose fastboot compares read_link() directly to absolute run paths.
            target = path.resolve()
            if not target.exists() or root not in target.parents:
                raise ValueError("SYMLINK_REFUSED: " + rel)
            files[rel] = {"kind": "internal_alias", "target": os.path.relpath(target, path.parent)}
            continue
        if stat.S_ISSOCK(mode):
            sockets.append(rel)  # IPC endpoints are recreated by Agave, not ledger data.
            continue
        record = {"mode": stat.S_IMODE(mode)}
        if stat.S_ISDIR(mode):
            record["kind"] = "directory"
        elif stat.S_ISREG(mode):
            record.update(kind="file", bytes=path.stat().st_size, sha256=digest(path))
        else:
            raise ValueError("SPECIAL_FILE_REFUSED: " + rel)
        files[rel] = record
    return files, sockets


def root_path(value):
    raw = Path(value).absolute()
    if raw != raw.resolve():
        raise ValueError("ROOT_SYMLINK_REFUSED")
    return raw


def new_target(source, destination):
    source, destination = root_path(source), root_path(destination)
    if destination == source or source in destination.parents or destination in source.parents:
        raise ValueError("OVERLAPPING_PATHS")
    if destination.exists():
        raise ValueError("TARGET_EXISTS")
    return source, destination


@contextmanager
def stopped_ledger(root):
    # Agave 3.1.10 uses fd_lock on ledger/ledger.lock. Hold both POSIX
    # lock forms on Linux; the actual-node acceptance tests live refusal.
    path = root / "ledger" / "ledger.lock"
    if not path.is_file() or path.is_symlink():
        raise ValueError("LEDGER_LOCK_MISSING")
    with path.open("r+b") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.lockf(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("VALIDATOR_STILL_RUNNING") from error
        yield


def copy_records(source, target, records):
    target.mkdir(mode=0o700)
    for rel, record in records.items():
        origin, dest = source / rel, target / rel
        if record["kind"] == "directory":
            dest.mkdir(mode=record["mode"])
            dest.chmod(record["mode"])
        elif record["kind"] == "internal_alias":
            dest.symlink_to((dest.parent / record["target"]).resolve())
        else:
            shutil.copyfile(origin, dest, follow_symlinks=False)
            dest.chmod(record["mode"])
            with dest.open("rb") as stream:
                os.fsync(stream.fileno())
    for directory in [p for p in target.rglob("*") if p.is_dir()] + [target]:
        fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)


def verify(directory, expected):
    root = root_path(directory)
    if root.stat().st_mode & 0o077:
        raise ValueError("PRIVATE_DIRECTORY_REQUIRED")
    if not isinstance(expected, str) or len(expected) != 64 or digest(root / INDEX) != expected:
        raise ValueError("BACKUP_HEAD_MISMATCH")
    data = json.loads((root / INDEX).read_text())
    if data.get("schema") != SCHEMA or data.get("contains_test_private_keys") is not True:
        raise ValueError("BACKUP_SCHEMA")
    actual, sockets = inventory(root)
    if sockets or actual != data["files"]:
        raise ValueError("BACKUP_CONTENT_MISMATCH")
    return data


def backup(source, destination):
    source, destination = new_target(source, destination)
    if source.stat().st_mode & 0o077:
        raise ValueError("PRIVATE_DIRECTORY_REQUIRED")
    with stopped_ledger(source):
        before, sockets = inventory(source)
        if INDEX in [p.name for p in source.iterdir()]:
            raise ValueError("BACKUP_IS_NOT_A_LIVE_RUN")
        for required in ("ledger/genesis.bin", "state.json", "test-keys.json"):
            if required not in before:
                raise ValueError("INCOMPLETE_RUN: " + required)
        if before["test-keys.json"]["mode"] & 0o077:
            raise ValueError("PRIVATE_KEYS_MODE")
        copy_records(source, destination, before)
        after, after_sockets = inventory(source)
        if before != after or sockets != after_sockets or inventory(destination)[0] != before:
            raise ValueError("SOURCE_CHANGED_DURING_BACKUP")
        data = {"schema": SCHEMA, "contains_test_private_keys": True,
                "public_artifact": False, "files": before,
                "omitted_runtime_sockets": sockets}
        with (destination / INDEX).open("x") as stream:
            json.dump(data, stream, sort_keys=True, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        head = digest(destination / INDEX)
        verify(destination, head)
        return {"valid": True, "backup_sha256": head, "files": len(before),
                "bytes": sum(r.get("bytes", 0) for r in before.values()),
                "contains_test_private_keys": True, "omitted_runtime_sockets": sockets}


def restore(source, destination, expected):
    source, destination = new_target(source, destination)
    data = verify(source, expected)
    copy_records(source, destination, data["files"])
    if inventory(destination)[0] != data["files"]:
        raise ValueError("RESTORE_CONTENT_MISMATCH")
    return {"valid": True, "restored_files": len(data["files"]), "backup_sha256": expected}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["backup", "verify", "restore"])
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path)
    parser.add_argument("--expect-head")
    args = parser.parse_args()
    if args.command != "verify" and args.destination is None:
        parser.error("--destination required")
    if args.command != "backup" and args.expect_head is None:
        parser.error("--expect-head required")
    if args.command == "backup":
        result = backup(args.source, args.destination)
    elif args.command == "restore":
        result = restore(args.source, args.destination, args.expect_head)
    else:
        data = verify(args.source, args.expect_head)
        result = {"valid": True, "files": len(data["files"]), "contains_test_private_keys": True}
    print(json.dumps(result))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(str(error)) from error
