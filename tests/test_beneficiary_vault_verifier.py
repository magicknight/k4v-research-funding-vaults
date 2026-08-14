# SPDX-License-Identifier: MIT OR Apache-2.0
import copy
import unittest

from beneficiary_vault_verifier import (
    MIN_CLIFF_SECONDS,
    PERIOD_SECONDS,
    SNAPSHOT_SCHEMA,
    STATE_SEED,
    TOKEN_VAULT_SEED,
    _base58_encode,
    find_program_address,
    verification_receipt,
    verify_snapshot,
)


PROGRAM_ID_BYTES = bytes(range(1, 33))
BENEFICIARY_BYTES = bytes(range(33, 65))
MINT_BYTES = bytes(range(65, 97))
DEPOSITOR_BYTES = bytes(range(97, 129))
POLICY_HASH = bytes.fromhex("42" * 32)


def valid_snapshot():
    state_pda, state_bump = find_program_address(
        (STATE_SEED, BENEFICIARY_BYTES, MINT_BYTES, POLICY_HASH), PROGRAM_ID_BYTES
    )
    token_pda, token_bump = find_program_address(
        (TOKEN_VAULT_SEED, state_pda), PROGRAM_ID_BYTES
    )
    genesis = 1_800_000_000
    cliff_end = genesis + MIN_CLIFF_SECONDS
    deposit = 1_000_000_000
    monthly_cap = (deposit * 500 // 10_000) // 12
    state_address = _base58_encode(state_pda)
    mint = _base58_encode(MINT_BYTES)
    return {
        "schema": SNAPSHOT_SCHEMA,
        "program_id": _base58_encode(PROGRAM_ID_BYTES),
        "vault_state": state_address,
        "vault_token_account": _base58_encode(token_pda),
        "observed_at_ts": cliff_end,
        "state": {
            "depositor": _base58_encode(DEPOSITOR_BYTES),
            "beneficiary": _base58_encode(BENEFICIARY_BYTES),
            "mint": mint,
            "policy_hash": POLICY_HASH.hex(),
            "deposited_amount": deposit,
            "monthly_cap": monthly_cap,
            "released_total": 0,
            "released_this_period": 0,
            "genesis_ts": genesis,
            "cliff_end_ts": cliff_end,
            "current_period_index": 0,
            "annual_release_bps": 500,
            "mint_decimals": 9,
            "state_bump": state_bump,
            "token_vault_bump": token_bump,
        },
        "token_account": {
            "mint": mint,
            "authority": state_address,
            "amount": deposit,
        },
    }


class BeneficiaryVaultVerifierTests(unittest.TestCase):
    def test_valid_snapshot_recomputes_pdas_and_cap(self):
        result = verify_snapshot(valid_snapshot())
        self.assertTrue(result.valid)
        self.assertEqual(result.reasons, ())
        self.assertEqual(result.expected_monthly_cap, 4_166_666)
        self.assertEqual(result.currently_releasable_amount, 4_166_666)

    def test_rejects_shortened_cliff(self):
        snapshot = valid_snapshot()
        snapshot["state"]["cliff_end_ts"] -= 1
        self.assertIn("CLIFF_TOO_SHORT", verify_snapshot(snapshot).reasons)

    def test_rejects_forged_cap_and_bump(self):
        snapshot = valid_snapshot()
        snapshot["state"]["monthly_cap"] += 1
        snapshot["state"]["state_bump"] -= 1
        reasons = verify_snapshot(snapshot).reasons
        self.assertIn("MONTHLY_CAP_MISMATCH", reasons)
        self.assertIn("STATE_BUMP_MISMATCH", reasons)

    def test_rejects_broken_token_conservation(self):
        snapshot = valid_snapshot()
        snapshot["token_account"]["amount"] -= 1
        self.assertIn("TOKEN_CONSERVATION_FAILURE", verify_snapshot(snapshot).reasons)

    def test_non_carrying_cap_resets_after_a_new_period(self):
        snapshot = valid_snapshot()
        cap = snapshot["state"]["monthly_cap"]
        snapshot["state"]["released_total"] = cap
        snapshot["state"]["released_this_period"] = cap
        snapshot["token_account"]["amount"] -= cap
        snapshot["observed_at_ts"] += PERIOD_SECONDS
        result = verify_snapshot(snapshot)
        self.assertTrue(result.valid)
        self.assertEqual(result.observed_period_index, 1)
        self.assertEqual(result.currently_releasable_amount, cap)

    def test_receipt_is_deterministic_and_tamper_evident(self):
        first = verification_receipt(valid_snapshot())
        second = verification_receipt(valid_snapshot())
        self.assertEqual(first["sha256"], second["sha256"])
        changed = copy.deepcopy(valid_snapshot())
        changed["observed_at_ts"] += 1
        self.assertNotEqual(first["sha256"], verification_receipt(changed)["sha256"])


if __name__ == "__main__":
    unittest.main()
