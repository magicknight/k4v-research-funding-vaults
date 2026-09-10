"""Adversarial edits to signed-runtime E-10 raw bytes, with no IDL decoder."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import struct
import unittest
import zlib

from beneficiary_vault_verifier import _pubkey
from e10_verifier import expand_bundle, verify_bundle, verify_graph, CODE_BYTES
from launch_v6_verifier import read_policy

ROOT = Path(__file__).resolve().parents[1]


def edit(s, name, offset, value, kind='Q'):
    raw = bytearray.fromhex(s['accounts'][name]['data_hex'])
    if isinstance(value, bytes): raw[offset:offset + len(value)] = value
    else: struct.pack_into('<' + kind, raw, offset, value)
    s['accounts'][name]['data_hex'] = raw.hex()


class V6VerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bundle = json.loads((ROOT/'examples/e10_rehearsal_bundle.json').read_text())
        cls.snapshots = {s['label']: s for s in expand_bundle(cls.bundle)}

    def graph(self, label='continued_year_two'):
        return copy.deepcopy(self.snapshots[label])

    def changed_bundle(self, s):
        b = copy.deepcopy(self.bundle)
        target = next(t for t in b['checkpoints'] if t['label'] == s['label'])
        for name, a in s['accounts'].items():
            raw = bytes.fromhex(a['data_hex']); digest = hashlib.sha256(raw).hexdigest()
            b['blobs'][digest] = base64.b64encode(zlib.compress(raw)).decode()
            target['accounts'][name] = {k: v for k, v in a.items() if k != 'data_hex'} | {'blob': digest}
        return b

    def test_all_raw_checkpoints_reconstruct_recovery_and_full_token_conservation(self):
        result = verify_bundle(self.bundle)
        self.assertEqual(result['checkpoints_verified'], 11)
        self.assertFalse(result['production_ready'])
        self.assertFalse(result['transaction_signatures_verified'])
        last = result['results']['continued_year_two']
        self.assertEqual(last['conserved_supply'], '1000000000000000000')
        self.assertEqual([r['epoch'] for r in last['withdrawal']['roles']], ['1', '1'])
        self.assertEqual(last['withdrawal']['proposals_verified'], '4')
        self.assertIsNone(last['upgrade_authority'])
        self.assertTrue(last['program_bytes_verified'])

    def test_mixed_role_pause_and_expiry_do_not_create_financial_entitlement(self):
        for label, zero in [('recovery_pending', [0, 1]), ('after_founder', [1]),
                            ('normal_pending_expiry', [0]), ('expired', [0, 1])]:
            r = verify_graph(self.graph(label))
            for i in zero: self.assertEqual(r['amount_ceilings_before_transaction_signatures'][i], '0')
        r = verify_graph(self.graph('after_founder'))
        self.assertGreater(int(r['amount_ceilings_before_transaction_signatures'][0]), 0)

    def test_each_recovery_registration_and_t0_are_immutable_identity_inputs(self):
        for offset in (232, 532, 564, 596, 628, 660, 692):
            s = self.graph(); edit(s, 'policy', offset, b'\xff')
            with self.assertRaisesRegex(ValueError, 'POLICY_IDENTITY'): verify_graph(s)

    def test_missing_proposal_key_index_or_zero_consumed_future_approval_fails_closed(self):
        for label, prefix in [('continued_year_two', 'withdrawal_0_2'),
                              ('recovery_pending', 'key_'), ('partial_before', 'approval_13')]:
            s = self.graph(label)
            if prefix == 'key_':
                p = read_policy(s)
                proposal = bytes.fromhex(s['accounts']['withdrawal_0_1']['data_hex'])
                subject = next(n for n, a in s['accounts'].items() if n.startswith('key_') and bytes.fromhex(a['data_hex'])[40:72] == proposal[89:121])
                del s['accounts'][subject]
            else: del s['accounts'][prefix]
            with self.assertRaises((ValueError, KeyError)): verify_graph(s)

    def test_current_key_epoch_nonce_or_pending_link_cannot_be_forged(self):
        for offset, value, kind in [(945, _pubkey(read_policy(self.graph())['founder']), 'Q'),
                                     (977, 0, 'Q'), (985, 4, 'Q'), (993, 3, 'Q'),
                                     (1033, 0, 'Q'), (1057, 2, 'Q')]:
            s = self.graph(); edit(s, 'policy', offset, value, kind)
            with self.assertRaises((ValueError, KeyError)): verify_graph(s)

    def test_proposal_role_predecessor_successor_epoch_and_notice_are_reconstructed(self):
        for offset, value, kind in [(40, 1, 'B'), (41, 2, 'Q'), (49, 1, 'Q'),
                                    (57, bytes(32), 'Q'), (89, bytes(32), 'Q'),
                                    (121, 2, 'B'), (146, 1, 'q'), (154, 1, 'q'), (162, 7, 'B')]:
            s = self.graph(); edit(s, 'withdrawal_0_1', offset, value, kind)
            with self.assertRaises(ValueError): verify_graph(s)

    def test_early_execution_and_premature_expiry_tombstones_are_rejected(self):
        for name, offset_time in [('withdrawal_0_1', 146), ('withdrawal_0_3', 154)]:
            s = self.graph(); raw = bytes.fromhex(s['accounts'][name]['data_hex'])
            threshold = struct.unpack_from('<q', raw, offset_time)[0]
            edit(s, name, 163, threshold - 1, 'q')
            with self.assertRaisesRegex(ValueError, 'WITHDRAWAL_EXECUTION_WINDOW|WITHDRAWAL_EXPIRY'): verify_graph(s)

    def test_signed_admission_bounds_and_actual_start_are_reconstructed(self):
        for field, offset in [('valid_from', 122), ('valid_until', 130)]:
            for value in [-1, 0, 2**63 - 1]:
                with self.subTest(field=field, value=value):
                    s = self.graph(); edit(s, 'withdrawal_0_1', offset, value, 'q')
                    with self.assertRaisesRegex(ValueError, 'WITHDRAWAL_ADMISSION'): verify_graph(s)
        s = self.graph(); raw = bytes.fromhex(s['accounts']['withdrawal_0_1']['data_hex'])
        start = struct.unpack_from('<q', raw, 122)[0]
        edit(s, 'withdrawal_0_1', 130, start + 301, 'q')
        with self.assertRaisesRegex(ValueError, 'WITHDRAWAL_ADMISSION'): verify_graph(s)

    def test_backdating_creation_or_notice_cannot_hide_the_delay(self):
        for offset in (138, 146, 154):
            s = self.graph(); raw = bytes.fromhex(s['accounts']['withdrawal_0_1']['data_hex'])
            value = struct.unpack_from('<q', raw, offset)[0]
            edit(s, 'withdrawal_0_1', offset, value - 30, 'q')
            with self.assertRaisesRegex(ValueError, 'WITHDRAWAL_NOTICE'): verify_graph(s)

    def test_v5_proposal_layout_and_discriminator_cannot_enter_v6_export(self):
        s = self.graph(); a = s['accounts']['withdrawal_0_1']
        raw = bytes.fromhex(a['data_hex'])
        a['data_hex'] = (raw[:122] + raw[138:]).hex()
        with self.assertRaisesRegex(ValueError, 'TRUNCATED_ACCOUNT'): verify_graph(s)
        s = self.graph(); a = s['accounts']['withdrawal_0_1']
        edit(s, 'withdrawal_0_1', 0, hashlib.sha256(b'account:WithdrawalProposalV5').digest()[:8])
        with self.assertRaisesRegex(ValueError, 'DISCRIMINATOR'): verify_graph(s)

    def test_key_record_masks_and_recipient_flag_require_history(self):
        s = self.graph(); name = next(n for n in s['accounts'] if n.startswith('key_'))
        for offset, value in [(72, 4), (73, 1), (74, 2)]:
            x = copy.deepcopy(s); edit(x, name, offset, value, 'B')
            with self.assertRaises(ValueError): verify_graph(x)

    def test_approval_author_epoch_and_recipient_cannot_be_relabelled(self):
        for offset, value in [(80, bytes(32)), (137, bytes(32)), (169, 1)]:
            s = self.graph(); edit(s, 'approval_9', offset, value)
            with self.assertRaises(ValueError): verify_graph(s)

    def test_wrong_account_owner_discriminator_lengths_and_aliases_reject(self):
        for mode in ('owner', 'short', 'long', 'discriminator', 'alias'):
            s = self.graph(); a = s['accounts']['withdrawal_0_1']
            if mode == 'owner': a['owner'] = s['accounts']['mint']['owner']
            elif mode == 'short': a['data_hex'] = a['data_hex'][:-2]
            elif mode == 'long': a['data_hex'] += '00'
            elif mode == 'discriminator': edit(s, 'withdrawal_0_1', 0, bytes(8))
            else: a['address'] = s['accounts']['policy']['address']
            with self.assertRaises(ValueError): verify_graph(s)

    def test_custody_deficit_wrong_destination_or_changed_consumption_cannot_hide(self):
        for name, offset, value in [('founder_token', 64, 1), ('founder_destination_0', 32, bytes(32)),
                                    ('approval_9', 120, 0), ('policy', 799, 1)]:
            s = self.graph(); edit(s, name, offset, value)
            with self.assertRaises(ValueError): verify_graph(s)

    def test_actual_loader_authority_code_and_padding_are_checked(self):
        for offset, value in [(12, 1), (45 + 123, 255), (45 + CODE_BYTES, 1)]:
            s = self.graph(); edit(s, 'program_data', offset, value, 'B')
            with self.assertRaisesRegex(ValueError, 'IMMUTABLE|PINNED_CODE'): verify_graph(s)
        s = self.graph(); edit(s, 'program', 4, bytes(32))
        with self.assertRaisesRegex(ValueError, 'LOADER_PROGRAM_POINTER'): verify_graph(s)

    def test_clock_timestamp_slot_or_future_loader_deployment_reject(self):
        s = self.graph(); s['slot'] = str(int(s['slot']) + 1)
        with self.assertRaisesRegex(ValueError, 'CLOCK_CONTEXT'): verify_graph(s)
        s = self.graph(); edit(s, 'clock', 32, int(s['now']) - 1, 'q')
        with self.assertRaisesRegex(ValueError, 'CLOCK_CONTEXT'): verify_graph(s)
        s = self.graph(); edit(s, 'program_data', 4, int(s['slot']) + 1)
        with self.assertRaisesRegex(ValueError, 'LOADER_FUTURE_SLOT'): verify_graph(s)

    def test_recovery_cannot_change_an_individually_valid_approval_between_checkpoints(self):
        s = self.graph('after_founder'); a = bytes.fromhex(s['accounts']['approval_13']['data_hex'])
        edit(s, 'approval_13', 112, struct.unpack_from('<Q', a, 112)[0] + 1)
        self.assertTrue(verify_graph(s)['valid'])
        with self.assertRaisesRegex(ValueError, 'RECOVERY_MUTATED_CUSTODY_OR_APPROVAL'): verify_bundle(self.changed_bundle(s))

    def test_bundle_order_and_corrupted_raw_blob_are_not_trusted(self):
        b = copy.deepcopy(self.bundle); b['checkpoints'][0], b['checkpoints'][1] = b['checkpoints'][1], b['checkpoints'][0]
        with self.assertRaisesRegex(ValueError, 'CHECKPOINT_SEQUENCE'): verify_bundle(b)
        b = copy.deepcopy(self.bundle); digest = next(iter(b['blobs'])); b['blobs'][digest] = base64.b64encode(zlib.compress(b'forged')).decode()
        with self.assertRaisesRegex(ValueError, 'BLOB_HASH'): verify_bundle(b)


if __name__ == '__main__': unittest.main()
