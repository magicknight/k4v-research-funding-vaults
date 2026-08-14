# SPDX-License-Identifier: MIT OR Apache-2.0
"""Opt-in live JSON-RPC test against an already running local surfnet.

Run with:

    K4V_SURFPOOL_RPC=http://127.0.0.1:18999 \
      PYTHONPATH=src python3 -m unittest \
      tests/test_beneficiary_vault_rpc_surfpool.py -v

The test uses Surfpool cheatcodes to place deterministic account bytes behind
the RPC boundary. It does not deploy a program, sign, or send a transaction;
the loaded-SBF execution tests remain in Rust/LiteSVM.
"""

import os
import unittest

from beneficiary_vault_rpc_exporter import JsonRpcClient, export_snapshot
from beneficiary_vault_rpc_exporter import CLOCK_SYSVAR_ID
from beneficiary_vault_verifier import verify_snapshot
from test_beneficiary_vault_rpc_exporter import (
    GENESIS,
    MIN_CLIFF_SECONDS,
    fixture_accounts,
)


RPC_URL = os.environ.get("K4V_SURFPOOL_RPC")


@unittest.skipUnless(RPC_URL, "K4V_SURFPOOL_RPC is not set")
class BeneficiaryVaultSurfpoolRpcTests(unittest.TestCase):
    def test_raw_rpc_accounts_reconcile_with_the_independent_verifier(self):
        client = JsonRpcClient(RPC_URL)
        program, state, _, accounts = fixture_accounts()
        for address, account in accounts.items():
            if address == CLOCK_SYSVAR_ID:
                continue
            client._call(  # The cheatcode is test setup, not product behavior.
                "surfnet_setAccount",
                [
                    address,
                    {
                        "lamports": account.lamports,
                        # Surfpool 1.5.0's RPC accepts account data as hex here;
                        # the published cheatcode reference also mentions a
                        # byte array, which this compatibility probe rejects.
                        "data": account.data.hex(),
                        "owner": account.owner,
                        "executable": account.executable,
                        "rent_epoch": 0,
                    },
                ],
            )
        client._call(
            "surfnet_timeTravel",
            [
                {
                    # Surfpool 1.5.0 compares this cheatcode value with its
                    # millisecond wall-clock value, while the Clock sysvar it
                    # writes remains in Unix seconds.
                    "absoluteTimestamp": (GENESIS + MIN_CLIFF_SECONDS) * 1_000
                }
            ],
        )
        snapshot = export_snapshot(client, program, state)
        verification = verify_snapshot(snapshot)
        self.assertTrue(verification.valid, verification.reasons)
        self.assertEqual(verification.currently_releasable_amount, 4_166_666)


if __name__ == "__main__":
    unittest.main()
