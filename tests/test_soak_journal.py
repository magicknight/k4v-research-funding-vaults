"""Persistence, actual HTTP transport and false-continuity rejection from frozen v7 bytes."""
import copy
import json
from pathlib import Path
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from e11b_verifier import expand_bundle
from soak_journal import Journal, LoopbackClient, digest, encoded, initialize, locked

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from e11b_recorded_rpc import RecordedRpc, manifest_for, serve


class SoakJournalTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with tarfile.open(ROOT / "evidence/e11b/accepted-local-evidence.tar.gz", "r:gz") as archive:
            bundle = json.load(archive.extractfile("target/e11b/fresh-bundle.json"))
        cls.snapshot = expand_bundle(bundle)[0]

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "run"
        self.manifest = manifest_for(self.snapshot)
        initialize(self.directory, self.manifest, "preloaded-fixture")
        self.journal = Journal(self.directory)
        self.wall = 1_800_000_000
        self.boot = "boot-one"
        self.session = "session-one"
        self.journal.append("START", self.session, observed=self.stamp())

    def stamp(self):
        return {"wall_ns": self.wall * 10**9, "monotonic_ns": (self.wall - 1_700_000_000) * 10**9,
                "boot_id": self.boot}

    def sample(self, seconds=0, slot_delta=None, client=None, manifest=None):
        snapshot = copy.deepcopy(self.snapshot)
        snapshot["now"] = str(int(snapshot["now"]) + seconds)
        snapshot["slot"] = str(int(snapshot["slot"]) + (seconds * 2 if slot_delta is None else slot_delta))
        raw = bytearray.fromhex(snapshot["accounts"]["clock"]["data_hex"])
        struct.pack_into("<Q", raw, 0, int(snapshot["slot"]))
        struct.pack_into("<q", raw, 32, int(snapshot["now"]))
        snapshot["accounts"]["clock"]["data_hex"] = raw.hex()
        self.wall += seconds
        with patch("soak_journal.stamp", return_value=self.stamp()):
            return self.journal.observe(client or RecordedRpc(snapshot), manifest or self.manifest, self.session)

    def finish(self):
        self.journal.append("STOP", self.session, observed=self.stamp())
        return Journal(self.directory).summary()

    def test_restart_replays_raw_bytes_and_preserves_head(self):
        self.assertTrue(self.sample())
        head = self.finish()["head_sha256"]
        self.journal = Journal(self.directory, head)
        self.session = "session-two"
        self.journal.append("START", self.session, observed=self.stamp())
        self.assertTrue(self.sample(10))
        summary = self.finish()
        self.assertEqual((summary["samples"], summary["observer_sessions"]), (2, 2))
        self.assertEqual(summary["observed_bank_span_seconds"], 10)
        self.assertEqual(summary["first_sample"]["wall_ns"], "1800000000000000000")
        self.assertEqual(summary["anomalies"], [])
        self.assertFalse(summary["natural_90_180_day_soak"])
        self.assertFalse(summary["validator_uptime_verified"])

    def test_account_content_addressing_reuses_program_and_other_static_accounts(self):
        self.sample()
        first = len(list((self.directory / "objects").iterdir()))
        self.assertTrue(self.sample(10))
        second = len(list((self.directory / "objects").iterdir()))
        self.assertEqual(second - first, 1)  # Only Clock changed.

    def test_loopback_http_and_cli_offline_replay(self):
        with serve(RecordedRpc(copy.deepcopy(self.snapshot))) as endpoint:
            self.assertTrue(self.sample(client=LoopbackClient(endpoint)))
        expected = self.finish()["head_sha256"]
        result = subprocess.run([sys.executable, str(ROOT / "tools/soak_observer.py"), "verify",
                                 "--directory", str(self.directory), "--expect-head", expected],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["samples"], 1)

    def test_public_dns_credentials_query_and_redirect_refused(self):
        for endpoint in ("https://api.devnet.solana.com", "http://localhost:8899", "http://127.0.0.1.evil.test",
                         "http://user:pass@127.0.0.1", "http://127.0.0.1/?token=secret"):
            with self.subTest(endpoint=endpoint), self.assertRaises(ValueError):
                LoopbackClient(endpoint)
        with serve(RecordedRpc(self.snapshot), redirect=True) as endpoint:
            self.assertFalse(self.sample(client=LoopbackClient(endpoint)))
        self.assertEqual(self.finish()["observation_errors"], 1)

    def test_genesis_reset_is_recorded_as_error_without_sample(self):
        client = RecordedRpc(self.snapshot)
        original = client.call
        client.call = lambda method, params: self.snapshot["program_id"] if method == "getGenesisHash" else original(method, params)
        self.assertFalse(self.sample(client=client))
        result = self.finish()
        self.assertEqual((result["samples"], result["observation_errors"]), (0, 1))

    def test_identity_change_and_history_removal_rejected(self):
        for field in ("expected", "approval_periods", "external_accounts"):
            candidate = copy.deepcopy(self.manifest)
            if field == "expected":
                candidate[field]["genesis_hash"] = self.snapshot["program_id"]
            elif field == "approval_periods":
                candidate[field] = []
            else:
                candidate[field]["source"] = self.snapshot["program_id"]
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.journal.check_manifest(candidate)

    def test_altered_program_bytes_do_not_enter_accepted_history(self):
        broken = copy.deepcopy(self.snapshot)
        data = bytearray.fromhex(broken["accounts"]["program_data"]["data_hex"])
        data[-1] ^= 1
        broken["accounts"]["program_data"]["data_hex"] = data.hex()
        self.assertFalse(self.sample(client=RecordedRpc(broken)))
        self.assertEqual(self.finish()["samples"], 0)

    def test_observation_gap_and_bank_clock_jump_are_reported(self):
        self.sample()
        self.assertTrue(self.sample(1000))
        self.assertIn("OBSERVATION_GAP", self.finish()["anomalies"])

    def test_time_does_not_advance_soak_claim_for_replayed_same_bank(self):
        self.sample()
        self.wall += 180 * 86400
        self.assertTrue(self.sample())
        result = self.finish()
        self.assertIn("SLOT_NOT_ADVANCING", result["anomalies"])
        self.assertIn("BANK_WALL_TIME_DIVERGENCE", result["anomalies"])
        self.assertFalse(result["natural_90_180_day_soak"])

    def test_bank_time_and_slot_rollback_visible(self):
        self.sample(20)
        self.assertTrue(self.sample(10))
        result = self.finish()
        self.assertIn("BANK_TIME_ROLLBACK", result["anomalies"])
        self.assertIn("SLOT_NOT_ADVANCING", result["anomalies"])

    def test_host_reboot_and_unclosed_observer_session_visible(self):
        self.sample()
        self.journal = Journal(self.directory)
        self.boot, self.session = "boot-two", "session-two"
        self.journal.append("START", self.session, observed=self.stamp())
        self.assertTrue(self.sample(10))
        result = self.finish()
        self.assertIn("HOST_BOOT_CHANGED", result["anomalies"])
        self.assertEqual(result["unclosed_observer_sessions"], 1)

    def test_torn_tail_refuses_resume_without_silent_repair(self):
        self.sample()
        path = self.directory / "journal.jsonl"
        with path.open("ab") as stream:
            stream.write(b'{"sequence":2')
        before = path.read_bytes()
        with self.assertRaisesRegex(ValueError, "PARTIAL"):
            Journal(self.directory)
        self.assertEqual(path.read_bytes(), before)

    def test_changed_object_and_path_substitution_rejected(self):
        self.sample()
        record = json.loads((self.directory / "journal.jsonl").read_text().splitlines()[-1])
        key = record["payload"]["snapshot"]["accounts"]["clock"]
        path = self.directory / "objects" / (key + ".json")
        raw = json.loads(path.read_text())
        raw["lamports"] = "2"
        path.write_bytes(encoded(raw))
        with self.assertRaisesRegex(ValueError, "OBJECT_CHANGED"):
            Journal(self.directory)

    def test_middle_deletion_and_external_tail_anchor_detect_truncation(self):
        self.sample()
        head = self.finish()["head_sha256"]
        path = self.directory / "journal.jsonl"
        lines = path.read_bytes().splitlines(keepends=True)
        path.write_bytes(b"".join(lines[:-1]))
        with self.assertRaisesRegex(ValueError, "EXPECTED_HEAD"):
            Journal(self.directory, head)
        path.write_bytes(lines[0] + lines[2])
        with self.assertRaisesRegex(ValueError, "JOURNAL_CHAIN"):
            Journal(self.directory)

    def test_single_writer_lock_and_existing_run_protection(self):
        with locked(self.directory):
            with self.assertRaises(BlockingIOError), locked(self.directory):
                pass
        with self.assertRaises(FileExistsError):
            initialize(self.directory, self.manifest, "unknown")

    def test_failed_durable_write_stops_future_appends_without_advancing_head(self):
        before = self.journal.head
        with patch("soak_journal.os.fsync", side_effect=OSError("disk failure")):
            with self.assertRaises(OSError):
                self.journal.append("STOP", self.session, observed=self.stamp())
        self.assertEqual(self.journal.head, before)
        with self.assertRaisesRegex(ValueError, "WRITE_PREVIOUSLY_FAILED"):
            self.journal.append("STOP", self.session, observed=self.stamp())

    def test_runtime_substitution_and_duplicate_json_keys_rejected(self):
        with patch("soak_journal.runtime_hashes", return_value={}):
            with self.assertRaisesRegex(ValueError, "RUNTIME_CHANGED"):
                Journal(self.directory)
        path = self.directory / "run.json"
        raw = path.read_bytes().rstrip()
        path.write_bytes(raw[:-1] + b',"schema":"duplicate"}')
        with self.assertRaisesRegex(ValueError, "RPC_JSON"):
            Journal(self.directory)


if __name__ == "__main__":
    unittest.main()
