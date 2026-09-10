// Recheck signatures in the archived local-node transcript. Does not query a chain.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { inspectEnvelope, PROGRAM } from '../clients/launch_v7_local_client.mjs';
const path = 'evidence/e11b/accepted-local-evidence.tar.gz';
const read = name => JSON.parse(execFileSync('tar', ['-xOf', path, 'target/e11b/agave/' + name], { encoding: 'utf8', maxBuffer: 2000000 }));
const rows = read('signed-transactions.json');
assert.equal(rows.length, 15);
assert.equal(new Set(rows.map(r => r.signature)).size, 15);
for (const row of rows) {
  inspectEnvelope(row, row.messageHash);
  assert.equal(row.result.signature, row.signature);
  assert.equal(row.result.status, 'FINALIZED');
  assert.equal(row.result.error, null);
}
const opening = rows.find(r => r.label === 'six-role-open-separate-payer');
assert.equal(opening.byteLength, 924);
const tx = inspectEnvelope(opening, opening.messageHash);
assert.equal(tx.signatures.length, 7);
const instruction = tx.instructions.find(ix => ix.programId.equals(PROGRAM));
assert.equal(instruction.data.length, 8);
const actors = [0, 1, 2, 4, 5, 6].map(i => instruction.keys[i].pubkey.toBase58());
assert.equal(new Set(actors).size, 6);
assert(actors.every(key => tx.signatures.some(s => s.publicKey.toBase58() === key)));
assert(!actors.includes(tx.feePayer.toBase58()));
const damaged = { ...opening, bytes: Buffer.from(opening.bytes, 'base64').map((b, i) => i === 1 ? b ^ 1 : b).toString('base64') };
assert.throws(() => inspectEnvelope(damaged, opening.messageHash));
console.log(JSON.stringify({ valid: true, archived_signed_transactions: 15, opening_role_keys: 6,
  opening_signatures_with_separate_payer: 7, live_finality_requeried: false, independent_human_review: false }));
