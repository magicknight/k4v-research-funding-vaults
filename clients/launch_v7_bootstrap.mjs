// TEST_ONLY compiled-ABI bootstrap. No network, deployment or production parameters.
import { readFileSync } from 'node:fs';
import { Keypair, Transaction, ComputeBudgetProgram } from '@solana/web3.js';
import { encodeConfig, boundLaunchIdentity } from '../probes/launch_v7_identity.mjs';
import { PROGRAM, SYSTEM, pda } from './launch_v7_local_client.mjs';
import { fixture, instruction } from '../tools/e11b_fixtures.mjs';
const idl = JSON.parse(readFileSync(new URL('../idl/launch_vault_v7.json', import.meta.url)));
export function bootstrap(f) {
  const identity = boundLaunchIdentity({ program: PROGRAM.toBuffer(), creator: f.creator.publicKey.toBuffer(),
    mint: f.mint.publicKey.toBuffer(), founder: f.founder.publicKey.toBuffer(), treasury: f.treasury.publicKey.toBuffer(),
    oracle: f.oracle.publicKey.toBuffer(), specHash: f.specHash, config: f.config });
  const preparation = pda(Buffer.from('launch-v7-preparation'), identity), policy = pda(Buffer.from('launch-v7-policy'), identity);
  const actors = [f.creator, f.founder, f.treasury, ...f.recovery];
  const common = { creator: f.creator.publicKey, founder: f.founder.publicKey, treasury: f.treasury.publicKey,
    oracle: f.oracle.publicKey, mint: f.mint.publicKey, preparation, system_program: SYSTEM };
  return Object.freeze({ identity, preparation, policy, actors,
    prepare: instruction('prepare_policy', common, [encodeConfig(f.config), f.specHash, identity]),
    open: instruction('open_prepared_policy', { ...common, policy,
      recovery_one: f.recovery[0].publicKey, recovery_two: f.recovery[1].publicKey, recovery_three: f.recovery[2].publicKey }),
  });
}
export function signedBootstrap(candidate, stage, { compute = false, payer = candidate.actors[0], omit = null } = {}) {
  if (!['prepare', 'open'].includes(stage)) throw new Error('STAGE');
  const tx = new Transaction({ feePayer: payer.publicKey, recentBlockhash: Keypair.generate().publicKey.toBase58() });
  if (compute) tx.add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400000 }));
  tx.add(candidate[stage]);
  const signers = [...new Map([payer, ...(stage === 'open' ? candidate.actors : [candidate.actors[0]])]
    .filter(k => k !== omit).map(k => [k.publicKey.toBase58(), k])).values()];
  tx.partialSign(...signers);
  return tx;
}
function sampleType(type) {
  if (type === 'pubkey') return Keypair.generate().publicKey.toBuffer();
  const widths = { u8: 1, bool: 1, u16: 2, u32: 4, i64: 8, u64: 8 };
  if (Object.hasOwn(widths, type)) return Buffer.alloc(widths[type], 1);
  if (type.array) return Buffer.concat(Array.from({ length: type.array[1] }, () => sampleType(type.array[0])));
  if (type.defined) {
    const def = idl.types.find(t => t.name === type.defined.name);
    if (def?.type.kind !== 'struct') throw new Error('UNKNOWN_STRUCT');
    return Buffer.concat(def.type.fields.map(f => sampleType(f.type)));
  }
  throw new Error('UNKNOWN_TYPE');
}
export function wireReceipt() {
  const candidate = bootstrap(fixture({ solo: false })), extra = Keypair.generate();
  const measure = (stage, compute, payer) => {
    const tx = signedBootstrap(candidate, stage, { compute, payer });
    return { bytes: tx.serialize().length, signatures: tx.signatures.length, verified: tx.verifySignatures() };
  };
  const allInstructions = [];
  for (const item of idl.instructions) {
    const keys = Object.fromEntries(item.accounts.map(a => [a.name, Keypair.generate()]));
    const accounts = Object.fromEntries(Object.entries(keys).map(([name, key]) => [name, key.publicKey]));
    const ix = instruction(item.name, accounts, item.args.map(a => sampleType(a.type)));
    for (const externalPayer of [false, true]) {
      const signers = item.accounts.filter(a => a.signer).map(a => keys[a.name]);
      const payer = externalPayer || !signers.length ? Keypair.generate() : signers[0];
      const tx = new Transaction({ feePayer: payer.publicKey, recentBlockhash: Keypair.generate().publicKey.toBase58() })
        .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 1400000 }), ix);
      tx.sign(...new Map([payer, ...signers].map(k => [k.publicKey.toBase58(), k])).values());
      allInstructions.push({ instruction: item.name, external_payer: externalPayer,
        all_optional_accounts_present: true, signatures: tx.signatures.length, bytes: tx.serialize().length,
        verified: tx.verifySignatures(), semantic_execution_claimed: false });
    }
  }
  return { schema: 'K4V-E11B-COMPILED-ABI-WIRE-v1', packet_limit: 1232,
    prepare: measure('prepare', false), prepare_compute: measure('prepare', true),
    prepare_separate_payer_compute: measure('prepare', true, extra),
    open: measure('open', false), open_compute: measure('open', true),
    open_separate_payer_compute: measure('open', true, extra), all_instructions: allInstructions };
}
