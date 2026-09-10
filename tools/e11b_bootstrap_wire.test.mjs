import test from 'node:test';
import assert from 'node:assert/strict';
import { Keypair, Transaction } from '@solana/web3.js';
import { fixture } from './e11_fixtures.mjs';
import { proposedBootstrap, signedWire, wireReceipt } from './e11b_bootstrap_wire.mjs';
test('two-stage layout fits with distinct actors, extra payer and compute instruction', () => {
  const receipt = wireReceipt();
  for (const name of ['prepare', 'prepare_compute', 'prepare_separate_payer_compute', 'open', 'open_compute', 'open_separate_payer_compute']) {
    assert(receipt[name].bytes <= 1232, name);
    assert.equal(receipt[name].verified, true);
  }
  assert.equal(receipt.prepare.bytes, 933);
  assert.equal(receipt.open.bytes, 827);
  assert.equal(receipt.open.signatures, 6);
  assert.equal(receipt.open_separate_payer_compute.signatures, 7);
  console.log('E11B_WIRE_DESIGN ' + JSON.stringify(receipt));
});
test('every one of the six independent open signatures is mandatory', () => {
  const c = proposedBootstrap();
  for (const signer of c.actors) {
    const tx = signedWire(c, 'open', { omit: signer });
    assert.equal(tx.verifySignatures(), false);
    assert.throws(() => tx.serialize(), /signature/i);
  }
  assert.throws(() => proposedBootstrap(fixture({ solo: true })), /SIX_DISTINCT/);
});
test('config or program replacement changes the address bound by final consent', () => {
  const f = fixture({ solo: false }), program = Keypair.generate().publicKey;
  const a = proposedBootstrap(f, program), tx = signedWire(a, 'open');
  const changed = proposedBootstrap({ ...f, config: { ...f.config, t0: f.config.t0 + 1n } }, program);
  assert.notEqual(changed.preparation.toBase58(), a.preparation.toBase58());
  assert.notEqual(changed.policy.toBase58(), a.policy.toBase58());
  const tampered = Transaction.from(tx.serialize());
  tampered.instructions[0].keys[8].pubkey = changed.preparation;
  assert.equal(tampered.verifySignatures(), false);
  assert.notEqual(proposedBootstrap(f, Keypair.generate().publicKey).identity.toString('hex'), a.identity.toString('hex'));
  assert.equal(a.open.keys[8].isWritable, false);
  assert.equal(a.open.keys[8].pubkey.toBase58(), a.preparation.toBase58());
});
