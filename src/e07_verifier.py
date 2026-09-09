"""E-07 raw-account, pinned immutable local loader and continuity verification.

Independent Python decoding, not an unrelated human audit or chain proof.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address
from launch_v5_verifier import PROGRAM, verify, read_policy, require
from e05_verifier import expand_bundle as expand_checked_blobs

LOADER = 'BPFLoaderUpgradeab1e11111111111111111111111'
CLOCK = 'SysvarC1ock11111111111111111111111111111111'
SYSVAR = 'Sysvar1111111111111111111111111111111111111'
CODE_BYTES = 528160
CODE_SHA256 = '1dca41034f873882c41cc3eaaecc5b0124ae497f3ae41263a50ffccf9d339f14'
LABELS = ('partial_before', 'recovery_pending', 'notice_minus_one', 'before_execute',
          'after_founder', 'after_treasury', 'continued_period_9', 'normal_cancelled',
          'normal_pending_expiry', 'expired', 'continued_year_two')


def verify_graph(s):
    result = verify(s)
    a, d, c = (s['accounts'][n] for n in ('program', 'program_data', 'clock'))
    pda, _ = find_program_address((_pubkey(PROGRAM),), _pubkey(LOADER))
    require(a['address'] == PROGRAM and a['owner'] == LOADER and a['executable'] is True, 'LOADER_PROGRAM')
    raw = bytes.fromhex(a['data_hex'])
    require(len(raw) == 36 and struct.unpack('<I', raw[:4])[0] == 2 and raw[4:] == pda, 'LOADER_PROGRAM_POINTER')
    require(d['address'] == _base58_encode(pda) and d['owner'] == LOADER and d['executable'] is False, 'LOADER_PROGRAMDATA')
    data = bytes.fromhex(d['data_hex'])
    require(45 + CODE_BYTES <= len(data) <= 2_000_000 and struct.unpack('<I', data[:4])[0] == 3, 'LOADER_DATA_LAYOUT')
    require(data[12] == 0, 'TEST_CANDIDATE_MUST_BE_IMMUTABLE')
    require(hashlib.sha256(data[45:45 + CODE_BYTES]).hexdigest() == CODE_SHA256
            and not any(data[45 + CODE_BYTES:]), 'PINNED_CODE_OR_PADDING')
    require(c['address'] == CLOCK and c['owner'] == SYSVAR and c['executable'] is False, 'CLOCK_ACCOUNT')
    clock = bytes.fromhex(c['data_hex'])
    require(len(clock) == 40, 'CLOCK_LAYOUT')
    slot, _, _, _, timestamp = struct.unpack('<QqQQq', clock)
    require(type(s['slot']) is str and s['slot'].isdigit() and slot == int(s['slot'])
            and timestamp == int(s['now']), 'CLOCK_CONTEXT')
    deploy_slot = struct.unpack('<Q', data[4:12])[0]
    require(deploy_slot <= slot, 'LOADER_FUTURE_SLOT')
    result.update(program_bytes_verified=True, upgrade_authority=None,
                  program_sha256=CODE_SHA256, production_ready=False,
                  public_chain_authenticity_verified=False, transaction_signatures_verified=False)
    return result


def expand_bundle(bundle):
    require(bundle['schema'] == 'K4V-E07-REHEARSAL-BUNDLE-v1', 'BUNDLE_SCHEMA')
    return expand_checked_blobs({**bundle, 'schema': 'K4V-E05-REHEARSAL-BUNDLE-v1'})


def verify_bundle(bundle):
    snapshots = expand_bundle(bundle)
    require(tuple(s['label'] for s in snapshots) == LABELS, 'CHECKPOINT_SEQUENCE')
    results = {s['label']: verify_graph(s) for s in snapshots}
    policies = [read_policy(s) for s in snapshots]
    require(len({p['identity'] for p in policies}) == 1, 'REHEARSAL_IDENTITY_CHANGED')
    for before, after, s, t in zip(policies, policies[1:], snapshots, snapshots[1:]):
        require(int(s['now']) <= int(t['now']) and int(s['slot']) < int(t['slot']), 'CHECKPOINT_CLOCK_ORDER')
        for k in ('founder_released_total', 'treasury_released_total', 'report_sequence',
                  'last_action_at', 'change_sequence', 'approval_count'):
            require(before[k] <= after[k], 'REHEARSAL_COUNTER_ROLLBACK')
    by = {s['label']: s for s in snapshots}
    for a, b in (('before_execute', 'after_founder'), ('after_founder', 'after_treasury'),
                 ('continued_period_9', 'normal_cancelled'), ('normal_pending_expiry', 'expired')):
        x, y = read_policy(by[a]), read_policy(by[b])
        require({k: v for k, v in x.items() if k not in ('last_action_at', 'withdrawal')}
                == {k: v for k, v in y.items() if k not in ('last_action_at', 'withdrawal')}, 'RECOVERY_MUTATED_FINANCIAL_POLICY')
        names = {n for n in by[a]['accounts'] if n not in ('clock', 'policy')
                 and not n.startswith(('withdrawal_', 'key_'))}
        require(all(by[a]['accounts'][n] == by[b]['accounts'][n] for n in names), 'RECOVERY_MUTATED_CUSTODY_OR_APPROVAL')
    for a, b in (('partial_before', 'continued_period_9'), ('continued_period_9', 'continued_year_two')):
        x, y = read_policy(by[a]), read_policy(by[b])
        require(all(y[r + '_released_total'] > x[r + '_released_total'] for r in ('founder', 'treasury')), 'BOTH_POOLS_MUST_CONTINUE')
    require(results['notice_minus_one']['amount_ceilings_before_transaction_signatures'] == ['0', '0'], 'PENDING_ROLE_PAUSE')
    final = policies[-1]
    require(final['annual_index'] == 1 and final['period'] == 13
            and final['founder_released_total'] == 300_000 * 10**9
            and final['treasury_released_total'] == 450_000 * 10**9
            and final['founder_annual_used'] == 100_000 * 10**9
            and final['treasury_annual_used'] == 150_000 * 10**9, 'REHEARSAL_FINAL_ACCOUNTING')
    return {'valid': True, 'checkpoints_verified': len(snapshots), 'results': results,
            'scope': 'AUTHOR_RUN_SUPPLIED_LOCAL_REHEARSAL', 'production_ready': False,
            'public_chain_transactions': 0, 'transaction_signatures_verified': False,
            'independent_human_audit': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    args = parser.parse_args()
    try:
        result = verify_bundle(json.loads(args.bundle.read_text()))
    except (ValueError, KeyError, TypeError, struct.error) as error:
        print(json.dumps({'valid': False, 'error': str(error)}))
        raise SystemExit(1) from error
    print(json.dumps(result, indent=2))
