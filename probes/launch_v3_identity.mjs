// Offline fixed-width codec only: no RPC, wallet, signing or transaction send.
import { createHash } from 'node:crypto';

const fields = ['t0', 'founder_amount', 'treasury_amount', 'founder_period_cap',
  'treasury_period_cap', 'shared_hard_cap', 'max_report_age'];
const annualFields = ['start_period', 'end_period', 'founder_basis', 'treasury_basis', 'shared_cap'];
const exact = (value, keys) => {
  if (!value || Object.keys(value).sort().join() !== [...keys].sort().join()) {
    throw new Error('Exact frozen fields required');
  }
};
const integer = value => {
  if (typeof value !== 'bigint' && !(typeof value === 'string' && /^-?\d+$/.test(value))) {
    throw new Error('Use bigint or a decimal string, never a JavaScript Number');
  }
  return BigInt(value);
};

export function encodeConfig(config) {
  exact(config, [...fields, 'annual_rules']);
  if (!Array.isArray(config.annual_rules) || config.annual_rules.length !== 2) {
    throw new Error('This TEST_ONLY profile freezes exactly two annual input epochs');
  }
  const bytes = Buffer.alloc(204);
  fields.forEach((key, i) => {
    const value = integer(config[key]);
    if (key === 't0' || key === 'max_report_age') bytes.writeBigInt64LE(value, 8 * i);
    else bytes.writeBigUInt64LE(value, 8 * i);
  });
  config.annual_rules.forEach((rule, i) => {
    exact(rule, [...annualFields, 'release_bps', 'source_hash']);
    const offset = 56 + 74 * i;
    annualFields.forEach((key, j) => bytes.writeBigUInt64LE(integer(rule[key]), offset + 8 * j));
    const rate = integer(rule.release_bps);
    if (rate < 0n || rate > 65535n) throw new Error('Rate exceeds u16');
    bytes.writeUInt16LE(Number(rate), offset + 40);
    if (typeof rule.source_hash !== 'string' || !/^[0-9a-f]{64}$/.test(rule.source_hash)) {
      throw new Error('Source hash must be exactly 32 lower-case hex bytes');
    }
    Buffer.from(rule.source_hash, 'hex').copy(bytes, offset + 42);
  });
  return bytes;
}

export function boundLaunchIdentity({ program, creator, mint, founder, treasury, oracle, specHash, config }) {
  const keys = [program, creator, mint, founder, treasury, oracle, specHash];
  if (keys.some(key => !Buffer.isBuffer(key) || key.length !== 32)) throw new Error('Expected seven 32-byte buffers');
  const constants = Buffer.alloc(26);
  constants.writeBigInt64LE(15_552_000n, 0);
  constants.writeBigInt64LE(2_592_000n, 8);
  constants.writeBigUInt64LE(12n, 16);
  constants.writeUInt16LE(500, 24);
  return createHash('sha256').update(Buffer.concat([
    Buffer.from('k4v-launch-policy-v3-test-profile-1'), ...keys, constants, encodeConfig(config),
  ])).digest();
}
