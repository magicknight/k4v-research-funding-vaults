// Validate signatures over recorded bytes; does not authenticate a past RPC server.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { inspectEnvelope } from '../clients/launch_v7_local_client.mjs';
assert.equal(process.argv.length, 3, 'PUBLIC_EVIDENCE_DIRECTORY_REQUIRED');
const transactions = JSON.parse(readFileSync(join(process.argv[2], 'signed-transactions.json')));
assert.equal(transactions.length, 13);
for (const tx of transactions) inspectEnvelope(tx, tx.messageHash);
console.log(JSON.stringify({ valid: true, signed_envelopes_verified: transactions.length,
  live_rpc_checked: false, production_ready: false }));
