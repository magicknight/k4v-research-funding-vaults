// Ephemeral local fixtures; all units, keys and dates are test inputs.
import { readFileSync } from 'node:fs';
import { Keypair, PublicKey, TransactionInstruction, Transaction, TransactionMessage,
  VersionedTransaction, AddressLookupTableAccount } from '@solana/web3.js';
import { encodeConfig, boundLaunchIdentity } from '../probes/launch_v7_identity.mjs';
import { PROGRAM, SYSTEM, pda, PACKET_LIMIT } from '../clients/launch_v7_local_client.mjs';
const idl = JSON.parse(readFileSync(new URL('../idl/launch_vault_v7.json', import.meta.url)));
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
  f.policy = pda(Buffer.from('launch-v7-policy'), f.identity);
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
