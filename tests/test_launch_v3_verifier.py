"""Mutate runtime-produced raw bytes; the decoder never trusts summary fields."""
import copy
import json
from pathlib import Path
import random
import struct
import unittest

from launch_v3_verifier import quotas, read_policy, verify, PERIOD, U64

FIXTURE = Path(__file__).resolve().parents[1] / "examples/launch_v3_raw_snapshot.json"


class V3VerifierTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = json.loads(FIXTURE.read_text())

    def mutate(self, account, offset, value, kind="Q"):
        data = bytearray.fromhex(self.snapshot["accounts"][account]["data_hex"])
        struct.pack_into("<" + kind, data, offset, value)
        self.snapshot["accounts"][account]["data_hex"] = data.hex()

    def test_runtime_graph_conserves_full_supply_and_reconstructs_reserved_shares(self):
        result = verify(self.snapshot)
        self.assertTrue(result["valid"])
        self.assertFalse(result["on_chain_authenticity_verified"])
        self.assertEqual(result["conserved_supply"], "1000000000000000000")
        self.assertEqual(result["limits"]["quotas"], ["800000000000000", "1200000000000000"])
        self.assertEqual(result["limits"]["annual_used"], ["200000000000000", "300000000000000"])
        self.assertEqual(result["amount_ceilings_before_transaction_signatures"], ["600000000000000", "900000000000000"])

    def test_wrong_owner_discriminator_and_appended_bytes_reject(self):
        for modification in ("owner", "discriminator", "trailing"):
            with self.subTest(modification=modification):
                s = copy.deepcopy(self.snapshot)
                a = s["accounts"]["policy"]
                if modification == "owner":
                    a["owner"] = s["accounts"]["mint"]["owner"]
                elif modification == "discriminator":
                    a["data_hex"] = "00" * 8 + a["data_hex"][16:]
                else:
                    a["data_hex"] += "00"
                with self.assertRaises(ValueError):
                    verify(s)

    def test_annual_input_tamper_breaks_bound_identity(self):
        # Fixed ABI: config begins after discriminator + five keys + two hashes.
        self.mutate("policy", 232 + 56 + 40, 250, "H")
        with self.assertRaisesRegex(ValueError, "POLICY_IDENTITY"):
            verify(self.snapshot)

    def test_custody_deficit_and_escrow_delegation_reject(self):
        self.mutate("founder_token", 64, 0)
        with self.assertRaisesRegex(ValueError, "CUSTODY_DEFICIT"):
            verify(self.snapshot)
        self.setUp()
        self.mutate("treasury_token", 72, 1, "I")
        with self.assertRaisesRegex(ValueError, "TOKEN_VAULT_AUTHORITY"):
            verify(self.snapshot)

    def test_counter_mismatch_and_wrong_annual_index_reject(self):
        # Policy layout is independently fixed at 544 bytes.
        self.mutate("policy", 495, 0)  # founder_period_used
        with self.assertRaisesRegex(ValueError, "PERIOD_ACCOUNTING"):
            verify(self.snapshot)
        self.setUp()
        self.mutate("policy", 527, 1, "B")
        with self.assertRaisesRegex(ValueError, "ANNUAL_INDEX_PERIOD"):
            verify(self.snapshot)

    def test_capacity_correction_is_valid_state_but_blocks_both_pools(self):
        # Directly changing report_capacity models a possible signed oracle
        # correction; authentication of that report is outside this verifier.
        self.mutate("policy", 471, 100_000_000_000_000)
        result = verify(self.snapshot)
        self.assertTrue(result["limits"]["correction_pause"])
        self.assertEqual(result["amount_ceilings_before_transaction_signatures"], ["0", "0"])

    def test_clock_staleness_year_transition_and_missing_input_fail_closed(self):
        p = read_policy(self.snapshot)
        self.snapshot["now"] = str(p["last_action_at"] - 1)
        with self.assertRaisesRegex(ValueError, "CLOCK_ROLLBACK"):
            verify(self.snapshot)
        for now in [p["last_action_at"] + 86_401, p["config"]["t0"] + 12 * PERIOD]:
            self.snapshot["now"] = str(now)
            self.assertEqual(verify(self.snapshot)["amount_ceilings_before_transaction_signatures"], ["0", "0"])
        self.snapshot["now"] = str(p["config"]["t0"] + 24 * PERIOD)
        result = verify(self.snapshot)
        self.assertEqual(result["limits"]["blocked"], "NO_ANNUAL_INPUT")
        self.assertEqual(result["amount_ceilings_before_transaction_signatures"], ["0", "0"])

    def test_integer_reference_conservation_and_rounding_at_u64_scale(self):
        rng = random.Random(8403)
        for _ in range(1000):
            c, f, t = (rng.randrange(U64 + 1) for _ in range(3))
            qf, qt = quotas(c, f, t, True)
            self.assertEqual(qf + qt, min(c, f + t))
            self.assertLessEqual(qf, f)
            self.assertLessEqual(qt, t)
            if f + t:
                rounding_error = min(c, f + t) * f - qf * (f + t)
                self.assertGreaterEqual(rounding_error, 0)
                self.assertLess(rounding_error, f + t)
        self.assertEqual(quotas(U64, U64, U64, True), [U64 // 2, U64 // 2 + 1])


if __name__ == "__main__":
    unittest.main()
