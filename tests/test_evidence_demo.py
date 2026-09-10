import copy
import hashlib
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from evidence_demo import MANIFESTS, ROOT, build_report, compose_report, render_html, verify_manifests


class EvidenceDemoTest(unittest.TestCase):
    def setUp(self):
        self.init = dict(valid=True, scope="RECHECK_OF_ARCHIVED_AUTHOR_LOCAL_EVIDENCE",
                         financial_checkpoints=11, agave_account_checkpoints=4, preparation=True,
                         independent_human_review=False, live_rpc_observed_by_this_command=False)
        phase = dict(valid=True, raw_checkpoints=4, authority_action_preserved_counters_and_budgets=True,
                     supply_conserved=True, bounded_withdrawals_after_authority_action=True,
                     application_state_preloaded=True, fixture_clock_controlled=True,
                     natural_90_180_day_soak=False, independent_human_review=False, production_ready=False)
        self.cont = dict(valid=True, mode="OFFLINE_ARCHIVED_BYTES_REPLAY", finalized_transactions_in_archive=11,
                         raw_checkpoints_replayed=8, application_state_preloaded=True, natural_90_180_day_soak=False,
                         independent_human_review=False, production_ready=False,
                         phases={name: dict(phase, phase=name, year_two_checked=name == "expiry") for name in ("recovery", "expiry")})

    def report(self):
        return compose_report(self.init, self.cont, 3)

    def test_bounded_report(self):
        result = self.report()
        self.assertEqual(len(result["demonstrations"]), 5)
        self.assertEqual(result["boundaries"]["new_transactions"], 0)
        self.assertFalse(result["boundaries"]["production_ready"])

    def test_missing_initialization(self):
        self.init.pop("valid")
        with self.assertRaises(ValueError): self.report()

    def test_invalid_continuation(self):
        self.cont["valid"] = False
        with self.assertRaises(ValueError): self.report()

    def test_false_scope_claims_rejected(self):
        for key in ("production_ready", "natural_90_180_day_soak", "independent_human_review"):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.cont)
                changed[key] = True
                with self.assertRaises(ValueError): compose_report(self.init, changed, 3)

    def test_phase_claims_rejected(self):
        for key in ("production_ready", "natural_90_180_day_soak", "independent_human_review"):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.cont)
                changed["phases"]["recovery"][key] = True
                with self.assertRaises(ValueError): compose_report(self.init, changed, 3)

    def test_preload_cannot_be_hidden(self):
        self.cont["application_state_preloaded"] = False
        with self.assertRaises(ValueError): self.report()

    def test_failed_financial_check_rejected(self):
        self.cont["phases"]["recovery"]["supply_conserved"] = False
        with self.assertRaises(ValueError): self.report()

    def test_live_rpc_claim_rejected(self):
        self.init["live_rpc_observed_by_this_command"] = True
        with self.assertRaises(ValueError): self.report()

    def test_missing_phase(self):
        self.cont["phases"].pop("expiry")
        with self.assertRaises(ValueError): self.report()

    def test_extra_phase(self):
        self.cont["phases"]["fabricated"] = {}
        with self.assertRaises(ValueError): self.report()

    def test_count_is_exact_integer(self):
        self.cont["finalized_transactions_in_archive"] = "11"
        with self.assertRaises(ValueError): self.report()

    def test_year_two_required(self):
        self.cont["phases"]["expiry"]["year_two_checked"] = False
        with self.assertRaises(ValueError): self.report()

    def test_html_escapes_data_and_has_no_script(self):
        report = self.report()
        report["demonstrations"][0]["name"] = '<script>alert("x")</script>'
        result = render_html(report)
        self.assertNotIn("<script>", result)
        self.assertIn("&lt;script&gt;", result)
        self.assertIn("NOT A LIVE DAPP", result)
        self.assertIn("default-src 'none'", result)

    def test_manifest_rejects_corruption_and_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "data").write_bytes(b"original")
            line = hashlib.sha256(b"original").hexdigest() + "  data\n"
            for name in MANIFESTS: (root / name).write_text(line)
            self.assertEqual(verify_manifests(root), 3)
            (root / "data").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "CHECKSUM_MISMATCH"): verify_manifests(root)
            (root / MANIFESTS[0]).write_text(hashlib.sha256(b"original").hexdigest() + "  ../outside\n")
            with self.assertRaisesRegex(ValueError, "MANIFEST_PATH"): verify_manifests(root)

    def test_manifest_symlink_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "data").write_bytes(b"original")
            (root / "link").symlink_to(root / "data")
            line = hashlib.sha256(b"original").hexdigest() + "  link\n"
            for name in MANIFESTS: (root / name).write_text(line)
            with self.assertRaisesRegex(ValueError, "MANIFEST_ESCAPE"): verify_manifests(root)

    def test_checksum_failure_prevents_verifier_execution(self):
        with patch("evidence_demo.verify_manifests", side_effect=ValueError("CHECKSUM_MISMATCH")):
            with patch("evidence_demo.run_verifier") as runner:
                with self.assertRaises(ValueError): build_report()
                runner.assert_not_called()

    def test_real_cli_json_is_bounded(self):
        run = subprocess.run([sys.executable, "-E", "-B", str(ROOT / "tools/evidence_demo.py")],
                             capture_output=True, text=True, timeout=120)
        self.assertEqual(run.returncode, 0, run.stderr)
        report = json.loads(run.stdout)
        self.assertEqual(report["boundaries"]["new_transactions"], 0)
        self.assertGreater(report["files_checksum_verified"], 0)
        self.assertFalse(report["boundaries"]["production_ready"])

    def test_real_cli_refuses_to_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "existing.html"
            target.write_text("preserve this user file")
            run = subprocess.run([sys.executable, "-E", "-B", str(ROOT / "tools/evidence_demo.py"),
                                  "--format", "html", "--output", str(target)],
                                 capture_output=True, text=True, timeout=120)
            self.assertEqual(run.returncode, 1)
            self.assertEqual(target.read_text(), "preserve this user file")


if __name__ == "__main__":
    unittest.main()
