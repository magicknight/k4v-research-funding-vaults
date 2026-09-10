// DESIGN_ONLY. Offline proposed wire layout; no deployed program implements it.
import { createHash } from 'node:crypto';
import { PublicKey, Keypair, Transaction, TransactionInstruction, ComputeBudgetProgram } from '@solana/web3.js';
import { encodeConfig } from '../probes/launch_v6_identity.mjs';
import { fixture } from './e11_fixtures.mjs';

const digest = bytes => createHash('sha256').update(bytes).digest();
const discriminator = name => digest(Buffer.from('global:' + name)).subarray(0, 8);
const system = new PublicKey('11111111111111111111111111111111');
const meta = (pubkey, isSigner = false, isWritable = false) => ({ pubkey, isSigner, isWritable });
export function proposedBootstrap(f = fixture({ solo: false }), program = Keypair.generate().publicKey) {
  const actors = [f.creator, f.founder, f.treasury, ...f.recovery];
  if (new Set(actors.map(k => k.publicKey.toBase58())).size !== 6) throw new Error('SIX_DISTINCT_ACTORS_REQUIRED');
  const identity = digest(Buffer.concat([Buffer.from('k4v-e11b-bootstrap-design-v1'), program.toBuffer(),
    ...[f.creator, f.mint, f.founder, f.treasury, f.oracle].map(k => k.publicKey.toBuffer()),
    f.specHash, encodeConfig(f.config)]));
  const derive = prefix => PublicKey.findProgramAddressSync([Buffer.from(prefix), identity], program)[0];
  const preparation = derive('e11b-design-preparation'), policy = derive('e11b-design-policy');
  const prepare = new TransactionInstruction({ programId: program,
    keys: [meta(f.creator.publicKey, true, true), meta(f.founder.publicKey), meta(f.treasury.publicKey),
      meta(f.oracle.publicKey), meta(f.mint.publicKey), meta(preparation, false, true), meta(system)],
    data: Buffer.concat([discriminator('prepare_policy'), encodeConfig(f.config), f.specHash, identity]) });
  const open = new TransactionInstruction({ programId: program,
    keys: [meta(f.creator.publicKey, true, true), meta(f.founder.publicKey, true), meta(f.treasury.publicKey, true),
      meta(f.oracle.publicKey), ...f.recovery.map(k => meta(k.publicKey, true)), meta(f.mint.publicKey),
      meta(preparation), meta(policy, false, true), meta(system)],
    data: discriminator('open_prepared_policy') });
  return { f, program, actors, identity, preparation, policy, prepare, open };
}
export function signedWire(candidate, stage, { compute = false, omit = null, payer = candidate.f.creator } = {}) {
  const ix = candidate[stage];
  if (!['prepare', 'open'].includes(stage)) throw new Error('STAGE');
  const tx = new Transaction({ feePayer: payer.publicKey, recentBlockhash: Keypair.generate().publicKey.toBase58() });
  if (compute) tx.add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400000 }));
  tx.add(ix);
  const signers = [...new Map([payer, ...(stage === 'open' ? candidate.actors : [candidate.f.creator])]
    .filter(k => k !== omit).map(k => [k.publicKey.toBase58(), k])).values()];
  tx.partialSign(...signers);
  return tx;
}
export function wireReceipt(candidate = proposedBootstrap()) {
  const measure = (stage, compute, externalPayer) => {
    const tx = signedWire(candidate, stage, { compute, payer: externalPayer ?? candidate.f.creator });
    return { bytes: tx.serialize().length, signatures: tx.signatures.length, verified: tx.verifySignatures() };
  };
  const extraPayer = Keypair.generate();
  return { schema: 'K4V-E11B-OFFLINE-WIRE-DESIGN-v1', status: 'DESIGN_ONLY', packet_limit: 1232,
    prepare: measure('prepare', false), prepare_compute: measure('prepare', true),
    prepare_separate_payer_compute: measure('prepare', true, extraPayer),
    open: measure('open', false), open_compute: measure('open', true),
    open_separate_payer_compute: measure('open', true, extraPayer),
    sbf_implemented: false, actual_node_executed: false };
}
