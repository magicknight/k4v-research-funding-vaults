# SPDX-License-Identifier: MIT OR Apache-2.0
import json
from pathlib import Path
import unittest


class BeneficiaryVaultIdlTests(unittest.TestCase):
    def test_public_interface_has_exactly_two_instructions(self):
        root = Path(__file__).resolve().parents[1]
        idl = json.loads((root / "idl" / "beneficiary_vault.json").read_text())
        self.assertEqual(
            [instruction["name"] for instruction in idl["instructions"]],
            ["deposit", "release"],
        )
        release = idl["instructions"][1]
        beneficiary = next(
            account for account in release["accounts"] if account["name"] == "beneficiary"
        )
        self.assertTrue(beneficiary["signer"])


if __name__ == "__main__":
    unittest.main()
