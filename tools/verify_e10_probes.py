#!/usr/bin/env python3
"""Check coverage of local signed-probe receipts; JSON is not a signature proof."""
import itertools
import json
from pathlib import Path
import sys

directory = Path(sys.argv[1])
PROGRAM = 'FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU'


def read(name):
    data = json.loads((directory / (name + '.json')).read_text())
    assert data['schema'] == 'K4V-E10-V6-SUBMISSION-PROBE-v1' and data['probe'] == name
    assert data['scope'] == 'SIGNED_LOCAL_LITESVM_ON_V6_TEST_SBF' and data['program_id'] == PROGRAM
    assert all(data[k] is True for k in ('program_and_mint_injected', 'clock_controlled', 'fee_airdrops'))
    assert data['financial_transfers_tested'] is False and data['public_chain_transactions'] == 0
    assert data['private_keys_serialized'] is False
    return data['cases']


pairs = set(itertools.product(range(2), (False, True)))
delay = read('signed_delay')
assert len(delay) == 20 and {(x['role'], x['recovery'], x['delay_seconds']) for x in delay} == set(itertools.product(range(2), (False, True), (0, 1, 30, 300, 301)))
assert all(x['accepted'] == (x['delay_seconds'] <= 300) and x['same_valid_blockhash'] for x in delay)
boundary = read('interval_boundaries')
names = {'zero_width', 'negative_start', 'reverse', 'oversize', 'early', 'future_scheduled', 'latest_safe', 'latest_overflow'}
assert len(boundary) == 32 and {(x['role'], x['recovery'], x['case']) for x in boundary} == {(r, m, n) for r, m in pairs for n in names}
assert all(x['accepted'] == (x['case'] in ('zero_width', 'future_scheduled', 'latest_safe')) for x in boundary)
binding = read('signature_binding')
assert len(binding) == 66
fields = {'role', 'mode', 'nonce', 'epoch', 'valid_from', 'valid_until', 'predecessor', 'program', 'policy', 'successor', 'fee_payer_only_resigned', 'resigned_wrong_predecessor', 'all_signers_refreshed'}
for role, mode in pairs:
    group = [x for x in binding if (x['role'], x['recovery']) == (role, mode)]
    assert {x['changed'] for x in group if 'changed' in x} == fields
    assert {x['missing_signature'] for x in group if 'missing_signature' in x} == set(range(4 if mode else 3))
    assert all(x['accepted'] == (x.get('changed') == 'all_signers_refreshed') for x in group)
for name, flags in [('blockhash_separation', ('expired_hash_rejected', 'freshly_signed_valid_hash_accepted')),
                    ('nonce_race', ('competing_rejected', 'successor_cancellation_accepted', 'fresh_hash_replay_after_cancellation_rejected'))]:
    rows = read(name)
    assert len(rows) == 4 and {(x['role'], x['recovery']) for x in rows} == pairs
    assert all(all(x[k] is True for k in flags) for x in rows)
assert sum(x['accepted'] for x in delay + boundary + binding) + 4 + 8 == 44
print('E-10 signed probes: 142 target transactions; 44 accepted / 98 expected refusals; PASS')
