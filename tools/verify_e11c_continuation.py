#!/usr/bin/env python3
"""Compare actual validator raw snapshots, retaining explicit preload boundaries."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'src'))
from e11b_verifier import verify_graph
from launch_v7_verifier import read_policy, read_token, read_vault
UNIT = 1_000_000_000

def check(ok, reason):
    if not ok:
        raise ValueError(reason)

def verify(directory):
    directory = Path(directory)
    fixture = json.loads((directory / 'fixture.json').read_text())
    observations = {}
    for label in ('preloaded', 'authority-action', 'continued', 'after-refusals'):
        record = json.loads((directory / ('observation-' + label + '.json')).read_text())
        check(record['verification']['valid'], 'EXPORTER_REJECTED')
        snapshot = record['snapshot']
        check(verify_graph(snapshot)['valid'], 'RAW_GRAPH_REJECTED')
        observations[label] = snapshot
    before, after, continued, refused = [observations[k] for k in ('preloaded','authority-action','continued','after-refusals')]
    p0, p1, p2 = map(read_policy, (before, after, continued))
    counters = ('period','shared_used','founder_period_used','treasury_period_used','founder_released_total',
                'treasury_released_total','annual_index','founder_annual_used','treasury_annual_used')
    check(all(p0[k] == p1[k] for k in counters), 'AUTHORITY_ACTION_CHANGED_MONEY')
    for p in (p1, p2):
        for k in ('address','identity','spec_hash','config','mint','creator','founder','treasury','initial_oracle'):
            check(p[k] == p0[k], 'IMMUTABLE_FIELD_CHANGED_' + k)
    for name in ('founder_vault','treasury_vault','approval_6','approval_9','approval_13'):
        check(before['accounts'][name]['data_hex'] == after['accounts'][name]['data_hex'], 'AUTHORITY_ACTION_CHANGED_' + name)
    if fixture['phase'] == 'recovery':
        for role in (0, 1):
            check(p1['withdrawal'][role]['epoch'] == p0['withdrawal'][role]['epoch'] + 1, 'RECOVERY_EPOCH')
            check(p1['withdrawal'][role]['current'] != p0['withdrawal'][role]['current'], 'RECOVERY_KEY')
            check(p0['withdrawal'][role]['pending'] == 1 and p1['withdrawal'][role]['pending'] == 0, 'RECOVERY_PENDING')
    else:
        check(p0['withdrawal'][0]['pending'] == 3 and p1['withdrawal'][0]['pending'] == 0, 'EXPIRY_PENDING')
        for role in (0, 1):
            check(p1['withdrawal'][role]['current'] == p0['withdrawal'][role]['current'] and
                  p1['withdrawal'][role]['epoch'] == p0['withdrawal'][role]['epoch'], 'EXPIRY_CHANGED_AUTHORITY')
    check(p2['period'] == fixture['period'], 'WRONG_CONTINUATION_PERIOD')
    for role, name, amount in ((0,'founder',100_000*UNIT),(1,'treasury',150_000*UNIT)):
        check(p2[name+'_released_total'] - p1[name+'_released_total'] == amount, 'RELEASE_TOTAL_DELTA')
        check(read_token(after,name+'_token')['amount'] - read_token(continued,name+'_token')['amount'] == amount,'VAULT_DELTA')
        check(read_vault(continued,name+'_vault')['principal'] == p0['config'][name+'_amount'],'PRINCIPAL_CHANGED')
        expected_annual = amount if fixture['phase'] == 'expiry' else p1[name+'_annual_used'] + amount
        check(p2[name+'_annual_used'] == expected_annual,'ANNUAL_USED')
    check(p2['annual_index'] == (1 if fixture['phase'] == 'expiry' else 0),'YEAR_BOUNDARY')
    for snapshot in observations.values():
        token_names = ['source','founder_token','treasury_token','treasury_destination']
        token_names += sorted(n for n in snapshot['accounts'] if n.startswith('founder_destination_'))
        check(sum(read_token(snapshot,n)['amount'] for n in token_names) == 10**18,'SUPPLY_CONSERVATION')
    check(set(continued['accounts']) == set(refused['accounts']),'REJECTION_GRAPH_CHANGED')
    for name in continued['accounts']:
        if name != 'clock':
            check(continued['accounts'][name]['data_hex'] == refused['accounts'][name]['data_hex'],'REJECTION_CHANGED_'+name)
    check(fixture['application_state_preloaded'] is True and fixture['natural_90_180_day_soak'] is False,'SCOPE')
    return {'valid':True,'phase':fixture['phase'],'raw_checkpoints':4,
        'authority_action_preserved_counters_and_budgets':True,'supply_conserved':True,
        'bounded_withdrawals_after_authority_action':True,'year_two_checked':fixture['phase']=='expiry',
        'application_state_preloaded':True,'fixture_clock_controlled':True,
        'natural_90_180_day_soak':False,'independent_human_review':False,'production_ready':False,
        'observation_sha256':{k:hashlib.sha256((directory/('observation-'+k+'.json')).read_bytes()).hexdigest() for k in observations}}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('directory');args=parser.parse_args()
    print(json.dumps(verify(args.directory),indent=2))
