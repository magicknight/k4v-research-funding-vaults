#!/usr/bin/env python3
"""Adversarial decoder checks on fresh signed-runtime bytes, never synthetic PASS receipts."""
from pathlib import Path
import copy
import json
import sys
import unittest
R = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(R / 'src'))
from e11b_verifier import expand_bundle, verify_bundle, verify_graph
from launch_v7_rpc_exporter import validate_manifest, export
from e11b_recorded_rpc import manifest_for, RecordedRpc
BUNDLE = json.loads((R / 'target/e11b/fresh-bundle.json').read_text())
SNAPSHOTS = expand_bundle(BUNDLE)

class CandidateRawVerification(unittest.TestCase):
    def test_all_financial_checkpoints(self):
        self.assertEqual(verify_bundle(BUNDLE)['checkpoints_verified'], 11)

    def test_prep_each_field_and_padding_rejected(self):
        for offset in (0, 8, 40, 72, 104, 136, 168, 200, 232, 400, 724):
            with self.subTest(offset=offset):
                s = copy.deepcopy(SNAPSHOTS[-1])
                raw = bytearray.fromhex(s['accounts']['preparation']['data_hex']); raw[offset] ^= 1
                s['accounts']['preparation']['data_hex'] = raw.hex()
                with self.assertRaises(ValueError): verify_graph(s)
        s = copy.deepcopy(SNAPSHOTS[-1]); s['accounts']['preparation']['data_hex'] += '00'
        with self.assertRaises(ValueError): verify_graph(s)

    def test_preparation_missing_wrong_owner_or_address(self):
        for field in ('missing', 'owner', 'address'):
            s = copy.deepcopy(SNAPSHOTS[-1])
            if field == 'missing': del s['accounts']['preparation']
            else: s['accounts']['preparation'][field] = '11111111111111111111111111111111'
            with self.assertRaises((ValueError, KeyError)): verify_graph(s)

    def test_program_bytes_and_loader_authority(self):
        for offset in (12, 45, 1000):
            s = copy.deepcopy(SNAPSHOTS[-1]); raw = bytearray.fromhex(s['accounts']['program_data']['data_hex'])
            raw[offset] ^= 1; s['accounts']['program_data']['data_hex'] = raw.hex()
            with self.assertRaises(ValueError): verify_graph(s)

    def test_every_snapshot_export_keeps_preparation_in_same_response(self):
        for s in SNAPSHOTS:
            r = export(RecordedRpc(s), manifest_for(s))
            self.assertTrue(r['verification']['valid'])
            self.assertIn('preparation', r['snapshot']['accounts'])
            self.assertTrue(r['provenance']['single_final_response'])

    def test_manifest_identity_and_network_mismatch(self):
        s = SNAPSHOTS[-1]; m = manifest_for(s)
        for key in ('policy', 'program_id'):
            changed = copy.deepcopy(m); changed['expected'][key] = '11111111111111111111111111111111'
            with self.assertRaises(ValueError): validate_manifest(changed)
        changed = copy.deepcopy(m); changed['expected']['genesis_hash'] = s['accounts']['policy']['address']
        with self.assertRaises(ValueError): export(RecordedRpc(s), changed)

if __name__ == '__main__': unittest.main(verbosity=2)
