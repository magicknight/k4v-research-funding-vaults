// TEST_ONLY local Agave client. No wallet files, public RPC or automatic re-signing.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { PublicKey, Transaction, TransactionInstruction } from '@solana/web3.js';

export const PROGRAM = new PublicKey('CYFsfATtQB3Excjsm4Cuh8ZWnPE5j6XAU3GS3RKXmUcK');
export const LOADER = new PublicKey('BPFLoaderUpgradeab1e11111111111111111111111');
export const CLOCK = new PublicKey('SysvarC1ock11111111111111111111111111111111');
export const SYSVAR = 'Sysvar1111111111111111111111111111111111111';
export const SYSTEM = new PublicKey('11111111111111111111111111111111');
const BUILD = JSON.parse(readFileSync(new URL('../spec/LAUNCH_V7_BUILD_IDENTITY_v1.json', import.meta.url)));
if (BUILD.program !== PROGRAM.toBase58()) throw new Error('BUILD_PROGRAM');
export const CODE_SHA256 = BUILD.profiles.test.sha256;
export const CODE_BYTES = BUILD.profiles.test.bytes;
export const PACKET_LIMIT = 1232;
export const hash = data => createHash('sha256').update(data).digest('hex');
export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export const requireThat = (condition, code) => { if (!condition) throw new Error(code); };
export const pda = (...seeds) => PublicKey.findProgramAddressSync(seeds, PROGRAM)[0];
export function integer(value, signed = false) {
  requireThat(typeof value === 'bigint', 'BIGINT_REQUIRED');
  requireThat(value >= (signed ? -(1n << 63n) : 0n) && value < (signed ? 1n << 63n : 1n << 64n), 'INTEGER_RANGE');
  const out = Buffer.alloc(8);
  if (signed) out.writeBigInt64LE(value); else out.writeBigUInt64LE(value);
  return out;
}
export function base58(bytes) {
  const alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
  let n = BigInt('0x' + (Buffer.from(bytes).toString('hex') || '0')), result = '';
  while (n) { result = alphabet[Number(n % 58n)] + result; n /= 58n; }
  for (const b of bytes) { if (b !== 0) break; result = '1' + result; }
  return result;
}
export function loopbackEndpoint(endpoint) {
  requireThat(typeof endpoint === 'string' && /^http:\/\/127\.0\.0\.1:[1-9][0-9]*\/?$/.test(endpoint), 'LOOPBACK_ONLY');
  const u = new URL(endpoint);
  requireThat(Number(u.port) > 0 && Number(u.port) <= 65535, 'LOOPBACK_PORT');
  return u.href;
}
export class LocalRpc {
  constructor(endpoint) { this.endpoint = loopbackEndpoint(endpoint); this.id = 0; }
  async call(method, params = []) {
    const allowed = ['getHealth', 'getVersion', 'getGenesisHash', 'getMultipleAccounts',
      'getLatestBlockhash', 'getBlockHeight', 'getSlot', 'isBlockhashValid',
      'getSignatureStatuses', 'getMinimumBalanceForRentExemption', 'requestAirdrop',
      'simulateTransaction', 'sendTransaction'];
    requireThat(allowed.includes(method), 'RPC_METHOD');
    const id = ++this.id;
    const response = await fetch(this.endpoint, {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(15000),
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
    });
    requireThat(response.ok && response.body, 'RPC_HTTP');
    const reader = response.body.getReader(), chunks = [];
    let size = 0;
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        size += value.length;
        requireThat(size <= 3000000, 'RPC_RESPONSE_TOO_LARGE');
        chunks.push(Buffer.from(value));
      }
    } finally { await reader.cancel(); }
    const out = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    requireThat(out?.jsonrpc === '2.0' && out.id === id && Object.hasOwn(out, 'result') && !Object.hasOwn(out, 'error'),
      'RPC_RESPONSE_' + (out?.error?.code ?? 'ENVELOPE'));
    return out.result;
  }
}
export function decodeAccount(value, owner, executable, length) {
  requireThat(value?.owner === owner && value.executable === executable &&
    Array.isArray(value.data) && value.data.length === 2 && value.data[1] === 'base64' &&
    typeof value.data[0] === 'string', 'ACCOUNT_ENVELOPE');
  const bytes = Buffer.from(value.data[0], 'base64');
  requireThat(bytes.toString('base64') === value.data[0] &&
    (length === undefined || bytes.length === length), 'ACCOUNT_BYTES');
  return bytes;
}
// A mismatched response is discarded in full. Never combine Clock from one
// response with other accounts from another, and never accept persistent skew.
export async function readClockBoundAccounts(rpc, addresses, { commitment = 'finalized', pollMs = 200 } = {}) {
  const clockIndex = addresses.indexOf(CLOCK.toBase58());
  requireThat(clockIndex >= 0 && addresses.lastIndexOf(CLOCK.toBase58()) === clockIndex, 'CLOCK_REQUEST');
  for (let attempt = 1; attempt <= 3; attempt++) {
    const r = await rpc.call('getMultipleAccounts', [addresses, { encoding: 'base64', commitment }]);
    requireThat(Number.isSafeInteger(r?.context?.slot) && r.context.slot >= 0 &&
      Array.isArray(r.value) && r.value.length === addresses.length, 'CLOCK_RESPONSE');
    const clock = decodeAccount(r.value[clockIndex], SYSVAR, false, 40);
    if (clock.readBigUInt64LE(0) === BigInt(r.context.slot)) return { response: r, clock };
    console.warn('CLOCK_BANK_REJECTED ' + JSON.stringify({ attempt, context_slot: r.context.slot,
      clock_slot: clock.readBigUInt64LE(0).toString() }));
    if (attempt < 3) await sleep(pollMs);
  }
  throw new Error('CLOCK_BANK');
}
export async function readClock(rpc, commitment = 'finalized') {
  const { response: r, clock: raw } = await readClockBoundAccounts(rpc, [CLOCK.toBase58()], { commitment });
  return { slot: r.context.slot, now: raw.readBigInt64LE(32) };
}
export async function readBoundPolicy(rpc, binding) {
  requireThat(await rpc.call('getGenesisHash') === binding.genesisHash, 'GENESIS_MISMATCH');
  const programData = PublicKey.findProgramAddressSync([PROGRAM.toBuffer()], LOADER)[0];
  const { response: r } = await readClockBoundAccounts(rpc,
    [binding.policy, PROGRAM.toBase58(), programData.toBase58(), CLOCK.toBase58()]);
  requireThat(Number.isSafeInteger(r?.context?.slot) && r.value?.length === 4, 'POLICY_RESPONSE');
  const policy = decodeAccount(r.value[0], PROGRAM.toBase58(), false, 1065);
  const program = decodeAccount(r.value[1], LOADER.toBase58(), true, 36);
  const code = decodeAccount(r.value[2], LOADER.toBase58(), false);
  const clock = decodeAccount(r.value[3], SYSVAR, false, 40);
  requireThat(program.readUInt32LE(0) === 2 && program.subarray(4).equals(programData.toBuffer()), 'PROGRAM_POINTER');
  requireThat(code.length >= 45 + CODE_BYTES && code.length <= 2000000 && code.readUInt32LE(0) === 3 &&
    code[12] === 0 && hash(code.subarray(45, 45 + CODE_BYTES)) === CODE_SHA256 &&
    code.subarray(45 + CODE_BYTES).every(b => b === 0) && code.readBigUInt64LE(4) <= BigInt(r.context.slot), 'PROGRAM_CODE');
  requireThat(clock.readBigUInt64LE(0) === BigInt(r.context.slot), 'CLOCK_BANK');
  requireThat(policy.subarray(0, 8).equals(Buffer.from(hash(Buffer.from('account:LaunchPolicyV7')), 'hex').subarray(0, 8)), 'POLICY_DISCRIMINATOR');
  requireThat(policy.subarray(168, 200).toString('hex') === binding.identityHex &&
    policy.subarray(200, 232).toString('hex') === binding.specHashHex &&
    pda(Buffer.from('launch-v7-policy'), policy.subarray(168, 200)).toBase58() === binding.policy, 'POLICY_BINDING');
  for (const [name, offset] of [['creator', 8], ['mint', 40], ['founder', 72], ['treasury', 104], ['oracle', 832]]) {
    requireThat(new PublicKey(policy.subarray(offset, offset + 32)).toBase58() === binding[name], 'ACTOR_BINDING_' + name);
  }
  requireThat(await rpc.call('getGenesisHash') === binding.genesisHash, 'GENESIS_CHANGED');
  return { raw: policy, slot: r.context.slot, now: clock.readBigInt64LE(32),
    roles: [945, 1001].map(offset => ({ current: new PublicKey(policy.subarray(offset, offset + 32)).toBase58(),
      epoch: policy.readBigUInt64LE(offset + 32), sequence: policy.readBigUInt64LE(offset + 40),
      pending: policy.readBigUInt64LE(offset + 48) })) };
}
export function proposalInstruction({ payer, initiator, cosigner, successor, policy, role, recovery,
  nonce, epoch, validFrom, validUntil, predecessor }) {
  requireThat(role === 0 || role === 1, 'ROLE');
  requireThat(typeof recovery === 'boolean', 'RECOVERY_BOOL');
  integer(validFrom, true); integer(validUntil, true);
  requireThat(validFrom >= 0n && validUntil >= validFrom && validUntil - validFrom <= 300n &&
    validUntil <= (1n << 63n) - 1n - 10368000n, 'SUBMISSION_WINDOW');
  const policyKey = new PublicKey(policy), successorKey = new PublicKey(successor);
  const proposal = pda(Buffer.from('launch-v7-withdraw'), policyKey.toBuffer(), Buffer.from([role]), integer(nonce));
  const record = pda(Buffer.from('launch-v7-key'), policyKey.toBuffer(), successorKey.toBuffer());
  return new TransactionInstruction({ programId: PROGRAM, data: Buffer.concat([
    Buffer.from([24, 15, 80, 161, 146, 233, 1, 25]), Buffer.from([role, Number(recovery)]),
    integer(nonce), integer(epoch), integer(validFrom, true), integer(validUntil, true), new PublicKey(predecessor).toBuffer(),
  ]), keys: [[payer, true, true], [initiator, true, false], [cosigner, true, false], [successor, true, false],
    [policyKey, false, true], [proposal, false, true], [record, false, true], [SYSTEM, false, false]]
    .map(([key, isSigner, isWritable]) => ({ pubkey: new PublicKey(key), isSigner, isWritable })) });
}
export async function signInstructions(rpc, instructions, payer, signers) {
  const latest = await rpc.call('getLatestBlockhash', [{ commitment: 'finalized' }]);
  requireThat(Number.isSafeInteger(latest?.value?.lastValidBlockHeight), 'BLOCKHASH_RESPONSE');
  const tx = new Transaction({ feePayer: payer, recentBlockhash: latest.value.blockhash }).add(...instructions);
  const unique = [...new Map(signers.map(s => [s.publicKey.toBase58(), s])).values()];
  tx.sign(...unique);
  const bytes = tx.serialize(); // SDK enforces complete signatures and the legacy packet limit.
  requireThat(bytes.length <= PACKET_LIMIT && tx.verifySignatures(), 'SIGNED_PACKET');
  return Object.freeze({ schema: 'K4V-E11-SIGNED-LOCAL-v1', bytes: bytes.toString('base64'),
    messageHash: hash(tx.serializeMessage()), signature: base58(tx.signature),
    blockhash: tx.recentBlockhash, lastValidBlockHeight: latest.value.lastValidBlockHeight,
    byteLength: bytes.length });
}
export function inspectEnvelope(envelope, reviewedMessageHash) {
  requireThat(envelope?.schema === 'K4V-E11-SIGNED-LOCAL-v1' && typeof reviewedMessageHash === 'string' &&
    /^[0-9a-f]{64}$/.test(reviewedMessageHash), 'ENVELOPE_SCHEMA');
  const bytes = Buffer.from(envelope.bytes, 'base64');
  requireThat(bytes.toString('base64') === envelope.bytes && bytes.length <= PACKET_LIMIT && bytes.length === envelope.byteLength,
    'ENVELOPE_BYTES');
  const tx = Transaction.from(bytes);
  requireThat(hash(tx.serializeMessage()) === reviewedMessageHash && envelope.messageHash === reviewedMessageHash,
    'MESSAGE_CHANGED');
  requireThat(tx.verifySignatures() && base58(tx.signature) === envelope.signature, 'SIGNATURE_INVALID');
  requireThat(tx.recentBlockhash === envelope.blockhash &&
    Number.isSafeInteger(envelope.lastValidBlockHeight) && envelope.lastValidBlockHeight >= 0, 'BLOCKHASH_BINDING');
  return tx;
}
async function signatureStatus(rpc, signature) {
  const r = await rpc.call('getSignatureStatuses', [[signature], { searchTransactionHistory: true }]);
  requireThat(Array.isArray(r?.value) && r.value.length === 1, 'STATUS_RESPONSE');
  const s = r.value[0];
  if (s === null) return null;
  requireThat(Number.isSafeInteger(s?.slot) && Object.hasOwn(s, 'err') &&
    ['processed', 'confirmed', 'finalized'].includes(s.confirmationStatus), 'STATUS_VALUE');
  return s;
}
const attempted = new WeakMap();
function resultForStatus(s, signature) {
  if (s?.confirmationStatus !== 'finalized') return null;
  return { status: s.err === null ? 'FINALIZED' : 'ONCHAIN_FAILED', signature, slot: s.slot, error: s.err };
}
export async function submitSigned(rpc, envelope, reviewedMessageHash, { guard = async () => null, timeoutMs = 90000, pollMs = 300 } = {}) {
  inspectEnvelope(envelope, reviewedMessageHash);
  let signatures = attempted.get(rpc);
  if (!signatures) { signatures = new Set(); attempted.set(rpc, signatures); }
  let possiblySent = signatures.has(envelope.signature);
  const old = await signatureStatus(rpc, envelope.signature), existing = resultForStatus(old, envelope.signature);
  if (existing) return existing;
  if (old) possiblySent = true;
  if (!possiblySent) {
    const reason = await guard();
    if (reason) return { status: 'REBUILD_AND_RESIGN', reason, signature: envelope.signature };
    const validity = await rpc.call('isBlockhashValid', [envelope.blockhash, { commitment: 'finalized' }]);
    const height = await rpc.call('getBlockHeight', [{ commitment: 'finalized' }]);
    requireThat(typeof validity?.value === 'boolean' && Number.isSafeInteger(height), 'BLOCKHASH_STATUS');
    if (!validity.value || height > envelope.lastValidBlockHeight) {
      return { status: 'REBUILD_AND_RESIGN', reason: 'BLOCKHASH_EXPIRED', signature: envelope.signature };
    }
    const sim = await rpc.call('simulateTransaction', [envelope.bytes, {
      encoding: 'base64', commitment: 'finalized', sigVerify: true, replaceRecentBlockhash: false,
    }]);
    requireThat(sim?.value && Object.hasOwn(sim.value, 'err'), 'SIMULATION_RESPONSE');
    if (sim.value.err !== null) return { status: 'SIMULATION_REJECTED', signature: envelope.signature, error: sim.value.err, logs: sim.value.logs };
    // Recheck the semantic window after simulation. The on-chain program arbitrates races.
    const lastReason = await guard();
    if (lastReason) return { status: 'REBUILD_AND_RESIGN', reason: lastReason, signature: envelope.signature };
    signatures.add(envelope.signature);
    try {
      const sent = await rpc.call('sendTransaction', [envelope.bytes, {
        encoding: 'base64', skipPreflight: false, preflightCommitment: 'finalized', maxRetries: 0,
      }]);
      requireThat(sent === envelope.signature, 'SEND_SIGNATURE_MISMATCH');
    } catch {
      // A lost or malformed send response cannot establish failure. Reconcile our own signature.
    }
  }
  const deadline = Date.now() + timeoutMs;
  do {
    try {
      const s = await signatureStatus(rpc, envelope.signature), outcome = resultForStatus(s, envelope.signature);
      if (outcome) return outcome;
    } catch { /* A failed poll is not a successful transaction. */ }
    if (Date.now() >= deadline) break;
    await sleep(pollMs);
  } while (Date.now() <= deadline);
  return { status: 'UNKNOWN', signature: envelope.signature, reason: 'CONFIRMATION_UNRESOLVED_DO_NOT_RESIGN' };
}
export async function prepareWithdrawal(rpc, binding, intent, signers, windowSeconds = 300n) {
  const pin = Object.freeze({ ...binding }), choice = Object.freeze({ ...intent });
  integer(windowSeconds);
  requireThat(windowSeconds <= 300n, 'SUBMISSION_WINDOW');
  requireThat(choice.role === 0 || choice.role === 1, 'ROLE');
  const state = await readBoundPolicy(rpc, pin), role = state.roles[choice.role];
  requireThat(state.raw[724] !== 3 && role.pending === 0n, 'POLICY_NOT_AVAILABLE');
  const fields = Object.freeze({ ...choice, policy: pin.policy, nonce: role.sequence + 1n, epoch: role.epoch,
    predecessor: role.current, validFrom: state.now, validUntil: state.now + windowSeconds });
  const instruction = proposalInstruction(fields);
  const envelope = await signInstructions(rpc, [instruction], new PublicKey(choice.payer), signers);
  const guard = async () => {
    const current = await readBoundPolicy(rpc, pin), r = current.roles[choice.role];
    if (current.raw[724] === 3 || r.current !== fields.predecessor || r.epoch !== fields.epoch ||
      r.sequence + 1n !== fields.nonce || r.pending !== 0n) return 'POLICY_CHANGED';
    if (current.now < fields.validFrom || current.now > fields.validUntil) return 'SUBMISSION_WINDOW_CLOSED';
    return null;
  };
  return Object.freeze({ envelope, fields, guard, reviewedMessageHash: envelope.messageHash });
}
