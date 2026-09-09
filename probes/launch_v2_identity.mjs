// Offline identity codec. Does not connect to an RPC, sign or send transactions.
import { createHash } from 'node:crypto';

export const CLIFF = 15_552_000n;
export const PERIOD = 2_592_000n;
const fields = ['t0', 'founder_amount', 'treasury_amount', 'founder_period_cap',
  'treasury_period_cap', 'shared_hard_cap', 'max_report_age'];
const signed = new Set(['t0', 'max_report_age']);

export function encodeConfig(config) {
  const result = Buffer.alloc(56);
  if (Object.keys(config).sort().join() !== [...fields].sort().join()) {
    throw new Error('Exact LaunchConfig fields required');
  }
  fields.forEach((name, i) => {
    const value = config[name];
    if (typeof value !== 'bigint' && !(typeof value === 'string' && /^-?\d+$/.test(value))) {
      throw new Error(`${name}: use bigint or decimal string, never a JavaScript Number`);
    }
    if (signed.has(name)) result.writeBigInt64LE(BigInt(value), i * 8);
    else result.writeBigUInt64LE(BigInt(value), i * 8);
  });
  return result;
}

export function boundLaunchIdentity({ program, creator, mint, founder, treasury, oracle, specHash, config }) {
  const keys = [program, creator, mint, founder, treasury, oracle, specHash];
  if (keys.some(key => !Buffer.isBuffer(key) || key.length !== 32)) throw new Error('Expected seven 32-byte buffers');
  const clocks = Buffer.alloc(16);
  clocks.writeBigInt64LE(CLIFF, 0);
  clocks.writeBigInt64LE(PERIOD, 8);
  return createHash('sha256').update(Buffer.concat([
    Buffer.from('k4v-launch-policy-v2-test-profile-1'), ...keys, clocks, encodeConfig(config),
  ])).digest();
}
