// Actual local Agave RPC. Program bytes start in genesis; a signed loader transaction revokes its temporary authority.
// Mint, tokens, policy, deposits and proposals are created by signed transactions.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, createWriteStream, mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { Keypair, PublicKey, SystemProgram, TransactionInstruction, ComputeBudgetProgram } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, MINT_SIZE, ACCOUNT_SIZE, AuthorityType, createInitializeMint2Instruction,
  createInitializeAccount3Instruction, createMintToCheckedInstruction, createSetAuthorityInstruction } from '@solana/spl-token';
import { LocalRpc, PROGRAM, LOADER, CODE_SHA256, CODE_BYTES, CLOCK, SYSTEM, decodeAccount, pda, integer, hash, sleep, readClock, readClockBoundAccounts, readBoundPolicy,
  signInstructions, inspectEnvelope, submitSigned, prepareWithdrawal } from '../clients/launch_v7_local_client.mjs';
import { fixture, instruction, UNIT, SUPPLY } from './e11b_fixtures.mjs';
import { bootstrap, wireReceipt } from '../clients/launch_v7_bootstrap.mjs';
import { trackLocalChild, stopLocalChild } from './e11_process.mjs';
import { finished } from 'node:stream/promises';

const out = resolve('target/e11b/agave');
mkdirSync(out, { recursive: true });
const ledger = mkdtempSync(join(tmpdir(), 'k4v-e11b-agave-'));
const log = createWriteStream(join(out, 'validator.log'));
const loaderAuthority = Keypair.generate();
const validator = spawn('solana-test-validator', ['--reset', '--quiet', '--ledger', ledger,
  '--rpc-port', '19599', '--faucet-port', '19699', '--bind-address', '127.0.0.1',
  '--dynamic-port-range', '19700-19800', '--upgradeable-program', PROGRAM.toBase58(),
  resolve('target/v7-test/launch_vault_v7.so'), loaderAuthority.publicKey.toBase58()], { stdio: ['ignore', 'pipe', 'pipe'] });
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
  const wire = { receipt: wireReceipt() };
  save('wire-budget', wire.receipt);
  const f = fixture({ solo: false });
  const feePayer = Keypair.generate();
  assert.equal(new Set([f.creator, f.founder, f.treasury, ...f.recovery].map(k => k.publicKey.toBase58())).size, 6);
  await airdrop(feePayer.publicKey, 1000000000);
  await airdrop(f.creator.publicKey, 30000000000);
  const roleKeys = [f.founder, f.treasury, f.oracle, ...f.recovery, ...f.backups.flat()];
  await send('fund-distinct-role-fixture', roleKeys.map(key => SystemProgram.transfer({
    fromPubkey: f.creator.publicKey, toPubkey: key.publicKey, lamports: 1000000 })), f.creator, [f.creator]);
  // Agave 3.1.10 genesis always stores Some(authority), even for CLI "none".
  // Seal through the actual loader instead of treating Some(default) as None.
  const programData = PublicKey.findProgramAddressSync([PROGRAM.toBuffer()], LOADER)[0];
  async function programAuthority(label, expected) {
    const response = await rpc.call('getMultipleAccounts', [[programData.toBase58()], { encoding: 'base64', commitment: 'finalized' }]);
    const bytes = decodeAccount(response.value[0], LOADER.toBase58(), false, 45 + CODE_BYTES);
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
  f.config.t0 = clock.now + 180n;
  const { boundLaunchIdentity } = await import('../probes/launch_v7_identity.mjs');
  f.identity = boundLaunchIdentity({ program: PROGRAM.toBuffer(), creator: f.creator.publicKey.toBuffer(),
    mint: f.mint.publicKey.toBuffer(), founder: f.founder.publicKey.toBuffer(), treasury: f.treasury.publicKey.toBuffer(),
    oracle: f.oracle.publicKey.toBuffer(), specHash: f.specHash, config: f.config });
  f.policy = pda(Buffer.from('launch-v7-policy'), f.identity);
  const binding = Object.freeze({ genesisHash, policy: f.policy.toBase58(), identityHex: f.identity.toString('hex'),
    specHashHex: f.specHash.toString('hex'), creator: f.creator.publicKey.toBase58(), mint: f.mint.publicKey.toBase58(),
    founder: f.founder.publicKey.toBase58(), treasury: f.treasury.publicKey.toBase58(), oracle: f.oracle.publicKey.toBase58() });
  const setup = bootstrap(f);
  await send('prepare-immutable-config', [setup.prepare], f.creator, [f.creator]);
  const { response: prepResponse, clock: prepClock } = await readClockBoundAccounts(rpc,
    [setup.preparation.toBase58(), f.mint.publicKey.toBase58(), CLOCK.toBase58()]);
  const rawAccounts = {};
  for (const [i, name, address] of [[0, 'preparation', setup.preparation], [1, 'mint', f.mint.publicKey]]) {
    const a = prepResponse.value[i];
    rawAccounts[name] = { address: address.toBase58(), owner: a.owner, executable: a.executable,
      lamports: String(a.lamports), data_hex: Buffer.from(a.data[0], 'base64').toString('hex') };
  }
  save('preparation-input', { snapshot: { accounts: rawAccounts, slot: prepResponse.context.slot,
    now: prepClock.readBigInt64LE(32).toString() }, expected: binding });
  const prepCheck = spawnSync('python3', ['src/e11b_preparation_verifier.py', join(out, 'preparation-input.json')],
    { encoding: 'utf8', env: { ...process.env, PYTHONPATH: 'src' } });
  assert.equal(prepCheck.status, 0, prepCheck.stdout + prepCheck.stderr);
  const prepVerified = JSON.parse(prepCheck.stdout); assert.equal(prepVerified.valid, true);
  save('preparation-verification', prepVerified);
  await send('six-role-open-separate-payer', [setup.open], feePayer, [feePayer, ...setup.actors]);
  const replay = await signInstructions(rpc, [ComputeBudgetProgram.setComputeUnitLimit({ units: 400000 }), setup.open],
    feePayer.publicKey, [feePayer, ...setup.actors]);
  const replayResult = await submitSigned(rpc, replay, replay.messageHash);
  assert.equal(replayResult.status, 'SIMULATION_REJECTED');
  refusal.push({ label: 'bootstrap-replay', result: replayResult });
  await readBoundPolicy(rpc, binding);
  const vault = role => pda(Buffer.from('launch-v7-vault'), f.policy.toBuffer(), Buffer.from([role]));
  const vaultToken = role => pda(Buffer.from('launch-v7-token'), vault(role).toBuffer());
  for (const role of [0, 1]) {
    await send('deposit-' + role, [instruction('deposit', { creator: f.creator.publicKey,
      depositor: f.creator.publicKey, authority: (role === 0 ? f.founder : f.treasury).publicKey, policy: f.policy, mint: f.mint.publicKey,
      source: f.source.publicKey, vault: vault(role), vault_token: vaultToken(role),
      token_program: TOKEN_PROGRAM_ID, system_program: SYSTEM },
    [Buffer.from([role]), integer(role === 0 ? f.config.founder_amount : f.config.treasury_amount)])], f.creator, [f.creator, role === 0 ? f.founder : f.treasury]);
  }
  await send('arm', [instruction('arm', { creator: f.creator.publicKey, policy: f.policy })], f.creator, [f.creator]);
  await until(async () => (await readClock(rpc)).now >= f.config.t0, 200000, 'T0-runtime-clock');
  await send('activate', [instruction('activate', { policy: f.policy })], f.creator, [f.creator]);
  const manifest = { schema: 'K4V-V7-RPC-REVIEW-MANIFEST-v1', expected: {
    genesis_hash: genesisHash, program_id: PROGRAM.toBase58(), policy: binding.policy, identity_sha256: binding.identityHex,
    spec_sha256: binding.specHashHex, mint: binding.mint, creator: binding.creator, founder: binding.founder,
    treasury: binding.treasury, initial_oracle: binding.oracle },
    external_accounts: { source: f.source.publicKey.toBase58(), founder_destination_0: f.founderOut.publicKey.toBase58(),
      treasury_destination: f.treasuryOut.publicKey.toBase58() }, approval_periods: [] };
  save('manifest', manifest);
  async function observe(label) {
    const path = join(out, 'observation-' + label + '.json');
    let result;
    for (let attempt = 1; attempt <= 3; attempt++) {
      result = spawnSync('python3', ['src/launch_v7_rpc_exporter.py', '--rpc-url', rpc.endpoint,
        '--manifest', join(out, 'manifest.json'), '--output', path], { encoding: 'utf8', env: { ...process.env, PYTHONPATH: 'src' } });
      if (result.status === 0) break;
      let rejected; try { rejected = JSON.parse(result.stdout); } catch { /* Other failures are not retried. */ }
      if (rejected?.error !== 'RPC_CLOCK_BANK_MISMATCH' || attempt === 3) break;
      console.warn('RPC_EXPORT_CLOCK_RESTART ' + JSON.stringify({ label, attempt }));
      await sleep(200); // Restart the entire exporter; no discarded bytes are reused.
    }
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const decoded = JSON.parse(readFileSync(path, 'utf8'));
    assert.equal(decoded.verification.valid, true);
    observations.push({ label, slot: decoded.snapshot.slot, now: decoded.snapshot.now,
      account_count: decoded.provenance.final_account_count, sha256: hash(readFileSync(path)) });
    console.log('RPC_VERIFIED ' + label + ' slot=' + decoded.snapshot.slot);
    return decoded;
  }
  await observe('active');
  const reportAt = (await readClock(rpc)).now;
  await send('report-capacity', [instruction('report_capacity', { oracle: f.oracle.publicKey, policy: f.policy },
    [integer(f.config.shared_hard_cap), integer(reportAt, true), integer(1n), integer(0n)])], f.creator, [f.creator, f.oracle]);
  const release = instruction('release', { authority: f.founder.publicKey, policy: f.policy, vault: vault(0),
    mint: f.mint.publicKey, vault_token: vaultToken(0), destination: f.founderOut.publicKey, token_program: TOKEN_PROGRAM_ID },
  [integer(UNIT), integer(0n)]);
  const early = await signInstructions(rpc, [release], f.creator.publicKey, [f.creator, f.founder]);
  const earlyResult = await submitSigned(rpc, early, early.messageHash);
  assert.equal(earlyResult.status, 'SIMULATION_REJECTED');
  assert(earlyResult.logs.some(l => l.includes('CliffActive')));
  refusal.push({ label: 'founder-pre-cliff', result: earlyResult });

  const successors = [Keypair.generate(), Keypair.generate()];
  for (const successor of successors) {
    await send('prepare-successor', [instruction('prepare_withdrawal_key', { payer: f.creator.publicKey, policy: f.policy,
      record: pda(Buffer.from('launch-v7-key'), f.policy.toBuffer(), successor.publicKey.toBuffer()), system_program: SYSTEM },
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
  const pending = await observe('recovery-pending');
  const proposalRaw = Buffer.from(pending.snapshot.accounts.withdrawal_0_1.data_hex, 'hex');
  const createdAt = proposalRaw.readBigInt64LE(138), executeAfter = proposalRaw.readBigInt64LE(146);
  assert(createdAt >= delayed.fields.validFrom + 3n && createdAt <= delayed.fields.validUntil);
  assert.equal(executeAfter - createdAt, 7776000n);
  const racedResult = await submitSigned(rpc, raced.envelope, raced.reviewedMessageHash, { guard: raced.guard });
  assert.equal(racedResult.reason, 'POLICY_CHANGED');
  refusal.push({ label: 'nonce-race', result: racedResult });
  const proposalKey = pda(Buffer.from('launch-v7-withdraw'), f.policy.toBuffer(), Buffer.from([0]), integer(1n));
  const recordKey = pda(Buffer.from('launch-v7-key'), f.policy.toBuffer(), successors[0].publicKey.toBuffer());
  const execute = instruction('execute_withdrawal', { policy: f.policy, proposal: proposalKey, successor_record: recordKey });
  const premature = await signInstructions(rpc, [execute], f.creator.publicKey, [f.creator]);
  const prematureResult = await submitSigned(rpc, premature, premature.messageHash);
  assert.equal(prematureResult.status, 'SIMULATION_REJECTED');
  assert(prematureResult.logs.some(l => l.includes('WithdrawalWindow')));
  refusal.push({ label: 'recovery-before-90-days', result: prematureResult });
  await send('successor-cancels-recovery', [instruction('cancel_withdrawal', { initiator: successors[0].publicKey,
    cosigner: successors[0].publicKey, policy: f.policy, proposal: proposalKey, successor_record: recordKey })],
  f.creator, [f.creator, successors[0]]);
  const cancelled = await observe('cancelled');
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
  const final = await observe('after-rejections');
  for (const name of ['policy', 'source', 'mint', 'preparation', 'founder_vault', 'treasury_vault',
    'founder_token', 'treasury_token', 'founder_destination_0', 'treasury_destination']) {
    assert.equal(final.snapshot.accounts[name].data_hex, cancelled.snapshot.accounts[name].data_hex, 'REJECTION_MUTATED_' + name);
  }
  const receipt = { schema: 'K4V-E11B-AGAVE-RECEIPT-v1', valid: true, cluster: 'local-agave',
    version, genesis_hash: genesisHash, program: PROGRAM.toBase58(), program_origin: 'genesis-SBF-then-signed-loader-authority-revocation',
    authority_revocation_signature: sealing.signature,
    program_sha256: hash(readFileSync('target/v7-test/launch_vault_v7.so')), client_private_keys_serialized: false,
    policy_token_account_injection: false, clock_override: false, public_chain_transactions: 0,
    bootstrap: 'six-distinct-role-keys-plus-separate-fee-payer', independent_human_roles: false, preparation_verified: prepVerified,
    wire: wire.receipt, finalized_client_transactions: receipts.length, raw_rpc_checkpoints: observations,
    actual_admission_delay_seconds: (createdAt - delayed.fields.validFrom).toString(),
    full_notice_seconds: (executeAfter - createdAt).toString(), expected_refusals: refusal,
    long_duration_recovery_execution_verified: false, production_ready: false, independent_human_audit: false };
  save('signed-transactions', receipts); save('receipt', receipt);
  console.log('E11B_RESULT ' + JSON.stringify(receipt));
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
