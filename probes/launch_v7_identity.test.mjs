import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { boundLaunchIdentity, encodeConfig } from './launch_v7_identity.mjs';

const v = JSON.parse(readFileSync(new URL('../spec/LAUNCH_V7_IDENTITY_VECTOR_v1.json', import.meta.url)));
const input = { config: v.config };
const actors = ['program', 'creator', 'mint', 'founder', 'treasury', 'oracle', 'specHash'];
for (const key of actors) input[key] = Buffer.from(v[`${key}_hex`], 'hex');

test('fixed-width config and identity agree with the Rust/Python vector', () => {
  assert.equal(encodeConfig(input.config).toString('hex'), v.config_borsh_hex);
  assert.equal(boundLaunchIdentity(input).toString('hex'), v.identity_hex);
});

test('actors, every base field, annual rule and source hash are identity-bound', () => {
  for (const key of actors) {
    const changed = { ...input, [key]: Buffer.from(input[key]) };
    changed[key][0] ^= 1;
    assert.notEqual(boundLaunchIdentity(changed).toString('hex'), v.identity_hex);
  }
  for (const key of Object.keys(input.config).filter(key => !['annual_rules', 'recovery_keys', 'founder_recovery_keys', 'treasury_recovery_keys'].includes(key))) {
    const config = structuredClone(input.config);
    config[key] = (BigInt(config[key]) + 1n).toString();
    assert.notEqual(boundLaunchIdentity({ ...input, config }).toString('hex'), v.identity_hex);
  }
  for (let i = 0; i < 2; i++) {
    for (const key of Object.keys(input.config.annual_rules[i])) {
      const config = structuredClone(input.config);
      const rule = config.annual_rules[i];
      rule[key] = key === 'source_hash' ? 'ab'.repeat(32) : (BigInt(rule[key]) + 1n).toString();
      assert.notEqual(boundLaunchIdentity({ ...input, config }).toString('hex'), v.identity_hex);
    }
  }
});

test('reject lossy integers, overflow, malformed hashes and wrong epoch counts', () => {
  assert.throws(() => encodeConfig({ ...input.config, founder_amount: 300000000000000000 }));
  assert.throws(() => encodeConfig({ ...input.config, founder_amount: 1n << 64n }));
  assert.throws(() => encodeConfig({ ...input.config, extra: '1' }));
  assert.throws(() => encodeConfig({ ...input.config, annual_rules: [] }));
  for (const [key, value] of [['source_hash', '00'], ['release_bps', '65536']]) {
    const config = structuredClone(input.config);
    config.annual_rules[0][key] = value;
    assert.throws(() => encodeConfig(config));
  }
});


test('all three recovery committees and their ordering are identity-bound', () => {
  for (const field of ['recovery_keys', 'founder_recovery_keys', 'treasury_recovery_keys']) {
    for (let i = 0; i < 3; i++) {
      const config = structuredClone(input.config);
      config[field][i] = 'ee'.repeat(32);
      assert.notEqual(boundLaunchIdentity({ ...input, config }).toString('hex'), v.identity_hex);
    }
    const config = structuredClone(input.config);
    config[field].reverse();
    assert.notEqual(boundLaunchIdentity({ ...input, config }).toString('hex'), v.identity_hex);
    assert.throws(() => encodeConfig({ ...input.config, [field]: [] }));
    assert.throws(() => encodeConfig({ ...input.config, [field]: ['bad', ...input.config[field].slice(1)] }));
  }
});
