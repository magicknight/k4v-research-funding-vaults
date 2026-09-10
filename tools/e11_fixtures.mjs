// Ephemeral local fixtures; all units, keys and dates are test inputs.
import { readFileSync } from 'node:fs';
import { Keypair, PublicKey, TransactionInstruction, Transaction, TransactionMessage,
  VersionedTransaction, AddressLookupTableAccount } from '@solana/web3.js';
import { encodeConfig, boundLaunchIdentity } from '../probes/launch_v6_identity.mjs';
import { PROGRAM, SYSTEM, pda, PACKET_LIMIT } from '../clients/launch_v6_local_client.mjs';
const idl = JSON.parse(readFileSync(new URL('../idl/launch_vault_v6.json', import.meta.url)));
export const UNIT = 1000000000n, SUPPLY = 1000000000n * UNIT;
export function fixture({ solo, t0 = 2000000000n }) {
  const creator = Keypair.generate();
  const f = { creator, founder: solo ? creator : Keypair.generate(), treasury: solo ? creator : Keypair.generate(),
    oracle: Keypair.generate(), mint: Keypair.generate(), source: Keypair.generate(),
    founderOut: Keypair.generate(), treasuryOut: Keypair.generate(), recipient: Keypair.generate(),
    recovery: Array.from({ length: 3 }, () => Keypair.generate()),
    backups: Array.from({ length: 2 }, () => Array.from({ length: 3 }, () => Keypair.generate())) };
  f.config = { t0, founder_amount: 300000000n * UNIT, treasury_amount: 500000000n * UNIT,
    founder_period_cap: 1000000n * UNIT, treasury_period_cap: 1500000n * UNIT,
    shared_hard_cap: 3000000n * UNIT, max_report_age: 86400n,
    annual_rules: [
      { start_period: 0n, end_period: 12n, founder_basis: 300000000n * UNIT,
        treasury_basis: 500000000n * UNIT, shared_cap: 40000000n * UNIT, release_bps: 500n, source_hash: '46'.repeat(32) },
      { start_period: 12n, end_period: 24n, founder_basis: 240000000n * UNIT,
        treasury_basis: 480000000n * UNIT, shared_cap: 36000000n * UNIT, release_bps: 500n, source_hash: '47'.repeat(32) }],
    recovery_keys: f.recovery.map(k => k.publicKey.toBuffer().toString('hex')),
    founder_recovery_keys: f.backups[0].map(k => k.publicKey.toBuffer().toString('hex')),
    treasury_recovery_keys: f.backups[1].map(k => k.publicKey.toBuffer().toString('hex')) };
  f.specHash = Buffer.alloc(32, 42);
  f.identity = boundLaunchIdentity({ program: PROGRAM.toBuffer(), creator: creator.publicKey.toBuffer(),
    mint: f.mint.publicKey.toBuffer(), founder: f.founder.publicKey.toBuffer(), treasury: f.treasury.publicKey.toBuffer(),
    oracle: f.oracle.publicKey.toBuffer(), specHash: f.specHash, config: f.config });
  f.policy = pda(Buffer.from('launch-v6-policy'), f.identity);
  return f;
}
export function instruction(name, accounts, args = []) {
  const item = idl.instructions.find(i => i.name === name);
  if (!item) throw new Error('UNKNOWN_INSTRUCTION');
  return new TransactionInstruction({ programId: PROGRAM,
    data: Buffer.concat([Buffer.from(item.discriminator), ...args]),
    keys: item.accounts.map(a => ({ pubkey: new PublicKey(accounts[a.name] ?? (a.optional ? PROGRAM : accounts[a.name])),
      isSigner: a.signer === true, isWritable: a.writable === true })) });
}
export function openInstruction(f) {
  return instruction('open_policy', { creator: f.creator.publicKey, founder: f.founder.publicKey,
    treasury: f.treasury.publicKey, oracle: f.oracle.publicKey,
    recovery_one: f.recovery[0].publicKey, recovery_two: f.recovery[1].publicKey,
    recovery_three: f.recovery[2].publicKey, mint: f.mint.publicKey, policy: f.policy, system_program: SYSTEM },
  [encodeConfig(f.config), f.specHash, f.identity]);
}
export function wireSizes() {
  const f = fixture({ solo: false }), ix = openInstruction(f), blockhash = Keypair.generate().publicKey.toBase58();
  const tx = new Transaction({ feePayer: f.creator.publicKey, recentBlockhash: blockhash }).add(ix);
  tx.sign(f.creator, f.founder, f.treasury, ...f.recovery);
  const legacySize = 1 + 64 * tx.signatures.length + tx.serializeMessage().length;
  let legacyRejected = false;
  try { tx.serialize(); } catch (e) { legacyRejected = /too large/i.test(e.message); }
  // All four eligible non-signers share one ideal lookup table. Signers and invoked
  // program IDs must remain static. This is the smallest v0 form for this ABI.
  const lookup = new AddressLookupTableAccount({ key: Keypair.generate().publicKey, state: {
    deactivationSlot: (1n << 64n) - 1n, lastExtendedSlot: 0, lastExtendedSlotStartIndex: 0, authority: undefined,
    addresses: [f.oracle.publicKey, f.mint.publicKey, f.policy, SYSTEM],
  } });
  const message = new TransactionMessage({ payerKey: f.creator.publicKey, recentBlockhash: blockhash, instructions: [ix] })
    .compileToV0Message([lookup]);
  const v0 = new VersionedTransaction(message);
  v0.sign([f.creator, f.founder, f.treasury, ...f.recovery]);
  const versioned = Buffer.from(v0.serialize());
  const solo = fixture({ solo: true });
  const small = new Transaction({ feePayer: solo.creator.publicKey, recentBlockhash: blockhash }).add(openInstruction(solo));
  small.sign(solo.creator, ...solo.recovery);
  return { receipt: { packet_limit: PACKET_LIMIT, instruction_data_bytes: ix.data.length,
    independent_signers: tx.signatures.length, independent_legacy_bytes: legacySize,
    independent_legacy_sdk_rejected: legacyRejected, independent_best_v0_bytes: versioned.length,
    v0_lookup_addresses: message.addressTableLookups.reduce((n, t) => n + t.readonlyIndexes.length + t.writableIndexes.length, 0),
    independent_configuration_fits: versioned.length <= PACKET_LIMIT,
    solo_legacy_bytes: small.serialize().length, solo_signature_count: small.signatures.length },
    // Negative node probe only. No funded account or usable blockhash is needed:
    // packet decoding must reject before execution.
    oversizedV0: versioned.toString('base64') };
}
