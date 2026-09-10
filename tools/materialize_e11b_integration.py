#!/usr/bin/env python3
"""One-time local integration derivation; preserve E10/E11A source and receipts."""
from pathlib import Path
import json
R = Path(__file__).resolve().parents[1]
D = R / 'candidates/launch-vault-v7'
OLD_ID = 'FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU'
NEW_ID = 'CYFsfATtQB3Excjsm4Cuh8ZWnPE5j6XAU3GS3RKXmUcK'
def norm(s):
    for a, b in [(OLD_ID, NEW_ID), ('launch_vault_v6', 'launch_vault_v7'), ('launch_v6', 'launch_v7'), ('launch-v6', 'launch-v7'), ('V6', 'V7'), ('v6', 'v7')]:
        s = s.replace(a, b)
    return s
p = D / 'tests/support/bootstrap.rs'
p.write_text((R / 'tools/e11b_bootstrap_tests.rs').read_text())
p = D / 'tests/launch_litesvm.rs'
s = p.read_text()
if 'mod bootstrap;' not in s:
    p.write_text(s + '\n#[path = "support/bootstrap.rs"]\nmod bootstrap;\n')
p = R / 'spec/LAUNCH_V7_IDENTITY_VECTOR_v1.json'
v = json.loads(p.read_text()); v['schema'] = 'K4V-LAUNCH-V7-IDENTITY-VECTOR-v1'
p.write_text(json.dumps(v, indent=2) + '\n')
p = R / 'clients/launch_v7_local_client.mjs'
s = norm((R / 'clients/launch_v6_local_client.mjs').read_text())
s = s.replace("import { createHash } from 'node:crypto';", "import { createHash } from 'node:crypto';\nimport { readFileSync } from 'node:fs';")
s = s.replace("export const CODE_SHA256 = '36e5b4f8916d7c4844e625f8665dd91b8057c43e88699168d0cb2936cd02c171';", "const BUILD = JSON.parse(readFileSync(new URL('../spec/LAUNCH_V7_BUILD_IDENTITY_v1.json', import.meta.url)));\nif (BUILD.program !== PROGRAM.toBase58()) throw new Error('BUILD_PROGRAM');\nexport const CODE_SHA256 = BUILD.profiles.test.sha256;\nexport const CODE_BYTES = BUILD.profiles.test.bytes;")
p.write_text(s.replace('529584', 'CODE_BYTES'))
p = R / 'clients/launch_v7_local_client.test.mjs'
s = norm((R / 'clients/launch_v6_local_client.test.mjs').read_text())
s = s.replace("import { wireSizes } from '../tools/e11_fixtures.mjs';\n", '')
a = s.index("test('independent v7 bootstrap exceeds")
b = s.index("test('endpoint parsing", a)
p.write_text(s[:a] + s[b:])
p = R / 'tools/e11b_fixtures.mjs'
s = norm((R / 'tools/e11_fixtures.mjs').read_text())
p.write_text(s[:s.index('export function openInstruction')])
p = R / 'src/e11b_verifier.py'
s = p.read_text()
s = s.replace("CODE_BYTES = 529584\nCODE_SHA256 = '36e5b4f8916d7c4844e625f8665dd91b8057c43e88699168d0cb2936cd02c171'", "BUILD = json.loads((Path(__file__).resolve().parents[1] / 'spec/LAUNCH_V7_BUILD_IDENTITY_v1.json').read_text())\nrequire(BUILD['program'] == PROGRAM, 'BUILD_PROGRAM')\nCODE_BYTES = BUILD['profiles']['test']['bytes']\nCODE_SHA256 = BUILD['profiles']['test']['sha256']")
s = s.replace('from e05_verifier import expand_bundle as expand_checked_blobs', 'from e05_verifier import expand_bundle as expand_checked_blobs\nfrom e11b_preparation_verifier import verify_preparation')
s = s.replace('    result = verify(s)\n', '    result = verify(s)\n    result["preparation"] = verify_preparation(s, policy=read_policy(s))\n', 1)
p.write_text(s)
p = R / 'src/launch_v7_rpc_exporter.py'
s = p.read_text().replace('    addresses.update(external)', '    addresses["preparation"] = pda(b"launch-v7-preparation", bytes.fromhex(expected["identity_sha256"]))\n    addresses.update(external)', 1)
p.write_text(s)
# Preserve signed-envelope checks, natural Clock, loader sealing and independent raw decoding.
s = norm((R / 'tools/e11_agave_rehearsal.mjs').read_text())
s = s.replace('LOADER, CODE_SHA256, SYSTEM,', 'LOADER, CODE_SHA256, CODE_BYTES, CLOCK, SYSTEM,')
s = s.replace('Keypair, PublicKey, SystemProgram, TransactionInstruction', 'Keypair, PublicKey, SystemProgram, TransactionInstruction, ComputeBudgetProgram')
s = s.replace('sleep, readClock, readBoundPolicy,', 'sleep, readClock, readClockBoundAccounts, readBoundPolicy,')
s = s.replace("import { fixture, instruction, openInstruction, wireSizes, UNIT, SUPPLY } from './e11_fixtures.mjs';", "import { fixture, instruction, UNIT, SUPPLY } from './e11b_fixtures.mjs';\nimport { bootstrap, wireReceipt } from '../clients/launch_v7_bootstrap.mjs';")
s = s.replace("resolve('target/e11')", "resolve('target/e11b/agave')").replace('k4v-e11-agave-', 'k4v-e11b-agave-').replace('529584', 'CODE_BYTES')
a = s.index('  const wire = wireSizes();')
b = s.index('  await airdrop(f.creator.publicKey', a)
s = s[:a] + '''  const wire = { receipt: wireReceipt() };
  save('wire-budget', wire.receipt);
  const f = fixture({ solo: false });
  const feePayer = Keypair.generate();
  assert.equal(new Set([f.creator, f.founder, f.treasury, ...f.recovery].map(k => k.publicKey.toBase58())).size, 6);
  await airdrop(feePayer.publicKey, 1000000000);
''' + s[b:]
s = s.replace('  // Agave 3.1.10 genesis always', '''  const roleKeys = [f.founder, f.treasury, f.oracle, ...f.recovery, ...f.backups.flat()];
  await send('fund-distinct-role-fixture', roleKeys.map(key => SystemProgram.transfer({
    fromPubkey: f.creator.publicKey, toPubkey: key.publicKey, lamports: 1000000 })), f.creator, [f.creator]);
  // Agave 3.1.10 genesis always''')
s = s.replace('f.config.t0 = clock.now + 150n;', 'f.config.t0 = clock.now + 180n;')
s = s.replace("  await send('solo-open-policy', [openInstruction(f)], f.creator, [f.creator, ...f.recovery]);", '''  const setup = bootstrap(f);
  await send('prepare-immutable-config', [setup.prepare], f.creator, [f.creator]);
  const { response: prepResponse, clock: prepClock } = await readClockBoundAccounts(rpc,
    [setup.preparation.toBase58(), f.mint.publicKey.toBase58(), CLOCK.toBase58()]);
  const rawAccounts = {};
  for (const [i, name, address] of [[0, 'preparation', setup.preparation], [1, 'mint', f.mint.publicKey]]) {
    const a = prepResponse.value[i];
    rawAccounts[name] = { address: address.toBase58(), owner: a.owner, executable: a.executable,
      lamports: String(a.lamports), data_hex: Buffer.from(a.data[0], 'base64').toString('hex') };
  }
  save('preparation-input', { snapshot: { accounts: rawAccounts, slot: prepResponse.context.slot,
    now: prepClock.readBigInt64LE(32).toString() }, expected: binding });
  const prepCheck = spawnSync('python3', ['src/e11b_preparation_verifier.py', join(out, 'preparation-input.json')],
    { encoding: 'utf8', env: { ...process.env, PYTHONPATH: 'src' } });
  assert.equal(prepCheck.status, 0, prepCheck.stdout + prepCheck.stderr);
  const prepVerified = JSON.parse(prepCheck.stdout); assert.equal(prepVerified.valid, true);
  save('preparation-verification', prepVerified);
  await send('six-role-open-separate-payer', [setup.open], feePayer, [feePayer, ...setup.actors]);
  const replay = await signInstructions(rpc, [ComputeBudgetProgram.setComputeUnitLimit({ units: 400000 }), setup.open],
    feePayer.publicKey, [feePayer, ...setup.actors]);
  const replayResult = await submitSigned(rpc, replay, replay.messageHash);
  assert.equal(replayResult.status, 'SIMULATION_REJECTED');
  refusal.push({ label: 'bootstrap-replay', result: replayResult });''')
s = s.replace('depositor: f.creator.publicKey, authority: f.creator.publicKey, policy:', 'depositor: f.creator.publicKey, authority: (role === 0 ? f.founder : f.treasury).publicKey, policy:')
s = s.replace('f.config.treasury_amount)])], f.creator, [f.creator]);', 'f.config.treasury_amount)])], f.creator, [f.creator, role === 0 ? f.founder : f.treasury]);')
s = s.replace("instruction('release', { authority: f.creator.publicKey, policy:", "instruction('release', { authority: f.founder.publicKey, policy:")
s = s.replace('signInstructions(rpc, [release], f.creator.publicKey, [f.creator]);', 'signInstructions(rpc, [release], f.creator.publicKey, [f.creator, f.founder]);')
s = s.replace("'source', 'mint', 'founder_vault'", "'source', 'mint', 'preparation', 'founder_vault'")
s = s.replace("schema: 'K4V-E11A-AGAVE-RECEIPT-v1'", "schema: 'K4V-E11B-AGAVE-RECEIPT-v1'")
s = s.replace("bootstrap: 'creator-founder-treasury-same-key-four-signatures', independent_bootstrap: 'BLOCKED_PACKET_SIZE',", "bootstrap: 'six-distinct-role-keys-plus-separate-fee-payer', independent_human_roles: false, preparation_verified: prepVerified,")
s = s.replace("console.log('E11_RESULT '", "console.log('E11B_RESULT '")
(R / 'tools/e11b_agave_rehearsal.mjs').write_text(s)
print('E11B integration source generated; no tests or public transactions claimed.')
