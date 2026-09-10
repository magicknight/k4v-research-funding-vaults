// Compiler-IDL-based wire budgets and an independently encoded fixed identity vector. No RPC.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { Keypair, PublicKey, Transaction, TransactionInstruction, ComputeBudgetProgram } from '@solana/web3.js';
import { encodeConfig, boundLaunchIdentity } from '../probes/launch_v7_identity.mjs';
import { PROGRAM, SYSTEM, PACKET_LIMIT } from '../clients/launch_v7_local_client.mjs';
import { fixture, prepareInstruction, openInstruction } from './e11b_fixtures.mjs';
const idl = JSON.parse(readFileSync('idl/launch_vault_v7.json'));
assert.equal(idl.address, PROGRAM.toBase58());
assert.equal(idl.instructions.length, 19);
assert.equal(idl.accounts.length, 7);
assert(!idl.instructions.some(i => i.name === 'open_policy'));
const budgets = [ComputeBudgetProgram.setComputeUnitLimit({ units: 1400000 }),
  ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 1 })];
function packet(ix, payer, signers, budget) {
  const tx = new Transaction({ feePayer: payer.publicKey, recentBlockhash: Keypair.generate().publicKey.toBase58() });
  if (budget) tx.add(...budgets);
  tx.add(ix); tx.sign(...signers);
  assert(tx.verifySignatures());
  const raw = tx.serialize();
  assert(raw.length <= PACKET_LIMIT);
  const altered = Transaction.from(raw);
  altered.signatures[0].signature[0] ^= 1;
  assert(!altered.verifySignatures());
  return { bytes: raw.length, signatures: tx.signatures.length };
}
function size(t) {
  if (typeof t === 'string') {
    const n = { u8:1, bool:1, u16:2, u32:4, i32:4, u64:8, i64:8, pubkey:32 }[t];
    assert(n, 'UNKNOWN_IDL_PRIMITIVE_' + t); return n;
  }
  if (t.array) return size(t.array[0]) * t.array[1];
  if (t.defined) {
    const def = idl.types.find(d => d.name === t.defined.name);
    assert(def?.type.kind === 'struct', 'UNKNOWN_IDL_DEFINED');
    return def.type.fields.reduce((a, f) => a + size(f.type), 0);
  }
  throw new Error('UNKNOWN_IDL_TYPE');
}
const all = [];
for (const item of idl.instructions) {
  const actors = item.accounts.map(() => Keypair.generate());
  const keys = item.accounts.map((a, j) => ({ pubkey: actors[j].publicKey, isSigner: a.signer === true, isWritable: a.writable === true }));
  // Distinct accounts, all optional accounts present, independent fee payer:
  // a wire upper bound, not a semantically valid execution of arbitrary zero arguments.
  const payer = Keypair.generate();
  const ix = new TransactionInstruction({ programId: PROGRAM, keys,
    data: Buffer.concat([Buffer.from(item.discriminator), Buffer.alloc(item.args.reduce((n,a)=>n+size(a.type),0))]) });
  const signers = [payer, ...actors.filter((_,j)=>keys[j].isSigner)];
  for (const budget of [false,true]) all.push({ instruction:item.name, budget, ...packet(ix,payer,signers,budget) });
}
const f = fixture({solo:false}), separate = Keypair.generate();
const real = [];
for (const independentPayer of [false,true]) for (const budget of [false,true]) {
  const payer = independentPayer ? separate : f.creator;
  const prepareSigners = independentPayer ? [separate,f.creator] : [f.creator];
  const consentSigners = [...prepareSigners,f.founder,f.treasury,...f.recovery];
  real.push({independentPayer,budget,prepare:packet(prepareInstruction(f),payer,prepareSigners,budget),
    consent:packet(openInstruction(f),payer,consentSigners,budget)});
}
const v = JSON.parse(readFileSync('spec/LAUNCH_V6_IDENTITY_VECTOR_v1.json'));
v.program_hex = PROGRAM.toBuffer().toString('hex');
v.identity_hex = boundLaunchIdentity({program:PROGRAM.toBuffer(), creator:Buffer.from(v.creator_hex,'hex'),
  mint:Buffer.from(v.mint_hex,'hex'), founder:Buffer.from(v.founder_hex,'hex'), treasury:Buffer.from(v.treasury_hex,'hex'),
  oracle:Buffer.from(v.oracle_hex,'hex'), specHash:Buffer.from(v.specHash_hex,'hex'), config:v.config}).toString('hex');
v.config_borsh_hex = encodeConfig(v.config).toString('hex');
v.scope = 'E11B_GENERATED_TEST_VECTOR_CHECKED_AGAINST_COMPILED_RUST';
writeFileSync('spec/LAUNCH_V7_IDENTITY_VECTOR_v1.json',JSON.stringify(v,null,2)+'\n');
const result={schema:'K4V-E11B-WIRE-v1',packet_limit:PACKET_LIMIT,all_instruction_variants:all,bootstrap_variants:real,
  public_rpc_calls:0, private_keys_serialized:false, runtime_acceptance:false};
writeFileSync('target/e11b/wire.json',JSON.stringify(result,null,2)+'\n');
console.log('E11B_WIRE_RESULT '+JSON.stringify(result));
