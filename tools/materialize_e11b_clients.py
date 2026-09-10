#!/usr/bin/env python3
"""Generate local-only v7 clients/IDL from pinned sources AFTER a successful SBF build."""
from pathlib import Path
import hashlib
import json
import subprocess
from materialize_e11b_v7 import ROOT, NEW, NEW_ID, INPUTS, blob_sha, renamed, once

PINNED = {
    'tools/build_launch_v6_idl.py': 'd3f634b13c96764b4623e53cf7cede3a1807d22b',
    'clients/launch_v6_local_client.mjs': 'bdf75293815453be06dd69a9dac252e34c890755',
    'probes/launch_v6_identity.mjs': '646a8d94b70a9ff9d7e8d513ec71fb80b26b9738',
    'tools/e11_fixtures.mjs': '505df55f76e9e4ac51d584a6aeb329370985620a',
    'src/launch_v6_verifier.py': 'f5bab389cf9a666eb2d14d0222f679aefb9fd306',
    'candidates/launch-vault-v6/tests/abi.rs': '25589358bde017c117c30bf8bbee0427557280f6',
}

def source(path):
    b = (ROOT / path).read_bytes()
    if blob_sha(b) != PINNED[path]:
        raise ValueError('FROZEN_CLIENT_INPUT_CHANGED: ' + path)
    return renamed(b.decode()).replace('launch_v6', 'launch_v7')

def write(path, text):
    p = ROOT / path
    if p.exists():
        raise ValueError('GENERATED_OUTPUT_EXISTS: ' + path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)

if __name__ == '__main__':
    builds = json.loads((ROOT / 'target/e11b/builds.json').read_text())
    code = (ROOT / 'target/v7-test/launch_vault_v7.so').read_bytes()
    assert len(code) == builds['test']['size']
    assert hashlib.sha256(code).hexdigest() == builds['test']['sha256']
    builder = source('tools/build_launch_v6_idl.py').replace('== 18', '== 19').replace('== 6', '== 7')
    write('tools/build_launch_v7_idl.py', builder)
    subprocess.run(['python3', str(ROOT / 'tools/build_launch_v7_idl.py')], cwd=ROOT, check=True)
    client = source('clients/launch_v6_local_client.mjs')
    client = client.replace('36e5b4f8916d7c4844e625f8665dd91b8057c43e88699168d0cb2936cd02c171', builds['test']['sha256'])
    client = client.replace('529584', str(builds['test']['size']))
    write('clients/launch_v7_local_client.mjs', client)
    write('probes/launch_v7_identity.mjs', source('probes/launch_v6_identity.mjs'))
    write('src/launch_v7_verifier.py', source('src/launch_v6_verifier.py'))
    fixtures = source('tools/e11_fixtures.mjs')
    fixtures = fixtures[:fixtures.index('export function openInstruction(f)')]
    fixtures += '''export const preparationAddress = f => pda(Buffer.from('launch-v7-preparation'), f.identity);
export function prepareInstruction(f) {
  return instruction('prepare_policy', { creator: f.creator.publicKey, founder: f.founder.publicKey,
    treasury: f.treasury.publicKey, oracle: f.oracle.publicKey, mint: f.mint.publicKey,
    preparation: preparationAddress(f), system_program: SYSTEM }, [encodeConfig(f.config), f.specHash, f.identity]);
}
export function openInstruction(f) {
  return instruction('open_prepared_policy', { creator: f.creator.publicKey, founder: f.founder.publicKey,
    treasury: f.treasury.publicKey, oracle: f.oracle.publicKey,
    recovery_one: f.recovery[0].publicKey, recovery_two: f.recovery[1].publicKey,
    recovery_three: f.recovery[2].publicKey, mint: f.mint.publicKey,
    preparation: preparationAddress(f), policy: f.policy, system_program: SYSTEM });
}
'''
    write('tools/e11b_fixtures.mjs', fixtures)
    abi = source('candidates/launch-vault-v6/tests/abi.rs')
    abi = abi.replace('identity, period_at,', 'LaunchPreparationV7, identity, period_at,')
    abi = once(abi, '    for (name, discriminator, bytes) in [',
        '    for (name, discriminator, bytes) in [\n        ("LaunchPreparationV7", LaunchPreparationV7::DISCRIMINATOR, LaunchPreparationV7::INIT_SPACE),')
    write('candidates/launch-vault-v7/tests/abi.rs', abi)
    print('Compiler IDL and local clients generated; not an acceptance receipt.')
