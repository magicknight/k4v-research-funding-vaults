#!/usr/bin/env python3
"""Create mature, explicitly preloaded test fixtures without changing frozen SBF.

Only a temporary test copy's origin time and public insecure fixture keys change.
Its complete signed native-loader history uses controlled Clock, not natural soak.
"""
from __future__ import annotations
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'src'))
from launch_v7_verifier import read_policy, read_vault, read_token
from e11b_verifier import expand_bundle
PERIOD = 2_592_000
FIXTURE_BLOB = '6f2eecb342a3a399d4e8f6aa68ca3e7d0681fc7b'
CASE = 'e11b::signed_native_loader_dual_withdrawal_recovery_preserves_money_notices_and_year_boundary'

def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def replace_once(text: str, old: str, new: str) -> str:
    if text.count(old) != 1:
        raise ValueError('FIXTURE_PATCH_ANCHOR: ' + old)
    return text.replace(old, new, 1)

def save(path: Path, data: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + '\n')

def extract_program() -> None:
    subprocess.run(['python3', 'tools/verify_e11b_archive.py'], cwd=ROOT, check=True)
    identity = json.loads((ROOT / 'spec/LAUNCH_V7_BUILD_IDENTITY_v1.json').read_text())
    with tarfile.open(ROOT / 'evidence/e11b/accepted-local-evidence.tar.gz', 'r:gz') as archive:
        member = archive.getmember('target/e11b/fresh-bundle.json')
        if not member.isfile() or member.size > 100_000_000:
            raise ValueError('BAD_ARCHIVE_MEMBER')
        raw = json.load(archive.extractfile(member))
    data = bytes.fromhex(expand_bundle(raw)[0]['accounts']['program_data']['data_hex'])
    spec = identity['profiles']['test']
    code = data[45:45 + spec['bytes']]
    if sha(code) != spec['sha256'] or code[:4] != b'\x7fELF':
        raise ValueError('FROZEN_SBF_MISMATCH')
    path = ROOT / 'target/v7-test/launch_vault_v7.so'
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and path.read_bytes() != code:
        raise ValueError('REFUSE_TO_REPLACE_DIFFERENT_SBF')
    path.write_bytes(code)

def construct(phase: str) -> None:
    out = ROOT / 'target/e11c' / phase
    out.mkdir(parents=True, exist_ok=True)
    original = ROOT / 'candidates/launch-vault-v7'
    raw = (original / 'tests/launch_litesvm.rs').read_bytes()
    blob = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
    if blob != FIXTURE_BLOB:
        raise ValueError('FROZEN_TEST_FIXTURE_CHANGED')
    offset_periods = 11 if phase == 'recovery' else 15
    anchor = int(time.time()) - offset_periods * PERIOD - 60 - 300
    source = replace_once(raw.decode(), 'const START: i64 = 1_700_000_000;', f'const START: i64 = {anchor};')
    source = replace_once(source, 'let oracle = Keypair::new();',
        'let oracle = Keypair::new_from_array(solana_sha256_hasher::hash(b"e11c-oracle-public-test-key").to_bytes());')
    for role in ('founder', 'treasury'):
        source = replace_once(source, f'let {role} = same_or_new();',
            f'let {role} = if solo {{ same_or_new() }} else {{ Keypair::new_from_array(solana_sha256_hasher::hash(b"e11c-old-{role}-public-test-key").to_bytes()) }};')
    with tempfile.TemporaryDirectory(prefix='k4v-e11c-harness-') as directory:
        harness = Path(directory)
        copied = harness / 'candidates/launch-vault-v7'
        shutil.copytree(original, copied, ignore=shutil.ignore_patterns('target'))
        shutil.copytree(ROOT / 'spec', harness / 'spec')
        shutil.copytree(ROOT / 'idl', harness / 'idl')
        (harness / 'target').mkdir()
        (harness / 'target/v7-test').symlink_to(ROOT / 'target/v7-test', target_is_directory=True)
        (copied / 'tests/launch_litesvm.rs').write_text(source)
        for path in (original / 'src').rglob('*.rs'):
            if path.read_bytes() != (copied / path.relative_to(original)).read_bytes():
                raise ValueError('PROGRAM_SOURCE_CHANGED')
        env = dict(os.environ, CARGO_TARGET_DIR='/tmp/k4v-e11c-native',
                   K4V_E11B_REHEARSAL_OUT=str(out / 'native-history.json'))
        with (out / 'fixture-test.log').open('w') as log:
            result = subprocess.run(['cargo', 'test', '--manifest-path', str(copied / 'Cargo.toml'),
                '--locked', '--features', 'test-profile', '--test', 'launch_litesvm', CASE,
                '--', '--exact', '--nocapture'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            print((out / 'fixture-test.log').read_text()[-12000:], file=sys.stderr)
            raise RuntimeError('NATIVE_FIXTURE_FAILED')
    history = json.loads((out / 'native-history.json').read_text())
    if history['program_or_token_injection'] or history['private_keys_serialized'] or not history['clock_controlled']:
        raise ValueError('FIXTURE_PROVENANCE')
    subprocess.run(['python3', 'tools/pack_e11b_rehearsal.py', str(out / 'native-history.json'),
                    str(out / 'native-bundle.json')], cwd=ROOT, check=True)
    with (out / 'native-verification.json').open('w') as log:
        subprocess.run(['python3', 'src/e11b_verifier.py', str(out / 'native-bundle.json')],
                       cwd=ROOT, env=dict(os.environ, PYTHONPATH=str(ROOT / 'src')), stdout=log, check=True)
    label = 'before_execute' if phase == 'recovery' else 'normal_pending_expiry'
    selected = [s for s in history['snapshots'] if s['label'] == label]
    if len(selected) != 1:
        raise ValueError('EXACT_FIXTURE_CHECKPOINT_REQUIRED')
    snapshot = selected[0]
    save(out / 'preload-snapshot.json', snapshot)
    p = read_policy(snapshot)
    accounts = []
    for name, a in snapshot['accounts'].items():
        if name in ('program', 'program_data', 'clock'):
            continue
        data = bytes.fromhex(a['data_hex'])
        target = out / 'accounts' / (name + '.json')
        save(target, {'pubkey': a['address'], 'account': {'lamports': int(a['lamports']),
            'data': [base64.b64encode(data).decode(), 'base64'], 'owner': a['owner'],
            'executable': a['executable'], 'rentEpoch': 0, 'space': len(data)}})
        accounts.append({'name': name, 'address': a['address'], 'file': str(target.relative_to(ROOT)),
                         'data_sha256': sha(data), 'owner': a['owner'], 'executable': a['executable']})
    external = {n: a['address'] for n, a in snapshot['accounts'].items()
                if n == 'source' or n.startswith('founder_destination_') or n == 'treasury_destination'}
    expected = {'program_id': snapshot['program_id'], 'policy': p['address'],
                'identity_sha256': p['identity'].hex(), 'spec_sha256': p['spec_hash'].hex(),
                **{k: p[k] for k in ('mint', 'creator', 'founder', 'treasury', 'initial_oracle')}}
    save(out / 'fixture.json', {'schema': 'K4V-E11C-PRELOAD-v1', 'phase': phase,
        'scope': 'SIGNED_NATIVE_HISTORY_THEN_EXPLICIT_APPLICATION_PRELOAD', 'anchor': anchor,
        'native_fixture_blob': FIXTURE_BLOB, 'modified_test_sha256': sha(source.encode()),
        'frozen_program_source_modified': False, 'fixture_clock_controlled': True,
        'application_state_preloaded': True, 'natural_90_180_day_soak': False,
        'private_keys_serialized': False, 'expected': expected, 'accounts': accounts,
        'external_accounts': external, 'approval_periods': [6, 9, 13],
        't0': str(p['config']['t0']), 'period': 9 if phase == 'recovery' else 13,
        'report_sequence': str(p['report_sequence']), 'oracle_epoch': str(p['oracle_epoch']),
        'capacity': str(p['config']['shared_hard_cap']),
        'recipient_owner': read_token(snapshot, 'treasury_destination')['owner'],
        'depositor': read_vault(snapshot, 'founder_vault')['depositor'],
        'native_signed_transactions': history['signed_transactions_sent'],
        'native_successful_transactions': history['signed_transactions_successful']})
    print('E11C_FIXTURE_READY ' + phase + ' (explicit preloaded state, not natural soak)')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--extract-program', action='store_true')
    parser.add_argument('--phase', choices=('recovery', 'expiry'))
    args = parser.parse_args()
    if args.extract_program:
        extract_program()
    elif args.phase:
        construct(args.phase)
    else:
        parser.error('select --extract-program or --phase')
