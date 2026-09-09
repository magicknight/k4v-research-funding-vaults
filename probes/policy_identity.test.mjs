import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { boundPolicyHash } from "./policy_identity.mjs";
const v = JSON.parse(readFileSync(new URL("../spec/POLICY_IDENTITY_VECTOR_v1.json", import.meta.url)));
test("policy identity matches the Rust/SBF protocol vector", () => {
  const args = [v.program_hex, v.creator_hex, v.mint_hex, v.policy_spec_hash_hex].map(x => Buffer.from(x, "hex"));
  assert.equal(boundPolicyHash(...args).toString("hex"), v.bound_hash_hex);
  for (let i = 0; i < args.length; i++) {
    const changed = args.map(x => Buffer.from(x)); changed[i][0] ^= 1;
    assert.notEqual(boundPolicyHash(...changed).toString("hex"), v.bound_hash_hex);
  }
  assert.throws(() => boundPolicyHash(...args.slice(0, 3), Buffer.alloc(32)));
  assert.throws(() => boundPolicyHash(...args.slice(0, 3), Buffer.alloc(31)));
});
