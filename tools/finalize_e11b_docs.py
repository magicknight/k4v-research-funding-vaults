#!/usr/bin/env python3
"""One-shot additive documentation/checksum finalization on the E11B feature branch."""
from pathlib import Path
import hashlib
import json
import os
import subprocess

R = Path.cwd()
branch = 'codex/e11b-compact-bootstrap'
def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()
def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()
assert os.environ['GITHUB_REPOSITORY'] == 'magicknight/k4v-research-funding-vaults'
assert os.environ['GITHUB_REF'] == 'refs/heads/' + branch
assert git('rev-parse', 'HEAD') == os.environ['GITHUB_SHA']
assert not git('status', '--porcelain', '--untracked-files=no')
subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], check=True)
record = json.loads(Path('evidence/e11b/source-publication.json').read_text())
assert all(digest(Path(p)) == h for p, h in record['source_sha256'].items())
assert record['upstream']['head_sha'] == 'bd7e34333ef422d73facdaf24f5363f63c575194'
old_manifest = Path('SHA256SUMS').read_bytes()
parent = Path('evidence/e11b/parent-SHA256SUMS')
assert not parent.exists()
parent.write_bytes(old_manifest)
expected = {
    'README.md': 'c8cef859f459ec0b36c5ec6450500305501bb64b1475135cc38d8fee35e2d50f',
    'docs/ROADMAP.md': 'b1efcf76d7b320a02d8e5e4811ce8ba7e02a4bea961f1ec1b16a902bc648f2b3',
    'docs/E11B_BOOTSTRAP_DESIGN.md': '4b68a92cce3d02d10e27a1a7d4b73fab859713ecddee9cc6e61bceeae34dc25e',
}
assert all(digest(Path(p)) == h for p, h in expected.items())
p = Path('README.md'); s = p.read_text()
a, b = s.index('**Latest engineering candidate:**'), s.index('**Frozen timing design:**')
s = s[:a] + '''**Latest engineering candidate:** [E-11B v7 compact bootstrap](docs/E11B_ENGINEERING_ACCEPTANCE.md)
implements immutable preparation plus six-role consent in an isolated,
default-disabled program. Initial full acceptance passed 72 Rust, 21 JavaScript
and six additional Python test methods, 11 native-loader financial checkpoints,
and actual Agave six-role initialization with 15 finalized client transactions.
The complete accepted source, exact build hashes, raw receipts and
[review handoff](docs/E11B_REVIEW_HANDOFF.md) are public. Run
`bash tools/run_e11b_acceptance.sh` to rebuild/replay the committed candidate.
A successful earlier run does not certify an untested descendant; consult the
exact commit's E11B workflow. Natural long-duration execution, named human
review, production parameters/rights and public deployment remain open.

''' + s[b:]
p.write_text(s)
p = Path('docs/ROADMAP.md'); s = p.read_text(); a = s.index('## Frontier state\n\n') + len('## Frontier state\n\n'); b = s.index('E-07 now has an', a)
s = s[:a] + '''[E-11B](E11B_ENGINEERING_ACCEPTANCE.md) now implements the two-step bootstrap in
an isolated v7 namespace. Both default-disabled and TEST_ONLY SBFs compile;
missing/substituted signatures, forged preparation, T0, replay and prefunding
are exercised. Compiled wire layouts fit the packet limit. Actual loopback
Agave uses six distinct role keys plus a separate fee payer, executes signed
prepare/open/deposit/arm/activate, and independently decodes the resulting
accounts. This is one operator's key separation, not independent people.

Initial full acceptance: 72 Rust / 21 JavaScript / 6 additional Python test
methods; 142 timing probes; 669 native-loader financial transactions; 11 raw
financial checkpoints and 55 loopback reads; 15 finalized actual-node client
transactions and 4 raw policy checkpoints. Exact source/receipt identities and
reproduction commands are in the linked acceptance and review-handoff files.
Published-source CI must pass for the exact reviewed commit.

[E-11A](E11_LOCAL_VALIDATOR_CLIENT.md) and the original
[E-11B design](E11B_BOOTSTRAP_DESIGN.md) remain historical evidence. The old v6
six-role initialization is still oversized; it was not silently patched or
upgraded. E-10/v6 and all earlier frozen program/evidence bytes are preserved.

Next engineering/runtime boundary: successful recovery/continued withdrawals
on an actual validator after the full long notice/cliff, with the runtime and
Clock assumptions explicitly stated. Controlled-Clock native-loader financial
continuity already passes and is a different claim. Human review, final annual
data/rights/actors and public deployment remain open. Feedback is still paused;
there is no new demand evidence or official mint.

''' + s[b:]; p.write_text(s)
p = Path('docs/E11B_BOOTSTRAP_DESIGN.md'); s = p.read_text(); a = s.index('Status:'); b = s.index('\n\nThe accepted v6', a)
s = s[:a] + '''Historical status at E-11A: design and offline signed-wire prototype only.
The implemented descendant is now [E-11B v7](E11B_ENGINEERING_ACCEPTANCE.md).
The proposal below is preserved as design provenance; it is not the live
implementation status. The v6 ABI, program bytes and E-10 evidence are unchanged.''' + s[b:]; p.write_text(s)
lines = []
for line in old_manifest.decode().splitlines():
    old_hash, name = line.split('  ', 1)
    lines.append((digest(Path(name)) if name in expected else old_hash) + '  ' + name)
Path('SHA256SUMS').write_text('\n'.join(lines) + '\n')
# Remove the one-time writer before merge. Permanent acceptance is read-only.
Path('.github/workflows/e11b-publish.yml').unlink()
extra = {'docs/E11B_ENGINEERING_ACCEPTANCE.md', 'docs/E11B_REVIEW_HANDOFF.md',
    'tools/run_e11b_acceptance.sh', 'tools/verify_e11b_archive.py', 'tools/verify_e11b_archived_signatures.mjs',
    'tools/finalize_e11b_docs.py', '.github/workflows/e11b-engineering.yml',
    'evidence/e11b/source-publication.json', 'evidence/e11b/accepted-local-evidence.tar.gz',
    'evidence/e11b/parent-SHA256SUMS'}
paths = set(record['source_sha256']) | extra
Path('E11B_SHA256SUMS').write_text(''.join(digest(Path(p)) + '  ' + p + '\n' for p in sorted(paths)))
subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], check=True)
subprocess.run(['sha256sum', '-c', 'E11B_SHA256SUMS'], check=True)
allowed = set(expected) | {'SHA256SUMS', 'E11B_SHA256SUMS', 'evidence/e11b/parent-SHA256SUMS', '.github/workflows/e11b-publish.yml'}
subprocess.run(['git', 'add', '-A', '--', *sorted(allowed)], check=True)
assert set(git('diff', '--cached', '--name-only').splitlines()) == allowed
assert all(digest(Path(p)) == h for p, h in record['source_sha256'].items())
assert git('ls-remote', 'origin', 'refs/heads/' + branch).split()[0] == os.environ['GITHUB_SHA']
subprocess.run(['git', '-c', 'user.name=github-actions[bot]', '-c', 'user.email=41898282+github-actions[bot]@users.noreply.github.com',
    'commit', '-m', 'docs(e11b): advance public frontier and freeze source checksums; remove one-shot writer'], check=True)
subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/' + branch], check=True)
print(json.dumps({'published_sha': git('rev-parse', 'HEAD'), 'program_bytes_changed': False, 'merged': False}))
