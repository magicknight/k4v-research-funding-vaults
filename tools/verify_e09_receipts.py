#!/usr/bin/env python3
"""Check local probe coverage and outcomes, not chain signatures/provenance."""
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
receipts = {name: json.loads((root / (name + '.json')).read_text())
            for name in ('clock_delay', 'signature_refresh', 'nonce_replay')}
for name, r in receipts.items():
    assert r['schema'] == 'K4V-E09-V5-CLOCK-PROBE-v1' and r['probe'] == name
    assert r['scope'] == 'SIGNED_LOCAL_LITESVM_ON_UNCHANGED_V5_SBF'
    assert r['program_id'] == 'EUhWSZAfU8hDki7AXskYrwh8ErwXN8iqicaHTP7yQfYS'
    assert r['program_and_mint_injected'] is True and r['clock_controlled'] is True and r['fee_airdrops'] is True
    assert r['financial_transfers_tested'] is False
    assert r['public_chain_transactions'] == 0 and r['private_keys_serialized'] is False
delay = receipts['clock_delay']['cases']
assert len(delay) == 16 and {(x['role'], x['recovery'], x['delay_seconds']) for x in delay} == {
    (r, m, d) for r in (0, 1) for m in (False, True) for d in (0, 1, 30, 300)}
for x in delay:
    assert x['same_valid_blockhash'] is True
    assert x['outcome'] == ('ACCEPTED' if x['delay_seconds'] == 0 else 'PROPOSAL_CLOCK_REJECTED_STATE_UNCHANGED')
for name in ('signature_refresh', 'nonce_replay'):
    cases = receipts[name]['cases']
    assert len(cases) == 4 and {(x['role'], x['recovery']) for x in cases} == {(r, m) for r in (0, 1) for m in (False, True)}
    for x in cases:
        if name == 'signature_refresh':
            assert x['tamper'] == 'SIGNATURE_FAILURE_STATE_UNCHANGED' and x['fresh_signing'] == 'ACCEPTED_AT_NEW_EXACT_CLOCK'
        else:
            assert x['first'] == 'ACCEPTED' and x['newly_signed_same_nonce'] == 'PDA_ALREADY_IN_USE_STATE_UNCHANGED'
print('E-09 local probe receipts: 32 target submissions, 12 successes, 20 expected refusals; no public transactions')
