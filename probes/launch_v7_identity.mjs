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
  exact(config, [...fields, 'annual_rules', 'recovery_keys', 'founder_recovery_keys', 'treasury_recovery_keys']);
  if (!Array.isArray(config.annual_rules) || config.annual_rules.length !== 2) {
    throw new Error('This TEST_ONLY profile freezes exactly two annual input epochs');
  }
  const bytes = Buffer.alloc(492);
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
  ['recovery_keys', 'founder_recovery_keys', 'treasury_recovery_keys'].forEach((field, j) => {
    if (!Array.isArray(config[field]) || config[field].length !== 3) throw new Error('Exactly three recovery keys required');
    config[field].forEach((key, i) => {
      if (typeof key !== 'string' || !/^[0-9a-f]{64}$/.test(key)) throw new Error('Recovery key must be 32 hex bytes');
      Buffer.from(key, 'hex').copy(bytes, 204 + 96 * j + 32 * i);
    });
  });
  return bytes;
}

export function boundLaunchIdentity({ program, creator, mint, founder, treasury, oracle, specHash, config }) {
  const keys = [program, creator, mint, founder, treasury, oracle, specHash];
  if (keys.some(key => !Buffer.isBuffer(key) || key.length !== 32)) throw new Error('Expected seven 32-byte buffers');
  const constants = Buffer.alloc(51);
  constants.writeBigInt64LE(15_552_000n, 0);
  constants.writeBigInt64LE(2_592_000n, 8);
  constants.writeBigUInt64LE(12n, 16);
  constants.writeUInt16LE(500, 24);
  constants.writeBigInt64LE(7_776_000n, 26);
  constants.writeUInt8(2, 34);
  constants.writeBigInt64LE(2_592_000n, 35);
  constants.writeBigInt64LE(300n, 43);
  return createHash('sha256').update(Buffer.concat([
    Buffer.from('k4v-launch-policy-v7-test-profile-1'), ...keys, constants, encodeConfig(config),
  ])).digest();
}
