"""Adversarial edits to runtime-produced bytes; no generated IDL decoding."""
import copy
import json
from pathlib import Path
import struct
import unittest

from beneficiary_vault_verifier import _pubkey
from launch_v4_verifier import verify, read_policy, PERIOD
from e05_verifier import verify_graph, verify_bundle, expand_bundle, VAULT

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "examples/e05_rehearsal_bundle.json"


def edit(snapshot, name, offset, value, kind="Q"):
    raw = bytearray.fromhex(snapshot["accounts"][name]["data_hex"])
    if isinstance(value, bytes):
        raw[offset:offset + len(value)] = value
    else:
        struct.pack_into("<" + kind, raw, offset, value)
    snapshot["accounts"][name]["data_hex"] = raw.hex()


class V4VerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bundle = json.loads(BUNDLE.read_text())
        cls.graphs = {s["label"]: s for s in expand_bundle(cls.bundle)}

    def graph(self, label="annual_boundary"):
        return copy.deepcopy(self.graphs[label])

    def test_e04_frozen_raw_fixture_is_supported_without_claiming_loader_verification(self):
        r = verify(json.loads((ROOT / "examples/launch_v4_raw_snapshot.json").read_text()))
        self.assertTrue(r["valid"])
        self.assertFalse(r["program_bytes_verified"])
        self.assertEqual(r["amount_ceilings_before_transaction_signatures"], ["600000000000000", "900000000000000"])

    def test_combined_runtime_graphs_reconstruct_keys_conservation_and_code_change(self):
        r = verify_bundle(self.bundle)
        self.assertTrue(r["valid"])
        self.assertEqual(r["checkpoints_verified"], 12)
        self.assertFalse(r["production_ready"])
        self.assertFalse(r["transaction_signatures_verified"])
        last = r["results"]["annual_boundary"]
        self.assertEqual(last["conserved_supply"], "1000000000000000000")
        self.assertEqual(last["governance"]["controller_epoch"], "1")
        self.assertEqual(last["governance"]["oracle_epoch"], "1")
        self.assertEqual(last["governance"]["change_sequence"], "3")
        self.assertFalse(last["governance"]["withdrawal_key_recovery_supported"])

    def test_envelope_alias_length_discriminator_and_noncanonical_boolean_reject(self):
        for mode in ("owner", "executable", "alias", "short", "long", "discriminator", "boolean"):
            with self.subTest(mode=mode):
                s = self.graph()
                a = s["accounts"]["policy"]
                if mode == "owner": a["owner"] = VAULT + "1"
                elif mode == "executable": a["executable"] = True
                elif mode == "alias": s["accounts"]["mint"]["address"] = a["address"]
                elif mode == "short": a["data_hex"] = a["data_hex"][:-2]
                elif mode == "long": a["data_hex"] += "00"
                elif mode == "discriminator": edit(s, "policy", 0, bytes(8))
                else: edit(s, "policy", 736, 2, "B")
                with self.assertRaises(ValueError): verify_graph(s)

    def test_immutable_recovery_registration_or_annual_input_change_breaks_identity(self):
        for offset in (436, 232 + 56 + 40):
            s = self.graph()
            edit(s, "policy", offset, b"\x00")
            with self.assertRaisesRegex(ValueError, "POLICY_IDENTITY"): verify(s)

    def test_missing_tombstone_or_unexplained_current_key_and_epoch_reject(self):
        for mode in ("missing", "controller", "oracle_epoch", "pending"):
            s = self.graph()
            if mode == "missing": del s["accounts"]["change_2"]
            elif mode == "controller": edit(s, "policy", 672, _pubkey(VAULT))
            elif mode == "oracle_epoch": edit(s, "policy", 712, 2)
            else: edit(s, "policy", 745, 3)
            with self.assertRaises(ValueError): verify(s)

    def test_proposal_wrong_policy_nonce_epoch_short_notice_and_status_reject(self):
        for offset, value, kind in ((8, _pubkey(VAULT), "Q"), (40, 2, "Q"),
                                    (82, 1, "Q"), (106, 0, "q"), (114, 7, "B"), (49, 2, "B")):
            with self.subTest(offset=offset):
                s = self.graph("pending_oracle_upgrade")
                edit(s, "change_1", offset, value, kind)
                with self.assertRaises(ValueError): verify(s)

    def test_pending_maturity_minus_one_and_exact_do_not_change_current_key(self):
        s = self.graph("notice_minus_one")
        before = verify(s)["governance"]
        self.assertFalse(before["pending"]["mature"])
        s["now"] = str(int(s["now"]) + 1)
        after = verify(s)["governance"]
        self.assertTrue(after["pending"]["mature"])
        self.assertEqual(before["oracle"], after["oracle"])

    def test_stale_epoch_cannot_be_revalidated_by_summary_or_report_flag(self):
        s = self.graph("after_oracle_recovery")
        s["claimed_valid"] = True
        self.assertEqual(verify(s)["amount_ceilings_before_transaction_signatures"], ["0", "0"])
        edit(s, "policy", 736, 1, "B")
        with self.assertRaisesRegex(ValueError, "REPORT_GENERATION"): verify(s)
        s = self.graph()
        edit(s, "policy", 728, 0)
        with self.assertRaisesRegex(ValueError, "REPORT_GENERATION"): verify(s)

    def test_escrow_deficit_and_delegation_and_conservation_fail(self):
        for name, offset, value, kind in (("founder_token", 64, 0, "Q"),
                ("treasury_token", 72, 1, "I"), ("source", 64, 1, "Q")):
            s = self.graph()
            edit(s, name, offset, value, kind)
            with self.assertRaises(ValueError): verify(s)

    def test_accounting_counters_and_treasury_notice_are_independently_checked(self):
        for name, offset, value, kind in (("policy", 591, 0, "Q"), ("policy", 623, 0, "B"),
                ("approval", 112, 0, "Q"), ("approval", 128, int(self.graph()["now"]), "q")):
            s = self.graph()
            edit(s, name, offset, value, kind)
            with self.assertRaises(ValueError): verify(s)

    def test_correction_staleness_and_unconfigured_future_year_block_releases(self):
        s = self.graph()
        edit(s, "policy", 567, 1)
        self.assertTrue(verify(s)["limits"]["correction_pause"])
        self.assertEqual(verify(s)["amount_ceilings_before_transaction_signatures"], ["0", "0"])
        s = self.graph()
        s["now"] = str(int(s["now"]) + 86_401)
        self.assertEqual(verify(s)["amount_ceilings_before_transaction_signatures"], ["0", "0"])
        s["now"] = str(read_policy(s)["config"]["t0"] + 24 * PERIOD)
        self.assertEqual(verify(s)["limits"]["blocked"], "NO_ANNUAL_INPUT")

    def test_real_loader_programdata_pda_owners_and_authority_bypass_reject(self):
        for mode in ("pda", "owner", "target_authority", "mutable_gate", "tag", "option"):
            s = self.graph()
            if mode == "pda": s["accounts"]["target_programdata"]["address"] = s["accounts"]["policy"]["address"]
            elif mode == "owner": s["accounts"]["target_program"]["owner"] = VAULT
            elif mode == "target_authority": edit(s, "target_programdata", 13, _pubkey(VAULT))
            elif mode == "mutable_gate": edit(s, "gate_programdata", 12, 1, "B")
            elif mode == "tag": edit(s, "target_programdata", 0, 2, "I")
            else: edit(s, "target_programdata", 12, 2, "B")
            with self.subTest(mode=mode), self.assertRaises(ValueError): verify_graph(s)

    def test_code_hash_padding_and_input_supplied_pin_forgery_reject(self):
        for offset in (45, -1):
            s = self.graph()
            raw = bytearray.fromhex(s["accounts"]["target_programdata"]["data_hex"])
            raw[offset] ^= 1
            s["accounts"]["target_programdata"]["data_hex"] = raw.hex()
            s["trusted_program_hash"] = "forged"
            with self.assertRaises(ValueError): verify_graph(s)

    def test_pending_buffer_authority_bytes_hash_length_and_notice_reject(self):
        for name, offset, value, kind in (("upgrade_buffer", 5, _pubkey(VAULT), "Q"),
                ("upgrade_buffer", 37, b"\0", "Q"), ("upgrade_gate", 241, 1, "Q"),
                ("upgrade_gate", 289, 0, "q"), ("upgrade_gate", 104, _pubkey(VAULT), "Q")):
            s = self.graph("pending_oracle_upgrade")
            if offset == 104: value = bytes.fromhex(s["accounts"]["upgrade_gate"]["data_hex"])[72:104]
            edit(s, name, offset, value, kind)
            with self.subTest(name=name, offset=offset), self.assertRaises(ValueError): verify_graph(s)

    def test_executed_gate_must_bind_current_code_and_not_claim_early_execution(self):
        for offset, value, kind in ((209, bytes(32), "Q"), (297, 0, "q")):
            s = self.graph()
            edit(s, "upgrade_gate", offset, value, kind)
            with self.assertRaises(ValueError): verify_graph(s)

    def test_clock_timestamp_future_deploy_slot_and_bundle_compression_tamper_reject(self):
        s = self.graph()
        s["now"] = str(int(s["now"]) + 1)
        with self.assertRaisesRegex(ValueError, "CLOCK_TIMESTAMP"): verify_graph(s)
        s = self.graph()
        edit(s, "target_programdata", 4, 2**64 - 1)
        with self.assertRaisesRegex(ValueError, "LOADER_FUTURE_SLOT"): verify_graph(s)
        b = copy.deepcopy(self.bundle)
        digest = next(iter(b["blobs"]))
        b["blobs"]["0" * 64] = b["blobs"].pop(digest)
        with self.assertRaisesRegex(ValueError, "BLOB_HASH"): expand_bundle(b)

    def test_bundle_missing_reordered_and_duplicate_checkpoints_do_not_claim_full_rehearsal(self):
        for mode in ("missing", "reordered", "duplicate"):
            b = copy.deepcopy(self.bundle)
            if mode == "missing": b["checkpoints"].pop()
            elif mode == "reordered": b["checkpoints"].reverse()
            else: b["checkpoints"][1] = b["checkpoints"][0]
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, "REHEARSAL_CHECKPOINT_SEQUENCE"):
                verify_bundle(b)


if __name__ == "__main__":
    unittest.main()
