#!/usr/bin/env python3
"""Offline replay of hash-bound E11C evidence, not a new validator execution."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile
import tempfile
from verify_e11c_continuation import verify
ROOT = Path(__file__).resolve().parents[1]

def check(ok, reason):
    if not ok:
        raise ValueError(reason)

def main():
    record = json.loads((ROOT / 'evidence/E11C_ACCEPTED_2026-09-10.json').read_text())
    archive = record['archive']
    path = ROOT / archive['path']
    raw = path.read_bytes()
    check(len(raw) == archive['bytes'], 'ARCHIVE_SIZE')
    check(hashlib.sha256(raw).hexdigest() == archive['sha256'], 'ARCHIVE_SHA256')
    check(hashlib.sha1(b'blob ' + str(len(raw)).encode() + bytes([0]) + raw).hexdigest() == archive['git_blob_sha'], 'GIT_BLOB_SHA')
    for name, expected in record['accepted_runtime_sources_sha256'].items():
        check(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, 'RUNTIME_SOURCE_CHANGED_' + name)
    with tempfile.TemporaryDirectory(prefix='k4v-e11c-replay-') as temporary:
        dest = Path(temporary)
        with tarfile.open(path, 'r:gz') as tar:
            members = tar.getmembers()
            check(len(members) <= 2000 and sum(m.size for m in members) <= 100_000_000, 'ARCHIVE_LIMIT')
            seen = set()
            for member in members:
                rel = PurePosixPath(member.name)
                check(not rel.is_absolute() and '..' not in rel.parts, 'ARCHIVE_PATH')
                check(member.name not in seen, 'DUPLICATE_ARCHIVE_MEMBER')
                seen.add(member.name)
                check(member.isdir() or member.isfile(), 'ARCHIVE_MEMBER_TYPE')
                check(rel.parts[:2] == ('target', 'e11c'), 'ARCHIVE_PREFIX')
                if member.isfile() and member.name.endswith('.json'):
                    target = dest / member.name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    stream = tar.extractfile(member)
                    check(stream is not None, 'ARCHIVE_STREAM')
                    target.write_bytes(stream.read())
        evidence = dest / 'target/e11c'
        acceptance = json.loads((evidence / 'acceptance.json').read_text())
        check(acceptance['tested_commit'] == record['tested_commit'], 'TESTED_COMMIT')
        check(acceptance['valid'] is True and acceptance['natural_90_180_day_soak'] is False, 'SCOPE')
        results = {}
        total = 0
        for phase in ('recovery', 'expiry'):
            data = evidence / phase / 'receipt.json'
            check(hashlib.sha256(data.read_bytes()).hexdigest() == record['phase_receipts_sha256'][phase], 'RECEIPT_SHA')
            receipt = json.loads(data.read_text())
            check(receipt['valid'] is True and receipt['application_state_preloaded'] is True, 'PHASE_SCOPE')
            check(receipt['public_chain_transactions'] == 0 and receipt['natural_90_180_day_soak'] is False, 'PUBLIC_OR_NATURAL_CLAIM')
            check(receipt['program_sha256'] == record['canonical_v7_program_sha256'], 'PROGRAM_SHA')
            txs = json.loads((evidence / phase / 'signed-transactions.json').read_text())
            check(len(txs) == receipt['finalized_client_transactions'], 'TRANSACTION_COUNT')
            check(all(tx['result']['status'] == 'FINALIZED' for tx in txs), 'FINALITY_RECORD')
            total += len(txs)
            results[phase] = verify(evidence / phase)
        check(total == record['finalized_client_transactions'] == 11, 'TOTAL_TRANSACTIONS')
        check(sum(r['raw_checkpoints'] for r in results.values()) == record['raw_checkpoints'] == 8, 'TOTAL_CHECKPOINTS')
    print(json.dumps({'valid': True, 'mode': 'OFFLINE_ARCHIVED_BYTES_REPLAY',
        'tested_commit': record['tested_commit'], 'finalized_transactions_in_archive': total,
        'raw_checkpoints_replayed': 8, 'application_state_preloaded': True,
        'natural_90_180_day_soak': False, 'production_ready': False,
        'independent_human_review': False, 'phases': results}, indent=2))

if __name__ == '__main__':
    main()
