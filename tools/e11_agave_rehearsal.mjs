// Actual local Agave RPC. Program bytes start in genesis; a signed loader transaction revokes its temporary authority.
// Mint, tokens, policy, deposits and proposals are created by signed transactions.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, createWriteStream, mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { Keypair, PublicKey, SystemProgram, TransactionInstruction } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, MINT_SIZE, ACCOUNT_SIZE, AuthorityType, createInitializeMint2Instruction,
  createInitializeAccount3Instruction, createMintToCheckedInstruction, createSetAuthorityInstruction } from '@solana/spl-token';
import { LocalRpc, PROGRAM, LOADER, CODE_SHA256, SYSTEM, decodeAccount, pda, integer, hash, sleep, readClock, readBoundPolicy,
  signInstructions, inspectEnvelope, submitSigned, prepareWithdrawal } from '../clients/launch_v6_local_client.mjs';
import { fixture, instruction, openInstruction, wireSizes, UNIT, SUPPLY } from './e11_fixtures.mjs';
import { trackLocalChild, stopLocalChild } from './e11_process.mjs';
import { finished } from 'node:stream/promises';

const out = resolve('target/e11');
mkdirSync(out, { recursive: true });
const ledger = mkdtempSync(join(tmpdir(), 'k4v-e11-agave-'));
const log = createWriteStream(join(out, 'validator.log'));
const loaderAuthority = Keypair.generate();
const validator = spawn('solana-test-validator', ['--reset', '--quiet', '--ledger', ledger,
  '--rpc-port', '19599', '--faucet-port', '19699', '--bind-address', '127.0.0.1',
  '--dynamic-port-range', '19700-19800', '--upgradeable-program', PROGRAM.toBase58(),
  resolve('target/v6-test/launch_vault_v6.so'), loaderAuthority.publicKey.toBase58()], { stdio: ['ignore', 'pipe', 'pipe'] });
const trackedValidator = trackLocalChild(validator);
validator.stdout.pipe(log, { end: false }); validator.stderr.pipe(log, { end: false });
const rpc = new LocalRpc('http://127.0.0.1:19599');
const receipts = [], observations = [], refusal = [];
const save = (name, value) => writeFileSync(join(out, name + '.json'), JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v, 2) + '\n');
async function until(check, timeout, label) {
  const start = Date.now(); let last = 0;
  while (Date.now() - start < timeout) {
    if (trackedValidator.error || validator.exitCode !== null || validator.signalCode !== null)
      throw trackedValidator.error ?? new Error('VALIDATOR_EXITED');
    if (await check()) return;
    if (Date.now() - last >= 10000) { console.log('WAIT ' + label); last = Date.now(); }
    await sleep(500);
  }
  throw new Error('WAIT_TIMEOUT_' + label);
}
async function airdrop(key, amount) {
  const signature = await rpc.call('requestAirdrop', [key.toBase58(), amount]);
  await until(async () => {
    const { value } = await rpc.call('getSignatureStatuses', [[signature]]);
    assert.equal(value[0]?.err ?? null, null);
    return value[0]?.confirmationStatus === 'finalized';
  }, 90000, 'airdrop-finality');
}
async function send(label, ixs, payer, signers) {
  const envelope = await signInstructions(rpc, ixs, payer.publicKey, signers);
  const result = await submitSigned(rpc, envelope, envelope.messageHash);
  assert.equal(result.status, 'FINALIZED', label + ':' + JSON.stringify(result));
  receipts.push({ label, ...envelope, result });
  console.log('FINALIZED ' + label + ' slot=' + result.slot + ' bytes=' + envelope.byteLength);
  return result;
}
try {
  await until(async () => {
    try { return await rpc.call('getHealth') === 'ok'; } catch { return false; }
  }, 90000, 'validator-health');
  const genesisHash = await rpc.call('getGenesisHash'), version = await rpc.call('getVersion');
  const wire = wireSizes();
  assert.deepEqual([wire.receipt.independent_legacy_bytes, wire.receipt.independent_best_v0_bytes], [1352, 1264]);
  let oversizedError;
  try { await rpc.call('sendTransaction', [wire.oversizedV0, { encoding: 'base64', skipPreflight: false }]); }
  catch (e) { oversizedError = e.message; }
  assert.equal(oversizedError, 'RPC_RESPONSE_-32602');
  wire.receipt.agave_oversized_packet_rejected = true;
  save('wire-budget', wire.receipt);
  console.log('WIRE_RESULT ' + JSON.stringify(wire.receipt));

  // Explicit single-operator fixture, not six independent bootstrap signers.
  const f = fixture({ solo: true });
  await airdrop(f.creator.publicKey, 30000000000);
  // Agave 3.1.10 genesis always stores Some(authority), even for CLI "none".
  // Seal through the actual loader instead of treating Some(default) as None.
  const programData = PublicKey.findProgramAddressSync([PROGRAM.toBuffer()], LOADER)[0];
  async function programAuthority(label, expected) {
    const response = await rpc.call('getMultipleAccounts', [[programData.toBase58()], { encoding: 'base64', commitment: 'finalized' }]);
    const bytes = decodeAccount(response.value[0], LOADER.toBase58(), false, 45 + 529584);
    assert.equal(bytes.readUInt32LE(0), 3);
    assert.equal(hash(bytes.subarray(45)), CODE_SHA256);
    assert.equal(bytes[12], expected === null ? 0 : 1);
    if (expected !== null) assert(bytes.subarray(13, 45).equals(expected.toBuffer()));
    const record = { label, slot: response.context.slot, option_tag: bytes[12],
      authority: expected?.toBase58() ?? null, sha256: hash(bytes.subarray(45)) };
    save('program-' + label, record);
    console.log('PROGRAM_AUTHORITY ' + JSON.stringify(record));
    return record;
  }
  await programAuthority('genesis', loaderAuthority.publicKey);
  const loaderInstruction = new TransactionInstruction({ programId: LOADER, data: Buffer.from([4, 0, 0, 0]),
    keys: [{ pubkey: programData, isSigner: false, isWritable: true },
      { pubkey: loaderAuthority.publicKey, isSigner: true, isWritable: false }] });
  const sealing = await send('revoke-genesis-upgrade-authority', [loaderInstruction], f.creator, [f.creator, loaderAuthority]);
  await programAuthority('sealed', null);
  const mintRent = await rpc.call('getMinimumBalanceForRentExemption', [MINT_SIZE]);
  const tokenRent = await rpc.call('getMinimumBalanceForRentExemption', [ACCOUNT_SIZE]);
  const makeAccount = (key, space, lamports) => SystemProgram.createAccount({ fromPubkey: f.creator.publicKey,
    newAccountPubkey: key.publicKey, space, lamports, programId: TOKEN_PROGRAM_ID });
  await send('mint-source-supply-and-revoke', [
    makeAccount(f.mint, MINT_SIZE, mintRent),
    createInitializeMint2Instruction(f.mint.publicKey, 9, f.creator.publicKey, null),
    makeAccount(f.source, ACCOUNT_SIZE, tokenRent),
    createInitializeAccount3Instruction(f.source.publicKey, f.mint.publicKey, f.creator.publicKey),
    createMintToCheckedInstruction(f.mint.publicKey, f.source.publicKey, f.creator.publicKey, SUPPLY, 9),
    createSetAuthorityInstruction(f.mint.publicKey, f.creator.publicKey, AuthorityType.MintTokens, null),
  ], f.creator, [f.creator, f.mint, f.source]);
  await send('destinations', [
    makeAccount(f.founderOut, ACCOUNT_SIZE, tokenRent),
    createInitializeAccount3Instruction(f.founderOut.publicKey, f.mint.publicKey, f.founder.publicKey),
    makeAccount(f.treasuryOut, ACCOUNT_SIZE, tokenRent),
    createInitializeAccount3Instruction(f.treasuryOut.publicKey, f.mint.publicKey, f.recipient.publicKey),
  ], f.creator, [f.creator, f.founderOut, f.treasuryOut]);
  const clock = await readClock(rpc);
  f.config.t0 = clock.now + 150n;
  const { boundLaunchIdentity } = await import('../probes/launch_v6_identity.mjs');
  f.identity = boundLaunchIdentity({ program: PROGRAM.toBuffer(), creator: f.creator.publicKey.toBuffer(),
    mint: f.mint.publicKey.toBuffer(), founder: f.founder.publicKey.toBuffer(), treasury: f.treasury.publicKey.toBuffer(),
    oracle: f.oracle.publicKey.toBuffer(), specHash: f.specHash, config: f.config });
  f.policy = pda(Buffer.from('launch-v6-policy'), f.identity);
  const binding = Object.freeze({ genesisHash, policy: f.policy.toBase58(), identityHex: f.identity.toString('hex'),
    specHashHex: f.specHash.toString('hex'), creator: f.creator.publicKey.toBase58(), mint: f.mint.publicKey.toBase58(),
    founder: f.founder.publicKey.toBase58(), treasury: f.treasury.publicKey.toBase58(), oracle: f.oracle.publicKey.toBase58() });
  await send('solo-open-policy', [openInstruction(f)], f.creator, [f.creator, ...f.recovery]);
  await readBoundPolicy(rpc, binding);
  const vault = role => pda(Buffer.from('launch-v6-vault'), f.policy.toBuffer(), Buffer.from([role]));
  const vaultToken = role => pda(Buffer.from('launch-v6-token'), vault(role).toBuffer());
  for (const role of [0, 1]) {
    await send('deposit-' + role, [instruction('deposit', { creator: f.creator.publicKey,
      depositor: f.creator.publicKey, authority: f.creator.publicKey, policy: f.policy, mint: f.mint.publicKey,
      source: f.source.publicKey, vault: vault(role), vault_token: vaultToken(role),
      token_program: TOKEN_PROGRAM_ID, system_program: SYSTEM },
    [Buffer.from([role]), integer(role === 0 ? f.config.founder_amount : f.config.treasury_amount)])], f.creator, [f.creator]);
  }
  await send('arm', [instruction('arm', { creator: f.creator.publicKey, policy: f.policy })], f.creator, [f.creator]);
  await until(async () => (await readClock(rpc)).now >= f.config.t0, 200000, 'T0-runtime-clock');
  await send('activate', [instruction('activate', { policy: f.policy })], f.creator, [f.creator]);
  const manifest = { schema: 'K4V-V6-RPC-REVIEW-MANIFEST-v1', expected: {
    genesis_hash: genesisHash, program_id: PROGRAM.toBase58(), policy: binding.policy, identity_sha256: binding.identityHex,
    spec_sha256: binding.specHashHex, mint: binding.mint, creator: binding.creator, founder: binding.founder,
    treasury: binding.treasury, initial_oracle: binding.oracle },
    external_accounts: { source: f.source.publicKey.toBase58(), founder_destination_0: f.founderOut.publicKey.toBase58(),
      treasury_destination: f.treasuryOut.publicKey.toBase58() }, approval_periods: [] };
  save('manifest', manifest);
  function observe(label) {
    const path = join(out, 'observation-' + label + '.json');
    const result = spawnSync('python3', ['src/launch_v6_rpc_exporter.py', '--rpc-url', rpc.endpoint,
      '--manifest', join(out, 'manifest.json'), '--output', path], { encoding: 'utf8', env: { ...process.env, PYTHONPATH: 'src' } });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const decoded = JSON.parse(readFileSync(path, 'utf8'));
    assert.equal(decoded.verification.valid, true);
    observations.push({ label, slot: decoded.snapshot.slot, now: decoded.snapshot.now,
      account_count: decoded.provenance.final_account_count, sha256: hash(readFileSync(path)) });
    console.log('RPC_VERIFIED ' + label + ' slot=' + decoded.snapshot.slot);
    return decoded;
  }
  observe('active');
  const reportAt = (await readClock(rpc)).now;
  await send('report-capacity', [instruction('report_capacity', { oracle: f.oracle.publicKey, policy: f.policy },
    [integer(f.config.shared_hard_cap), integer(reportAt, true), integer(1n), integer(0n)])], f.creator, [f.creator, f.oracle]);
  const release = instruction('release', { authority: f.creator.publicKey, policy: f.policy, vault: vault(0),
    mint: f.mint.publicKey, vault_token: vaultToken(0), destination: f.founderOut.publicKey, token_program: TOKEN_PROGRAM_ID },
  [integer(UNIT), integer(0n)]);
  const early = await signInstructions(rpc, [release], f.creator.publicKey, [f.creator]);
  const earlyResult = await submitSigned(rpc, early, early.messageHash);
  assert.equal(earlyResult.status, 'SIMULATION_REJECTED');
  assert(earlyResult.logs.some(l => l.includes('CliffActive')));
  refusal.push({ label: 'founder-pre-cliff', result: earlyResult });

  const successors = [Keypair.generate(), Keypair.generate()];
  for (const successor of successors) {
    await send('prepare-successor', [instruction('prepare_withdrawal_key', { payer: f.creator.publicKey, policy: f.policy,
      record: pda(Buffer.from('launch-v6-key'), f.policy.toBuffer(), successor.publicKey.toBuffer()), system_program: SYSTEM },
    [successor.publicKey.toBuffer()])], f.creator, [f.creator]);
  }
  const intent = successor => ({ payer: binding.creator, initiator: f.backups[0][0].publicKey.toBase58(),
    cosigner: f.backups[0][1].publicKey.toBase58(), successor: successor.publicKey.toBase58(), role: 0, recovery: true });
  const signers = successor => [f.creator, f.backups[0][0], f.backups[0][1], successor];
  const delayed = await prepareWithdrawal(rpc, binding, intent(successors[0]), signers(successors[0]));
  const raced = await prepareWithdrawal(rpc, binding, intent(successors[1]), signers(successors[1]));
  const originalBytes = delayed.envelope.bytes;
  const corrupt = Buffer.from(originalBytes, 'base64'); corrupt[1] ^= 1;
  assert.throws(() => inspectEnvelope({ ...delayed.envelope, bytes: corrupt.toString('base64') }, delayed.reviewedMessageHash), /SIGNATURE/);
  await until(async () => (await readClock(rpc)).now >= delayed.fields.validFrom + 3n, 30000, 'signed-delay');
  const admission = await submitSigned(rpc, delayed.envelope, delayed.reviewedMessageHash, { guard: delayed.guard });
  assert.equal(admission.status, 'FINALIZED');
  assert.equal(delayed.envelope.bytes, originalBytes);
  receipts.push({ label: 'delayed-recovery', ...delayed.envelope, result: admission });
  const pending = observe('recovery-pending');
  const proposalRaw = Buffer.from(pending.snapshot.accounts.withdrawal_0_1.data_hex, 'hex');
  const createdAt = proposalRaw.readBigInt64LE(138), executeAfter = proposalRaw.readBigInt64LE(146);
  assert(createdAt >= delayed.fields.validFrom + 3n && createdAt <= delayed.fields.validUntil);
  assert.equal(executeAfter - createdAt, 7776000n);
  const racedResult = await submitSigned(rpc, raced.envelope, raced.reviewedMessageHash, { guard: raced.guard });
  assert.equal(racedResult.reason, 'POLICY_CHANGED');
  refusal.push({ label: 'nonce-race', result: racedResult });
  const proposalKey = pda(Buffer.from('launch-v6-withdraw'), f.policy.toBuffer(), Buffer.from([0]), integer(1n));
  const recordKey = pda(Buffer.from('launch-v6-key'), f.policy.toBuffer(), successors[0].publicKey.toBuffer());
  const execute = instruction('execute_withdrawal', { policy: f.policy, proposal: proposalKey, successor_record: recordKey });
  const premature = await signInstructions(rpc, [execute], f.creator.publicKey, [f.creator]);
  const prematureResult = await submitSigned(rpc, premature, premature.messageHash);
  assert.equal(prematureResult.status, 'SIMULATION_REJECTED');
  assert(prematureResult.logs.some(l => l.includes('WithdrawalWindow')));
  refusal.push({ label: 'recovery-before-90-days', result: prematureResult });
  await send('successor-cancels-recovery', [instruction('cancel_withdrawal', { initiator: successors[0].publicKey,
    cosigner: successors[0].publicKey, policy: f.policy, proposal: proposalKey, successor_record: recordKey })],
  f.creator, [f.creator, successors[0]]);
  const cancelled = observe('cancelled');
  assert.equal(Buffer.from(cancelled.snapshot.accounts.withdrawal_0_1.data_hex, 'hex')[162], 2);

  const short = await prepareWithdrawal(rpc, binding, intent(successors[1]), signers(successors[1]), 3n);
  await until(async () => (await readClock(rpc)).now > short.fields.validUntil, 30000, 'admission-window-expiry');
  assert.equal((await rpc.call('isBlockhashValid', [short.envelope.blockhash, { commitment: 'finalized' }])).value, true);
  const expiredWindow = await submitSigned(rpc, short.envelope, short.reviewedMessageHash, { guard: short.guard });
  assert.equal(expiredWindow.reason, 'SUBMISSION_WINDOW_CLOSED');
  const bypass = await rpc.call('simulateTransaction', [short.envelope.bytes, {
    encoding: 'base64', commitment: 'finalized', sigVerify: true, replaceRecentBlockhash: false }]);
  assert(bypass.value.err && bypass.value.logs.some(l => l.includes('SubmissionWindow')));
  refusal.push({ label: 'expired-window', result: expiredWindow, onchain_simulation_error: bypass.value.err });
  const bh = await signInstructions(rpc, [SystemProgram.transfer({ fromPubkey: f.creator.publicKey,
    toPubkey: f.recipient.publicKey, lamports: 1 })], f.creator.publicKey, [f.creator]);
  await until(async () => await rpc.call('getBlockHeight', [{ commitment: 'finalized' }]) > bh.lastValidBlockHeight,
    180000, 'natural-blockhash-expiry');
  const expiredBlockhash = await submitSigned(rpc, bh, bh.messageHash);
  assert.equal(expiredBlockhash.reason, 'BLOCKHASH_EXPIRED');
  refusal.push({ label: 'expired-blockhash', result: expiredBlockhash });
  const final = observe('after-rejections');
  for (const name of ['policy', 'source', 'mint', 'founder_vault', 'treasury_vault',
    'founder_token', 'treasury_token', 'founder_destination_0', 'treasury_destination']) {
    assert.equal(final.snapshot.accounts[name].data_hex, cancelled.snapshot.accounts[name].data_hex, 'REJECTION_MUTATED_' + name);
  }
  const receipt = { schema: 'K4V-E11A-AGAVE-RECEIPT-v1', valid: true, cluster: 'local-agave',
    version, genesis_hash: genesisHash, program: PROGRAM.toBase58(), program_origin: 'genesis-SBF-then-signed-loader-authority-revocation',
    authority_revocation_signature: sealing.signature,
    program_sha256: hash(readFileSync('target/v6-test/launch_vault_v6.so')), client_private_keys_serialized: false,
    policy_token_account_injection: false, clock_override: false, public_chain_transactions: 0,
    bootstrap: 'creator-founder-treasury-same-key-four-signatures', independent_bootstrap: 'BLOCKED_PACKET_SIZE',
    wire: wire.receipt, finalized_client_transactions: receipts.length, raw_rpc_checkpoints: observations,
    actual_admission_delay_seconds: (createdAt - delayed.fields.validFrom).toString(),
    full_notice_seconds: (executeAfter - createdAt).toString(), expected_refusals: refusal,
    long_duration_recovery_execution_verified: false, production_ready: false, independent_human_audit: false };
  save('signed-transactions', receipts); save('receipt', receipt);
  console.log('E11_RESULT ' + JSON.stringify(receipt));
} catch (error) {
  save('failure', { message: error.message, stack: error.stack, finalized_transactions: receipts.length });
  save('signed-transactions', receipts);
  console.error('E11_FAILURE', error.stack ?? error);
  throw error;
} finally {
  try { await stopLocalChild(validator, trackedValidator); }
  finally {
    log.end();
    await finished(log);
    rmSync(ledger, { recursive: true, force: true });
  }
}
