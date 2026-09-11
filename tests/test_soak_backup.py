"""Failure boundaries for private stopped-ledger copies; actual Agave is separate."""
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import socket
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("soak_backup", Path(__file__).resolve().parents[1] / "tools/soak_backup.py")
backup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(backup)


class PrivateLedgerBackupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "run"
        self.source.mkdir(mode=0o700)
        (self.source / "ledger").mkdir()
        for name, value in {"ledger/ledger.lock": b"", "ledger/genesis.bin": b"test genesis",
                            "ledger/blockstore": b"test ledger", "state.json": b"{}",
                            "test-keys.json": b"[]"}.items():
            path = self.source / name
            path.write_bytes(value)
            path.chmod(0o600)
        self.target = self.root / "backup"

    def make(self):
        return backup.backup(self.source, self.target)["backup_sha256"]

    def test_roundtrip_all_files_modes_and_empty_directories(self):
        (self.source / "ledger/empty").mkdir()
        head = self.make()
        restored = self.root / "restored"
        backup.restore(self.target, restored, head)
        self.assertEqual(backup.inventory(self.source), backup.inventory(restored))
        self.assertFalse((restored / backup.INDEX).exists())

    def test_live_ledger_lock_refused_before_target_created(self):
        with (self.source / "ledger/ledger.lock").open("r+b") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(ValueError, "VALIDATOR_STILL_RUNNING"):
                self.make()
        self.assertFalse(self.target.exists())

    def test_symlink_cannot_import_outside_file(self):
        (self.source / "outside").symlink_to(self.root / "secret")
        with self.assertRaisesRegex(ValueError, "SYMLINK"):
            self.make()

    def test_root_symlink_refused(self):
        link = self.root / "alias"
        link.symlink_to(self.source, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "ROOT_SYMLINK"):
            backup.backup(link, self.target)

    def test_internal_agave_aliases_are_relocated_without_source_dependency(self):
        (self.source / "ledger/validator-123.log").write_text("log")
        link = self.source / "ledger/validator.log"
        link.symlink_to("validator-123.log")
        (self.source / "ledger/absolute-alias").symlink_to(self.source / "ledger/validator-123.log")
        head = self.make()
        backup.restore(self.target, self.root / "restored", head)
        self.assertEqual((self.root / "restored/ledger/validator.log").read_text(), "log")
        self.assertEqual((self.root / "restored/ledger/absolute-alias").readlink(),
                         self.root / "restored/ledger/validator-123.log")
        link.unlink()
        link.symlink_to("../../secret")
        with self.assertRaisesRegex(ValueError, "SYMLINK"):
            backup.backup(self.source, self.root / "other")

    def test_runtime_socket_explicitly_omitted_and_fifo_refused(self):
        with socket.socket(socket.AF_UNIX) as sock:
            sock.bind(str(self.source / "admin.rpc"))
            head = self.make()
        data = backup.verify(self.target, head)
        self.assertEqual(data["omitted_runtime_sockets"], ["admin.rpc"])
        os.mkfifo(self.source / "pipe")
        with self.assertRaisesRegex(ValueError, "SPECIAL_FILE"):
            backup.backup(self.source, self.root / "other")

    def test_nested_source_or_existing_target_refused(self):
        for target in [self.source, self.source / "nested", self.root]:
            with self.assertRaisesRegex(ValueError, "OVERLAPPING"):
                backup.backup(self.source, target)
        self.target.mkdir()
        with self.assertRaisesRegex(ValueError, "TARGET_EXISTS"):
            self.make()

    def test_corrupt_or_missing_ledger_refused(self):
        head = self.make()
        path = self.target / "ledger/blockstore"
        path.write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "CONTENT_MISMATCH"):
            backup.restore(self.target, self.root / "restored", head)
        self.assertFalse((self.root / "restored").exists())
        path.unlink()
        with self.assertRaisesRegex(ValueError, "CONTENT_MISMATCH"):
            backup.verify(self.target, head)

    def test_extra_file_refused(self):
        head = self.make()
        (self.target / "unaccounted").write_text("new")
        with self.assertRaisesRegex(ValueError, "CONTENT_MISMATCH"):
            backup.verify(self.target, head)

    def test_rewritten_inventory_cannot_replace_pinned_head(self):
        head = self.make()
        index = self.target / backup.INDEX
        data = json.loads(index.read_text())
        data["files"].pop("ledger/blockstore")
        index.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "HEAD_MISMATCH"):
            backup.verify(self.target, head)

    def test_restoration_never_overwrites(self):
        head = self.make()
        destination = self.root / "restored"
        destination.mkdir()
        with self.assertRaisesRegex(ValueError, "TARGET_EXISTS"):
            backup.restore(self.target, destination, head)

    def test_missing_state_and_keys_are_not_a_resumable_backup(self):
        (self.source / "state.json").unlink()
        with self.assertRaisesRegex(ValueError, "INCOMPLETE_RUN"):
            self.make()

    def test_world_readable_run_or_keys_refused(self):
        self.source.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "PRIVATE_DIRECTORY"):
            self.make()
        self.source.chmod(0o700)
        (self.source / "test-keys.json").chmod(0o644)
        with self.assertRaisesRegex(ValueError, "PRIVATE_KEYS_MODE"):
            self.make()


if __name__ == "__main__":
    unittest.main()
