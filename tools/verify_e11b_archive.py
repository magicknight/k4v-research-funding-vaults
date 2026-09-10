#!/usr/bin/env python3
"""Recheck the published author evidence from a clean checkout; no archive execution."""
from pathlib import Path
import hashlib
import json
import sys
import tarfile
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'src'))
from e11b_verifier import verify_bundle, verify_graph
from e11b_preparation_verifier import verify_preparation

record = json.loads((ROOT / 'evidence/e11b/source-publication.json').read_text())
for name, expected in record['source_sha256'].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, 'PUBLISHED_SOURCE_DRIFT:' + name
archive_path = ROOT / 'evidence/e11b/accepted-local-evidence.tar.gz'
assert hashlib.sha256(archive_path.read_bytes()).hexdigest() == record['evidence_archive_sha256'], 'ARCHIVE_HASH'
with tarfile.open(archive_path, 'r:gz') as archive:
    members = archive.getmembers()
    assert len(members) == len(record['evidence_sha256']) and all(m.isfile() for m in members), 'ARCHIVE_MEMBERS'
    assert {m.name for m in members} == set(record['evidence_sha256']), 'ARCHIVE_SET'
    data = {m.name: archive.extractfile(m).read() for m in members}
for name, raw in data.items():
    assert hashlib.sha256(raw).hexdigest() == record['evidence_sha256'][name], 'EVIDENCE_HASH:' + name
read = lambda name: json.loads(data['target/e11b/' + name])
financial = verify_bundle(read('fresh-bundle.json'))
assert financial['checkpoints_verified'] == 11 and financial['valid'], 'FINANCIAL_REPLAY'
prep = read('agave/preparation-input.json')
assert verify_preparation(prep['snapshot'], expected=prep['expected'])['valid'], 'PREPARATION_REPLAY'
receipt = read('agave/receipt.json')
assert receipt['valid'] and receipt['finalized_client_transactions'] == 15, 'NODE_RECEIPT'
for row in receipt['raw_rpc_checkpoints']:
    name = 'agave/observation-' + row['label'] + '.json'
    assert hashlib.sha256(data['target/e11b/' + name]).hexdigest() == row['sha256'], 'CHECKPOINT_HASH'
    assert verify_graph(read(name)['snapshot'])['valid'], 'NODE_ACCOUNT_REPLAY'
assert len(receipt['raw_rpc_checkpoints']) == 4, 'CHECKPOINT_COUNT'
assert receipt['public_chain_transactions'] == 0 and receipt['clock_override'] is False
assert receipt['long_duration_recovery_execution_verified'] is False and receipt['independent_human_roles'] is False
print(json.dumps({'valid': True, 'scope': 'RECHECK_OF_ARCHIVED_AUTHOR_LOCAL_EVIDENCE',
    'financial_checkpoints': 11, 'agave_account_checkpoints': 4, 'preparation': True,
    'independent_human_review': False, 'live_rpc_observed_by_this_command': False}))
