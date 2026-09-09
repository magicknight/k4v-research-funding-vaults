import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { boundLaunchIdentity, encodeConfig } from './launch_v2_identity.mjs';

const vector = JSON.parse(readFileSync(new URL('../spec/LAUNCH_V2_IDENTITY_VECTOR_v1.json', import.meta.url)));
const input = { config: vector.config };
for (const key of ['program', 'creator', 'mint', 'founder', 'treasury', 'oracle', 'specHash']) {
  input[key] = Buffer.from(vector[`${key}_hex`], 'hex');
}

test('frozen Rust/JavaScript/Python identity and fixed-width config', () => {
  assert.equal(encodeConfig(input.config).toString('hex'), vector.config_borsh_hex);
  assert.equal(boundLaunchIdentity(input).toString('hex'), vector.identity_hex);
});

test('every actor, specification and config field changes the identity', () => {
  for (const key of ['program', 'creator', 'mint', 'founder', 'treasury', 'oracle', 'specHash']) {
    const changed = { ...input, [key]: Buffer.from(input[key]) };
    changed[key][0] ^= 1;
    assert.notEqual(boundLaunchIdentity(changed).toString('hex'), vector.identity_hex);
  }
  for (const key of Object.keys(input.config)) {
    const changed = { ...input, config: { ...input.config, [key]: (BigInt(input.config[key]) + 1n).toString() } };
    assert.notEqual(boundLaunchIdentity(changed).toString('hex'), vector.identity_hex);
  }
});

test('reject lossy numbers, overflow and extra config fields', () => {
  assert.throws(() => encodeConfig({ ...input.config, founder_amount: 300000000000000000 }));
  assert.throws(() => encodeConfig({ ...input.config, founder_amount: 1n << 64n }));
  assert.throws(() => encodeConfig({ ...input.config, t0: 1n << 63n }));
  assert.throws(() => encodeConfig({ ...input.config, extra: '1' }));
});
