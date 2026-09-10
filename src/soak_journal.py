"""Append-only local v7 observations. Integrity and elapsed time are not a soak verdict."""
from contextlib import contextmanager
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.parse import urlparse
from urllib.request import ProxyHandler, build_opener

from e11b_verifier import CODE_SHA256, verify_graph
from launch_v7_rpc_exporter import (
    Client, NoRedirect, bind_identity, export, history_addresses, key_addresses,
    strict_json, validate_manifest,
)
from launch_v7_verifier import read_policy, require

ROOT = Path(__file__).resolve().parents[1]
ZERO = "0" * 64
LIMIT = 4_000_000
ORIGINS = ("signed-bootstrap-declared", "preloaded-fixture", "unknown")


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def read_json(path):
    with Path(path).open("rb") as stream:
        raw = stream.read(LIMIT + 1)
    require(len(raw) <= LIMIT, "JOURNAL_OBJECT_TOO_LARGE")
    return strict_json(raw)


def exclusive_write(path, raw):
    with Path(path).open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def runtime_hashes():
    # Reproduction must use the exact decoder implementation as well as SBF.
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted((ROOT / "src").glob("*.py"))}


def initialize(directory, manifest, origin, max_gap_seconds=900, tolerance_seconds=30):
    validate_manifest(manifest)
    require(origin in ORIGINS, "JOURNAL_ORIGIN")
    require(type(max_gap_seconds) is int and max_gap_seconds > 0, "JOURNAL_GAP")
    require(type(tolerance_seconds) is int and tolerance_seconds >= 0, "JOURNAL_TOLERANCE")
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--", "src", "tools/soak_observer.py"], cwd=ROOT, text=True)
    run = {"schema": "K4V-SOAK-RUN-v1", "initial_manifest": manifest,
           "declared_origin": origin, "source_commit": commit, "source_worktree_clean": not bool(dirty),
           "runtime_sha256": runtime_hashes(), "program_sha256": CODE_SHA256,
           "max_gap_seconds": max_gap_seconds, "tolerance_seconds": tolerance_seconds}
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=False)
    (directory / "objects").mkdir()
    exclusive_write(directory / "run.json", encoded(run) + b"\n")
    exclusive_write(directory / "journal.jsonl", b"")
    exclusive_write(directory / "writer.lock", b"")
    sync_directory(directory)
    sync_directory(directory.parent)
    return run


@contextmanager
def locked(directory):
    # A second observer or an offline reader must not race an append.
    with (Path(directory) / "writer.lock").open("rb") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def put_object(directory, value):
    key = digest(value)
    path = Path(directory) / "objects" / (key + ".json")
    if path.exists():
        require(digest(read_json(path)) == key, "JOURNAL_OBJECT_CHANGED")
    else:
        exclusive_write(path, encoded(value) + b"\n")
        sync_directory(path.parent)
    return key


def get_object(directory, key):
    require(isinstance(key, str) and re.fullmatch(r"[0-9a-f]{64}", key), "JOURNAL_OBJECT_KEY")
    value = read_json(Path(directory) / "objects" / (key + ".json"))
    require(digest(value) == key, "JOURNAL_OBJECT_CHANGED")
    return value


def stamp():
    # CLOCK_BOOTTIME includes suspend; boot_id distinguishes host restarts.
    return {"wall_ns": time.time_ns(),
            "monotonic_ns": time.clock_gettime_ns(time.CLOCK_BOOTTIME),
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip()}


class LoopbackClient(Client):
    def __init__(self, endpoint):
        url = urlparse(endpoint)
        require(url.scheme == "http" and url.hostname in ("127.0.0.1", "::1")
                and url.path in ("", "/") and not url.query, "JOURNAL_LOOPBACK_ONLY")
        super().__init__(endpoint)
        self.opener = build_opener(ProxyHandler({}), NoRedirect())


class Journal:
    """Call under locked(). Replay streams the journal, holding at most one graph."""
    def __init__(self, directory, expected_head=None):
        self.directory = Path(directory)
        self.run = read_json(self.directory / "run.json")
        require(self.run["schema"] == "K4V-SOAK-RUN-v1", "JOURNAL_SCHEMA")
        require(self.run["program_sha256"] == CODE_SHA256, "JOURNAL_PROGRAM_CHANGED")
        require(self.run["runtime_sha256"] == runtime_hashes(), "JOURNAL_RUNTIME_CHANGED")
        validate_manifest(self.run["initial_manifest"])
        require(self.run["declared_origin"] in ORIGINS, "JOURNAL_ORIGIN")
        for name in ("max_gap_seconds", "tolerance_seconds"):
            require(type(self.run[name]) is int and self.run[name] >= (1 if name == "max_gap_seconds" else 0),
                    "JOURNAL_TIME_POLICY")
        self.run_hash = digest(self.run)
        self.head, self.count = ZERO, 0
        self.samples, self.errors, self.sessions, self.unclosed_sessions = 0, 0, 0, 0
        self.anomalies = set()
        self.active_session = None
        self.seen_sessions = set()
        self.last_event = self.last_sample = self.last_policy = self.first_sample = None
        self.last_manifest = self.run["initial_manifest"]
        self.max_observed_gap_seconds = 0
        self.write_broken = False
        with (self.directory / "journal.jsonl").open("rb") as stream:
            while True:
                line = stream.readline(LIMIT + 1)
                if not line:
                    break
                require(len(line) <= LIMIT and line.endswith(b"\n"), "JOURNAL_PARTIAL_OR_OVERSIZE_RECORD")
                self.accept(strict_json(line))
        if expected_head is not None:
            require(self.head == expected_head, "JOURNAL_EXPECTED_HEAD_MISMATCH")

    def check_manifest(self, manifest):
        validate_manifest(manifest)
        require(manifest["expected"] == self.run["initial_manifest"]["expected"], "JOURNAL_IDENTITY_CHANGED")
        old = self.last_manifest
        require(all(manifest["external_accounts"].get(k) == v for k, v in old["external_accounts"].items()),
                "JOURNAL_EXTERNAL_HISTORY_REMOVED")
        require(set(old["approval_periods"]) <= set(manifest["approval_periods"]), "JOURNAL_APPROVAL_HISTORY_REMOVED")

    def unpack(self, payload):
        manifest = get_object(self.directory, payload["manifest"])
        self.check_manifest(manifest)
        packed = payload["snapshot"]
        require(isinstance(packed["accounts"], dict) and 1 <= len(packed["accounts"]) <= 100,
                "JOURNAL_ACCOUNT_COUNT")
        snapshot = {**packed, "accounts": {
            name: get_object(self.directory, key) for name, key in packed["accounts"].items()}}
        policy = bind_identity(snapshot, manifest["expected"])
        addresses = validate_manifest(manifest)
        history = history_addresses(policy, manifest["approval_periods"])
        addresses.update(history)
        addresses.update(key_addresses(snapshot, history, policy["address"]))
        require({n: a["address"] for n, a in snapshot["accounts"].items()} == addresses,
                "JOURNAL_COMPLETE_ACCOUNT_GRAPH")
        require(verify_graph(snapshot)["valid"] is True, "JOURNAL_RAW_GRAPH")
        provenance = payload["provenance"]
        require(provenance["expected_genesis_hash"] == manifest["expected"]["genesis_hash"]
                and provenance["manifest_sha256"] == digest(manifest)
                and provenance["context_slot"] == snapshot["slot"]
                and provenance["final_account_count"] == len(snapshot["accounts"])
                and provenance["requested_commitment"] == "finalized"
                and provenance["single_final_response"] is True
                and provenance["discovery_bytes_used_in_snapshot"] is False, "JOURNAL_PROVENANCE")
        return manifest, snapshot

    def accept(self, record):
        require(set(record) == {"schema", "sequence", "previous", "run_sha256", "kind", "session",
                               "wall_ns", "monotonic_ns", "boot_id", "payload", "sha256"}, "JOURNAL_RECORD_FIELDS")
        require(record["schema"] == "K4V-SOAK-EVENT-v1" and record["run_sha256"] == self.run_hash,
                "JOURNAL_RUN_BINDING")
        body = {k: v for k, v in record.items() if k != "sha256"}
        require(digest(body) == record["sha256"] and record["previous"] == self.head
                and type(record["sequence"]) is int and record["sequence"] == self.count,
                "JOURNAL_CHAIN")
        for key in ("wall_ns", "monotonic_ns"):
            require(type(record[key]) is int and record[key] > 0, "JOURNAL_TIMESTAMP")
        for key in ("session", "boot_id"):
            require(isinstance(record[key], str) and re.fullmatch(r"[a-zA-Z0-9-]{1,80}", record[key]),
                    "JOURNAL_SESSION_ID")
        kind, payload = record["kind"], record["payload"]
        if kind == "START":
            require(payload == {} and record["session"] not in self.seen_sessions, "JOURNAL_SESSION_REPLAY")
            self.seen_sessions.add(record["session"])
            if self.active_session is not None:
                self.unclosed_sessions += 1
            self.active_session = record["session"]
            self.sessions += 1
        else:
            require(record["session"] == self.active_session, "JOURNAL_SESSION_ORDER")
            if kind == "SAMPLE":
                require(set(payload) == {"manifest", "snapshot", "provenance"}, "JOURNAL_SAMPLE_FIELDS")
                manifest, snapshot = self.unpack(payload)
                policy = read_policy(snapshot)
                current = {"slot": int(snapshot["slot"]), "bank_time": int(snapshot["now"]),
                           "wall_ns": record["wall_ns"]}
                if self.last_sample is not None:
                    wall_delta = (current["wall_ns"] - self.last_sample["wall_ns"]) / 1e9
                    bank_delta = current["bank_time"] - self.last_sample["bank_time"]
                    self.max_observed_gap_seconds = max(self.max_observed_gap_seconds, wall_delta)
                    if current["slot"] <= self.last_sample["slot"]:
                        self.anomalies.add("SLOT_NOT_ADVANCING")
                    if bank_delta < 0:
                        self.anomalies.add("BANK_TIME_ROLLBACK")
                    if wall_delta > self.run["max_gap_seconds"]:
                        self.anomalies.add("OBSERVATION_GAP")
                    if abs(bank_delta - wall_delta) > self.run["tolerance_seconds"]:
                        self.anomalies.add("BANK_WALL_TIME_DIVERGENCE")
                    require(policy["config"] == self.last_policy["config"], "JOURNAL_CONFIG_CHANGED")
                    for key in ("founder_released_total", "treasury_released_total", "approval_count", "change_sequence"):
                        require(policy[key] >= self.last_policy[key], "JOURNAL_COUNTER_ROLLBACK")
                self.first_sample = self.first_sample or current
                self.last_sample, self.last_policy, self.last_manifest = current, policy, manifest
                self.samples += 1
            elif kind == "ERROR":
                require(set(payload) == {"reason"} and re.fullmatch(r"[A-Z0-9_]{1,100}", payload["reason"]),
                        "JOURNAL_ERROR_FIELDS")
                self.errors += 1
            elif kind == "STOP":
                require(payload == {}, "JOURNAL_STOP_FIELDS")
                self.active_session = None
            else:
                raise ValueError("JOURNAL_EVENT_KIND")
        if self.last_event is not None:
            old = self.last_event
            wall = (record["wall_ns"] - old["wall_ns"]) / 1e9
            if wall < 0:
                self.anomalies.add("HOST_WALL_TIME_ROLLBACK")
            if record["boot_id"] != old["boot_id"]:
                self.anomalies.add("HOST_BOOT_CHANGED")
            else:
                mono = (record["monotonic_ns"] - old["monotonic_ns"]) / 1e9
                if mono < 0 or abs(mono - wall) > self.run["tolerance_seconds"]:
                    self.anomalies.add("HOST_CLOCK_DISCONTINUITY")
        self.last_event = record
        self.head, self.count = record["sha256"], self.count + 1

    def append(self, kind, session, payload=None, observed=None):
        require(not self.write_broken, "JOURNAL_WRITE_PREVIOUSLY_FAILED")
        body = {"schema": "K4V-SOAK-EVENT-v1", "sequence": self.count, "previous": self.head,
                "run_sha256": self.run_hash, "kind": kind, "session": session,
                **(observed or stamp()), "payload": payload or {}}
        record = {**body, "sha256": digest(body)}
        staged = copy.copy(self)
        staged.anomalies = set(self.anomalies)
        staged.seen_sessions = set(self.seen_sessions)
        staged.accept(record)  # Validate without advancing the persisted head.
        try:
            with (self.directory / "journal.jsonl").open("ab") as stream:
                stream.write(encoded(record) + b"\n")
                stream.flush()
                os.fsync(stream.fileno())
        except OSError:
            self.write_broken = True
            raise
        self.__dict__.update(staged.__dict__)

    def observe(self, client, manifest, session):
        try:
            self.check_manifest(manifest)
            result = export(client, manifest, 0)
        except (OSError, ValueError, KeyError, TypeError) as error:
            reason = str(error)
            if re.fullmatch(r"[A-Z0-9_]{1,100}", reason) is None:
                reason = "OBSERVATION_FAILED"
            self.append("ERROR", session, {"reason": reason})
            return False
        # Storage failures are fatal, not retried past a possibly torn write.
        snapshot = result["snapshot"]
        payload = {"manifest": put_object(self.directory, manifest),
                   "snapshot": {**snapshot, "accounts": {
                       n: put_object(self.directory, a) for n, a in snapshot["accounts"].items()}},
                   "provenance": result["provenance"]}
        try:
            self.append("SAMPLE", session, payload)
        except (ValueError, KeyError, TypeError) as error:
            reason = str(error)
            if re.fullmatch(r"[A-Z0-9_]{1,100}", reason) is None:
                reason = "OBSERVATION_CONTINUITY_FAILED"
            self.append("ERROR", session, {"reason": reason})
            return False
        return True

    def summary(self):
        first, last = self.first_sample, self.last_sample
        def public_stamp(value):
            # Nanoseconds exceed JavaScript's exact integer range. Keep the raw
            # Python journal integers, but make cross-language summaries lossless.
            return {**value, "wall_ns": str(value["wall_ns"])} if value else None
        return {"schema": "K4V-SOAK-SUMMARY-v1", "valid": True,
                "scope": "LOCAL_RPC_OBSERVATION_JOURNAL", "head_sha256": self.head,
                "run_sha256": self.run_hash, "events": self.count, "samples": self.samples,
                "observation_errors": self.errors, "observer_sessions": self.sessions,
                "unclosed_observer_sessions": self.unclosed_sessions + int(self.active_session is not None),
                "anomalies": sorted(self.anomalies), "first_sample": public_stamp(first), "last_sample": public_stamp(last),
                "observed_bank_span_seconds": last["bank_time"] - first["bank_time"] if first else 0,
                "observed_wall_span_seconds": (last["wall_ns"] - first["wall_ns"]) / 1e9 if first else 0,
                "max_observed_gap_seconds": self.max_observed_gap_seconds,
                "declared_origin": self.run["declared_origin"], "origin_independently_verified": False,
                "rpc_server_trust_required": True, "validator_uptime_verified": False,
                "complete_ledger_preserved": False, "natural_90_180_day_soak": False,
                "continuous_bootstrap_to_maturity_history": False,
                "independent_human_review": False, "production_ready": False,
                "public_chain_transactions": 0}
