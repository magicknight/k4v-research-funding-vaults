// SPDX-License-Identifier: MIT OR Apache-2.0
import { createHash } from "node:crypto";

// All key inputs are raw 32-byte public keys, never base58 text.
export function boundPolicyHash(program, creator, mint, specHash) {
  const parts = [program, creator, mint, specHash].map(value => Buffer.from(value));
  if (parts.some(value => value.length !== 32)) throw new Error("policy identity inputs must each be 32 bytes");
  if (parts[3].every(value => value === 0)) throw new Error("policy specification hash must be nonzero");
  return createHash("sha256").update(Buffer.concat([Buffer.from("k4v-policy-authority-v1"), ...parts])).digest();
}
