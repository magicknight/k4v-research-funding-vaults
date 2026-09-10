#!/usr/bin/env python3
"""Import only byte-verified, previously accepted v7 source; never execute it here.

Existing files must be identical. Only the one-shot same-repository workflow may
publish; it first verifies the exact successful run and immutable artifact.
No production configuration, old candidate, workflow, key or executable binary
is accepted from the source tar. Public local-test receipts are archived separately.
"""
from pathlib import Path, PurePosixPath
import hashlib
import gzip
import io
import json
import os
import subprocess
import sys
import tarfile

ROOT = Path.cwd().resolve()
ARTIFACT = Path(sys.argv[1]).resolve()
BRANCH = 'codex/e11b-compact-bootstrap'


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def permitted(name):
    p = PurePosixPath(name)
    if p.is_absolute() or '..' in p.parts or any(part.startswith('.') for part in p.parts):
        return False
    if len(p.parts) >= 3 and p.parts[:2] == ('candidates', 'launch-vault-v7'):
        return p.suffix in ('.rs', '.toml', '.lock') and 'target' not in p.parts
    choices = {
        'idl': ('launch_vault_v7.json',),
        'spec': ('LAUNCH_V7_BUILD_IDENTITY_v1.json', 'LAUNCH_V7_IDENTITY_VECTOR_v1.json'),
        'probes': ('launch_v7_identity.mjs', 'launch_v7_identity.test.mjs'),
        'clients': ('launch_v7_bootstrap.mjs', 'launch_v7_bootstrap.test.mjs',
                    'launch_v7_local_client.mjs', 'launch_v7_local_client.test.mjs'),
        'src': ('launch_v7_verifier.py', 'launch_v7_rpc_exporter.py',
                'e11b_verifier.py', 'e11b_preparation_verifier.py'),
        'tools': ('build_launch_v7_idl.py', 'materialize_e11b_candidate.py',
                  'materialize_e11b_integration.py', 'e11b_pin_build.py',
                  'e11b_bootstrap_tests.rs', 'e11b_bootstrap_wire.mjs',
                  'e11b_bootstrap_wire.test.mjs', 'e11b_verify_tests.py',
                  'e11b_recorded_rpc.py', 'e11b_agave_rehearsal.mjs', 'e11b_fixtures.mjs',
                  'pack_e11b_rehearsal.py', 'verify_e11b_probes.py'),
    }
    return len(p.parts) == 2 and p.name in choices.get(p.parts[0], ())


require(os.environ['GITHUB_REPOSITORY'] == 'magicknight/k4v-research-funding-vaults', 'WRONG_REPO')
require(os.environ['GITHUB_REF'] == 'refs/heads/' + BRANCH, 'WRONG_BRANCH')
require(git('rev-parse', 'HEAD') == os.environ['GITHUB_SHA'], 'CHECKOUT_SHA')
require(not git('status', '--porcelain', '--untracked-files=no'), 'DIRTY_TRACKED_FILES')
metadata = json.loads((ARTIFACT / 'verified-run.json').read_text())
require(metadata['conclusion'] == 'success' and metadata['run_id'] == 34450900533
        and metadata['head_sha'] == 'bd7e34333ef422d73facdaf24f5363f63c575194', 'UNACCEPTED_SOURCE')
checksums = json.loads((ARTIFACT / 'source-sha256.json').read_text())
require(isinstance(checksums, dict) and 25 <= len(checksums) <= 60, 'MANIFEST_COUNT')
new_files = []
with tarfile.open(ARTIFACT / 'generated-source.tar.gz', 'r:gz') as archive:
    members = archive.getmembers()
    require(len(members) == len(checksums) and len({m.name for m in members}) == len(members), 'ARCHIVE_SET')
    require({m.name for m in members} == set(checksums), 'ARCHIVE_MANIFEST_MISMATCH')
    require(sum(m.size for m in members) <= 2_000_000, 'SOURCE_SIZE')
    for member in members:
        require(member.isfile() and permitted(member.name), 'FORBIDDEN_SOURCE_' + member.name)
        data = archive.extractfile(member).read()
        require(hashlib.sha256(data).hexdigest() == checksums[member.name], 'SOURCE_HASH_' + member.name)
        data.decode('utf-8')
        path = ROOT / member.name
        require(not path.is_symlink() and ROOT in path.resolve().parents, 'UNSAFE_DESTINATION')
        if path.exists():
            require(path.read_bytes() == data, 'REFUSE_EXISTING_FILE_CHANGE_' + member.name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            new_files.append(member.name)
require(len(new_files) >= 25, 'NO_GENERATED_SOURCE')
subprocess.run(['git', 'add', '--', *new_files], cwd=ROOT, check=True)
require(set(git('diff', '--cached', '--name-only').splitlines()) == set(new_files), 'STAGED_SCOPE')
require(all(line.startswith('A\t') for line in git('diff', '--cached', '--name-status').splitlines()), 'NON_ADDITIVE_CHANGE')
selected = {}
required = {'target/e11b/fresh-bundle.json', 'target/e11b/financial-verification.json',
            'target/e11b/loopback-replay.json', 'target/e11b/build-identity.json',
            'target/e11b/agave/receipt.json', 'target/e11b/agave/signed-transactions.json',
            'target/e11b/agave/preparation-input.json', 'target/e11b/agave/preparation-verification.json',
            'target/e11b/agave/manifest.json', 'target/e11b/rust-tests.log',
            'target/e11b/js-tests.log', 'target/e11b/python-tests.log'}
with tarfile.open(ARTIFACT / 'evidence.tar.gz', 'r:gz') as archive:
    for member in archive.getmembers():
        name = member.name
        allowed = name in required or (name.startswith('target/e11b/agave/observation-') and name.endswith('.json'))
        allowed = allowed or (name.startswith('target/e11b/probes/') and name.endswith('.json'))
        if allowed:
            require(member.isfile() and member.size <= 10_000_000, 'EVIDENCE_MEMBER')
            require(name not in selected, 'DUPLICATE_EVIDENCE')
            selected[name] = archive.extractfile(member).read()
require(required <= set(selected), 'MISSING_ACCEPTANCE_EVIDENCE')
node = json.loads(selected['target/e11b/agave/receipt.json'])
require(node['valid'] is True and node['public_chain_transactions'] == 0
        and node['clock_override'] is False and node['client_private_keys_serialized'] is False
        and node['bootstrap'] == 'six-distinct-role-keys-plus-separate-fee-payer'
        and node['full_notice_seconds'] == '7776000'
        and len(node['raw_rpc_checkpoints']) == 4 and node['production_ready'] is False,
        'NODE_SCOPE_OR_ACCEPTANCE')
require(json.loads(selected['target/e11b/financial-verification.json'])['checkpoints_verified'] == 11,
        'FINANCIAL_SCOPE')
require(node['program_sha256'] == json.loads((ROOT / 'spec/LAUNCH_V7_BUILD_IDENTITY_v1.json').read_text())['profiles']['test']['sha256'],
        'EVIDENCE_PROGRAM_MISMATCH')
evidence_path = ROOT / 'evidence/e11b/accepted-local-evidence.tar.gz'
evidence_path.parent.mkdir(parents=True, exist_ok=True)
require(not evidence_path.exists(), 'EVIDENCE_ALREADY_EXISTS')
with evidence_path.open('wb') as output:
    with gzip.GzipFile(filename='', mode='wb', fileobj=output, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            for name, data in sorted(selected.items()):
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), 0o644, 0
                archive.addfile(info, io.BytesIO(data))
subprocess.run(['git', 'add', '--', str(evidence_path.relative_to(ROOT))], cwd=ROOT, check=True)
record = ROOT / 'evidence/e11b/source-publication.json'
require(not record.exists(), 'RECEIPT_ALREADY_EXISTS')
record.write_text(json.dumps({'schema': 'K4V-E11B-SOURCE-PUBLICATION-v1', 'upstream': metadata,
    'checked_out_sha': (ARTIFACT / 'checked-out-commit.txt').read_text().strip(),
    'source_sha256': checksums,
    'evidence_sha256': {n: hashlib.sha256(d).hexdigest() for n, d in selected.items()},
    'evidence_archive_sha256': hashlib.sha256(evidence_path.read_bytes()).hexdigest(),
    'new_files': new_files, 'publication_base': os.environ['GITHUB_SHA'],
    'independent_human_review': False, 'public_chain_transactions': 0,
    'candidate_requires_committed_source_replay': True}, indent=2, sort_keys=True) + '\n')
subprocess.run(['git', 'add', '--', str(record.relative_to(ROOT))], cwd=ROOT, check=True)
require(git('ls-remote', 'origin', 'refs/heads/' + BRANCH).split()[0] == os.environ['GITHUB_SHA'], 'REMOTE_ADVANCED')
subprocess.run(['git', '-c', 'user.name=github-actions[bot]', '-c',
                'user.email=41898282+github-actions[bot]@users.noreply.github.com', 'commit', '-m',
                'feat(e11b): publish byte-bound accepted v7 candidate source and build identities'], cwd=ROOT, check=True)
subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/' + BRANCH], cwd=ROOT, check=True)
print(json.dumps({'published_sha': git('rev-parse', 'HEAD'), 'added_source_files': len(new_files),
                  'merged': False, 'production_ready': False}))
