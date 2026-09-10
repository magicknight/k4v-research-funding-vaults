import test from 'node:test';
import assert from 'node:assert/strict';
import { Keypair, SystemProgram, Transaction } from '@solana/web3.js';
import { hash, integer, loopbackEndpoint, proposalInstruction, signInstructions, inspectEnvelope,
  submitSigned } from './launch_v6_local_client.mjs';
import { wireSizes } from '../tools/e11_fixtures.mjs';
const a = Keypair.generate(), b = Keypair.generate(), blockhash = Keypair.generate().publicKey.toBase58();
function rpc({ lostSend = false, neverConfirm = false, expired = false, simulationError = false,
  statusError = null, wrongSignature = false } = {}) {
  let sent = 0, simulated = 0;
  return { get sent() { return sent; }, get simulated() { return simulated; },
    async call(method) {
      if (method === 'getLatestBlockhash') return { value: { blockhash, lastValidBlockHeight: 100 } };
      if (method === 'getSignatureStatuses') return { value: [sent && !neverConfirm ?
        { slot: 12, err: statusError, confirmationStatus: 'finalized' } : null] };
      if (method === 'isBlockhashValid') return { value: !expired };
      if (method === 'getBlockHeight') return expired ? 101 : 50;
      if (method === 'simulateTransaction') { simulated++; return { value: { err: simulationError ? 'Invalid' : null } }; }
      if (method === 'sendTransaction') { sent++; if (lostSend) throw new Error('response lost'); return wrongSignature ? 'wrong' : null; }
      throw new Error('UNEXPECTED_' + method);
    } };
}
async function envelope(r) {
  return signInstructions(r, [SystemProgram.transfer({ fromPubkey: a.publicKey, toPubkey: b.publicKey, lamports: 1 })],
    a.publicKey, [a]);
}
test('independent v6 bootstrap exceeds both legacy and ideal v0 packet budgets', () => {
  const { receipt: r } = wireSizes();
  assert.equal(r.instruction_data_bytes, 564);
  assert.equal(r.independent_legacy_bytes, 1352);
  assert.equal(r.independent_best_v0_bytes, 1264);
  assert.equal(r.independent_legacy_sdk_rejected, true);
  assert.equal(r.v0_lookup_addresses, 4);
  assert.equal(r.independent_configuration_fits, false);
  assert.equal(r.solo_legacy_bytes, 1160);
  assert.equal(r.solo_signature_count, 4);
});
test('endpoint parsing excludes aliases, redirects, credentials and public URLs', () => {
  assert.equal(loopbackEndpoint('http://127.0.0.1:19599'), 'http://127.0.0.1:19599/');
  for (const value of ['https://api.mainnet-beta.solana.com', 'http://localhost:19599',
    'http://127.1:19599', 'http://127.0.0.1:19599@evil.test', 'http://127.0.0.1:19599/a',
    'http://127.0.0.1:19599?x=1', 'http://127.0.0.1:0', 'http://127.0.0.1:65536']) {
    assert.throws(() => loopbackEndpoint(value));
  }
});
test('proposal codec has signed inclusive bounds and strict widths', () => {
  const fields = { payer: a.publicKey, initiator: a.publicKey, cosigner: a.publicKey,
    successor: b.publicKey, policy: b.publicKey, role: 0, recovery: false,
    nonce: 1n, epoch: 0n, validFrom: 100n, validUntil: 400n, predecessor: a.publicKey };
  const ix = proposalInstruction(fields);
  assert.equal(ix.data.length, 74);
  assert.equal(ix.data.readBigInt64LE(26), 100n);
  assert.equal(ix.data.readBigInt64LE(34), 400n);
  assert.throws(() => proposalInstruction({ ...fields, validUntil: 401n }), /SUBMISSION_WINDOW/);
  assert.throws(() => proposalInstruction({ ...fields, validFrom: -1n }), /SUBMISSION_WINDOW/);
  assert.throws(() => proposalInstruction({ ...fields, validFrom: (1n << 63n) - 1n, validUntil: (1n << 63n) - 1n }), /SUBMISSION_WINDOW/);
  assert.throws(() => proposalInstruction({ ...fields, nonce: 1 }), /BIGINT/);
  assert.throws(() => proposalInstruction({ ...fields, recovery: 1 }), /BOOL/);
  assert.throws(() => integer(1n << 64n), /RANGE/);
});
test('changed message and corrupted or missing signatures cannot be submitted', async () => {
  const r = rpc(), e = await envelope(r);
  assert.equal(inspectEnvelope(e, e.messageHash).verifySignatures(), true);
  const bytes = Buffer.from(e.bytes, 'base64'); bytes[1] ^= 1;
  assert.throws(() => inspectEnvelope({ ...e, bytes: bytes.toString('base64') }, e.messageHash), /SIGNATURE/);
  bytes.fill(0, 1, 65);
  assert.throws(() => inspectEnvelope({ ...e, bytes: bytes.toString('base64') }, e.messageHash), /SIGNATURE/);
  const tx = Transaction.from(Buffer.from(e.bytes, 'base64')); tx.instructions[0].data[4] ^= 1;
  const changed = tx.serialize({ verifySignatures: false });
  assert.throws(() => inspectEnvelope({ ...e, bytes: changed.toString('base64') }, e.messageHash), /MESSAGE/);
  assert.equal(r.sent, 0);
});
test('expired blockhash requires a new reviewed signature without simulation or send', async () => {
  const r = rpc({ expired: true }), e = await envelope(r), outcome = await submitSigned(r, e, e.messageHash);
  assert.equal(outcome.status, 'REBUILD_AND_RESIGN'); assert.equal(outcome.reason, 'BLOCKHASH_EXPIRED');
  assert.equal(r.sent, 0); assert.equal(r.simulated, 0);
});
test('semantic guard rejects stale intent before broadcasting', async () => {
  const r = rpc(), e = await envelope(r);
  const out = await submitSigned(r, e, e.messageHash, { guard: async () => 'POLICY_CHANGED' });
  assert.equal(out.reason, 'POLICY_CHANGED'); assert.equal(r.sent, 0);
});
test('semantic guard runs again after simulation', async () => {
  const r = rpc(), e = await envelope(r); let checks = 0;
  const out = await submitSigned(r, e, e.messageHash, { guard: async () => ++checks === 2 ? 'SUBMISSION_WINDOW_CLOSED' : null });
  assert.equal(out.reason, 'SUBMISSION_WINDOW_CLOSED'); assert.equal(r.simulated, 1); assert.equal(r.sent, 0);
});
test('simulation rejection cannot be reported as submitted or finalized', async () => {
  const r = rpc({ simulationError: true }), e = await envelope(r);
  assert.equal((await submitSigned(r, e, e.messageHash)).status, 'SIMULATION_REJECTED');
  assert.equal(r.sent, 0);
});
test('lost send response is reconciled from the actual signature status', async () => {
  const r = rpc({ lostSend: true }), e = await envelope(r);
  assert.equal((await submitSigned(r, e, e.messageHash)).status, 'FINALIZED');
  assert.equal(r.sent, 1);
});
test('RPC send acknowledgement alone is insufficient; retries never re-sign or resend', async () => {
  const r = rpc({ neverConfirm: true, wrongSignature: true }), e = await envelope(r), bytes = e.bytes;
  assert.equal((await submitSigned(r, e, e.messageHash, { timeoutMs: 0 })).status, 'UNKNOWN');
  assert.equal((await submitSigned(r, e, e.messageHash, { timeoutMs: 0 })).status, 'UNKNOWN');
  assert.equal(r.sent, 1); assert.equal(e.bytes, bytes);
});
test('finalized program error is failure even when the transport succeeds', async () => {
  const r = rpc({ statusError: { InstructionError: [0, 'Custom'] } }), e = await envelope(r);
  assert.equal((await submitSigned(r, e, e.messageHash)).status, 'ONCHAIN_FAILED');
});
