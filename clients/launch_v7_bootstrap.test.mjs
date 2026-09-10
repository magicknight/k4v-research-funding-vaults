import test from 'node:test';
import assert from 'node:assert/strict';
import { Keypair } from '@solana/web3.js';
import { fixture } from '../tools/e11b_fixtures.mjs';
import { bootstrap, signedBootstrap, wireReceipt } from './launch_v7_bootstrap.mjs';

test('compiled ABI fits preparation and six-role consent including separate payer and compute budget', () => {
  const r = wireReceipt();
  assert.equal(r.prepare.bytes, 933);
  assert.equal(r.open.bytes, 828);
  assert.equal(r.open.signatures, 6);
  assert.equal(r.open_separate_payer_compute.signatures, 7);
  assert.equal(r.all_instructions.length, 38);
  for (const v of Object.values(r).filter(v => v?.bytes)) assert(v.bytes <= 1232 && v.verified);
  for (const v of r.all_instructions) assert(v.bytes <= 1232 && v.verified);
});
test('none of the six required role signatures may be omitted', () => {
  const c = bootstrap(fixture({ solo: false }));
  assert.equal(new Set(c.actors.map(k => k.publicKey.toBase58())).size, 6);
  for (const actor of c.actors) {
    const tx = signedBootstrap(c, 'open', { omit: actor });
    assert.equal(tx.verifySignatures(), false);
    assert.throws(() => tx.serialize());
  }
});
test('changed preparation or program invalidates old confirmation signatures', () => {
  const c = bootstrap(fixture({ solo: false }));
  for (const field of ['preparation', 'policy', 'program']) {
    const tx = signedBootstrap(c, 'open');
    if (field === 'program') tx.instructions[0].programId = Keypair.generate().publicKey;
    else tx.instructions[0].keys[field === 'preparation' ? 8 : 9].pubkey = Keypair.generate().publicKey;
    assert.equal(tx.verifySignatures(), false);
  }
});
test('configuration changes produce different immutable preparation and policy addresses', () => {
  const f = fixture({ solo: false }), a = bootstrap(f);
  f.config.t0 += 1n;
  const b = bootstrap(f);
  assert(!a.identity.equals(b.identity));
  assert(!a.preparation.equals(b.preparation));
  assert(!a.policy.equals(b.policy));
  assert.equal(a.open.data.length, 8);
});
