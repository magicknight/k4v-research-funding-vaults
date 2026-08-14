# SPDX-License-Identifier: MIT OR Apache-2.0
import struct
import unittest

from beneficiary_vault_rpc_exporter import (
    CLOCK_ACCOUNT_DATA_LEN,
    CLOCK_SYSVAR_ID,
    STATE_ACCOUNT_DATA_LEN,
    STATE_DISCRIMINATOR,
    SYSVAR_PROGRAM_ID,
    TOKEN_ACCOUNT_DATA_LEN,
    TOKEN_PROGRAM_ID,
    RawAccount,
    RpcExportError,
    export_snapshot,
)
from beneficiary_vault_verifier import (
    MIN_CLIFF_SECONDS,
    STATE_SEED,
    TOKEN_VAULT_SEED,
    _base58_encode,
    find_program_address,
    verify_snapshot,
)


PROGRAM = bytes(range(1, 33))
BENEFICIARY = bytes(range(33, 65))
MINT = bytes(range(65, 97))
DEPOSITOR = bytes(range(97, 129))
POLICY_HASH = bytes.fromhex("42" * 32)
GENESIS = 1_800_000_000
DEPOSIT = 1_000_000_000
CAP = 4_166_666


class FakeReader:
    def __init__(self, accounts):
        self.accounts = accounts

    def get_account_info(self, address):
        try:
            return self.accounts[address]
        except KeyError as exc:
            raise RpcExportError("fixture account absent") from exc


def fixture_accounts():
    state_address, state_bump = find_program_address(
        (STATE_SEED, BENEFICIARY, MINT, POLICY_HASH), PROGRAM
    )
    token_address, token_bump = find_program_address(
        (TOKEN_VAULT_SEED, state_address), PROGRAM
    )
    state_data = b"".join(
        (
            STATE_DISCRIMINATOR,
            DEPOSITOR,
            BENEFICIARY,
            MINT,
            POLICY_HASH,
            struct.pack(
                "<QQQQqqQHBBB",
                DEPOSIT,
                CAP,
                0,
                0,
                GENESIS,
                GENESIS + MIN_CLIFF_SECONDS,
                0,
                500,
                9,
                state_bump,
                token_bump,
            ),
        )
    )
    assert len(state_data) == STATE_ACCOUNT_DATA_LEN

    token_data = bytearray(TOKEN_ACCOUNT_DATA_LEN)
    token_data[0:32] = MINT
    token_data[32:64] = state_address
    struct.pack_into("<Q", token_data, 64, DEPOSIT)
    token_data[108] = 1

    clock_data = bytearray(CLOCK_ACCOUNT_DATA_LEN)
    struct.pack_into("<q", clock_data, 32, GENESIS + MIN_CLIFF_SECONDS)
    program = _base58_encode(PROGRAM)
    state = _base58_encode(state_address)
    token = _base58_encode(token_address)
    accounts = {
        state: RawAccount(program, state_data, False, 10_000_000),
        token: RawAccount(TOKEN_PROGRAM_ID, bytes(token_data), False, 10_000_000),
        CLOCK_SYSVAR_ID: RawAccount(
            SYSVAR_PROGRAM_ID, bytes(clock_data), False, 1
        ),
    }
    return program, state, token, accounts


class BeneficiaryVaultRpcExporterTests(unittest.TestCase):
    def test_exports_authenticated_snapshot_that_verifier_accepts(self):
        program, state, token, accounts = fixture_accounts()
        snapshot = export_snapshot(FakeReader(accounts), program, state)
        self.assertEqual(snapshot["vault_token_account"], token)
        self.assertEqual(snapshot["token_account"]["state"], "initialized")
        self.assertTrue(verify_snapshot(snapshot).valid)

    def test_rejects_wrong_state_owner(self):
        program, state, _, accounts = fixture_accounts()
        original = accounts[state]
        accounts[state] = RawAccount(TOKEN_PROGRAM_ID, original.data, False, 1)
        with self.assertRaisesRegex(RpcExportError, "owner mismatch"):
            export_snapshot(FakeReader(accounts), program, state)

    def test_rejects_wrong_discriminator(self):
        program, state, _, accounts = fixture_accounts()
        original = accounts[state]
        accounts[state] = RawAccount(
            original.owner, b"X" * 8 + original.data[8:], False, original.lamports
        )
        with self.assertRaisesRegex(RpcExportError, "discriminator mismatch"):
            export_snapshot(FakeReader(accounts), program, state)

    def test_rejects_truncated_token_account(self):
        program, state, token, accounts = fixture_accounts()
        original = accounts[token]
        accounts[token] = RawAccount(
            original.owner, original.data[:-1], False, original.lamports
        )
        with self.assertRaisesRegex(RpcExportError, "data length mismatch"):
            export_snapshot(FakeReader(accounts), program, state)

    def test_rejects_noncanonical_state_pda(self):
        program, state, _, accounts = fixture_accounts()
        wrong_state = _base58_encode(bytes(reversed(range(1, 33))))
        accounts[wrong_state] = accounts.pop(state)
        with self.assertRaisesRegex(RpcExportError, "PDA reconciliation failed"):
            export_snapshot(FakeReader(accounts), program, wrong_state)


if __name__ == "__main__":
    unittest.main()
