// Bounded TEST_ONLY node lifecycle. Keeps ledger and local test keys; never uses public RPC.
// Initialization is adapted from frozen E11B; no existing program or evidence bytes are changed.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, createWriteStream, readFileSync, existsSync, statSync, renameSync, openSync, fsyncSync, closeSync, rmdirSync } from 'node:fs';
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


const [command, flag, directory, ...extra] = process.argv.slice(2);
assert(['acceptance', 'resume'].includes(command) && flag === '--directory' && directory && !extra.length,
  'USAGE: node tools/soak_persistent.mjs acceptance|resume --directory NEW_OR_EXISTING_PRIVATE_PATH');
process.umask(0o077);
const root = resolve(directory);
if (command === 'acceptance') mkdirSync(root, { mode: 0o700 }); // Existing output is never reused.
let out = command === 'acceptance' ? join(root, 'run') : root;
if (command === 'acceptance') mkdirSync(out, { mode: 0o700 });
assert.equal(statSync(out).mode & 0o077, 0, 'PRIVATE_DIRECTORY_REQUIRED');
const locks = [];
function lockRun(path) { const lock = path + '.driver-lock'; mkdirSync(lock, { mode: 0o700 }); locks.push(lock); }
lockRun(out); // A stale lock fails closed after an interrupted controller.
const rpc = new LocalRpc('http://127.0.0.1:19599');
const receipts = [], observations = [], refusal = [], lifecycle = [];
let validator, trackedValidator, log, interrupted = false;
const json = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v, 2) + '\n';
function save(name, value) {
  const fd = openSync(join(out, name + '.json'), 'wx', 0o600);
  try { writeFileSync(fd, json(value)); fsyncSync(fd); } finally { closeSync(fd); }
}
function atomicState(value) {
  const path = join(out, 'state.json'), temp = path + '.next';
  const fd = openSync(temp, 'wx', 0o600);
  try { writeFileSync(fd, json(value)); fsyncSync(fd); } finally { closeSync(fd); }
  renameSync(temp, path);
  const dir = openSync(out, 'r'); try { fsyncSync(dir); } finally { closeSync(dir); }
}
async function until(check, timeout, label) {
  const start = Date.now(); let last = 0;
  while (Date.now() - start < timeout) {
    if (interrupted || trackedValidator?.error || validator?.exitCode !== null || validator?.signalCode !== null)
      throw trackedValidator?.error ?? new Error('VALIDATOR_STOPPED_OR_INTERRUPTED');
    if (await check()) return;
    if (Date.now() - last > 15000) { console.log('WAIT ' + label); last = Date.now(); }
    await sleep(500);
  }
  throw new Error('WAIT_TIMEOUT_' + label);
}
async function startNode(label, loaderAuthority = null) {
  assert(!validator, 'NODE_ALREADY_RUNNING');
  // An occupied endpoint is refused before spawn, including a different local project.
  let occupied = false;
  try { await rpc.call('getVersion'); occupied = true; } catch { /* health is checked after spawn too */ }
  assert(!occupied, 'RPC_PORT_ALREADY_IN_USE');
  const binary = spawnSync('solana-test-validator', ['--version'], { encoding: 'utf8' });
  assert.equal(binary.status, 0, binary.stderr); assert(binary.stdout.includes('3.1.10'), 'VALIDATOR_VERSION');
  const program = resolve('target/v7-test/launch_vault_v7.so');
  assert.equal(hash(readFileSync(program)), CODE_SHA256, 'PROGRAM_HASH');
  const ledger = join(out, 'ledger');
  if (loaderAuthority) assert(!existsSync(ledger), 'FRESH_LEDGER_REQUIRED');
  else assert(existsSync(join(ledger, 'genesis.bin')), 'EXISTING_GENESIS_REQUIRED');
  const args = ['--quiet', '--ledger', ledger, '--rpc-port', '19599', '--faucet-port', '19699',
    '--bind-address', '127.0.0.1', '--dynamic-port-range', '19700-19800',
    '--limit-ledger-size', '1000000000000']; // Default 10000 shreds prunes bootstrap history.
  if (loaderAuthority) args.push('--upgradeable-program', PROGRAM.toBase58(), program, loaderAuthority.publicKey.toBase58());
  save('argv-' + label, { executable_version: binary.stdout.trim(), args, reset: false, warp: false });
  log = createWriteStream(join(out, 'validator-' + label + '.log'), { flags: 'wx', mode: 0o600 });
  validator = spawn('solana-test-validator', args, { stdio: ['ignore', 'pipe', 'pipe'] });
  trackedValidator = trackLocalChild(validator);
  lifecycle.push({ event: 'START', label, pid: validator.pid, at: new Date().toISOString(), ledger });
  validator.stdout.pipe(log, { end: false }); validator.stderr.pipe(log, { end: false });
  await until(async () => { try { return await rpc.call('getHealth') === 'ok'; } catch { return false; } },
    120000, label + '-health');
}
async function stopNode(label) {
  if (!validator) return;
  const pid = validator.pid;
  const outcome = await stopLocalChild(validator, trackedValidator, 30000);
  log.end(); await finished(log);
  lifecycle.push({ event: 'STOP', label, pid, outcome, at: new Date().toISOString() });
  validator = null; trackedValidator = null; log = null;
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
  assert(!interrupted, 'INTERRUPTED');
  const envelope = await signInstructions(rpc, ixs, payer.publicKey, signers);
  // Persist the exact signed attempt BEFORE sending. A crash never implies it was not sent.
  save('attempt-' + label, envelope);
  const result = await submitSigned(rpc, envelope, envelope.messageHash);
  save('result-' + label, result);
  assert.equal(result.status, 'FINALIZED', label + ':' + JSON.stringify(result));
  receipts.push({ label, ...envelope, result });
  console.log('FINALIZED ' + label + ' slot=' + result.slot);
  return result;
}
function python(args, expected = 0) {
  const result = spawnSync('python3', args, { encoding: 'utf8', maxBuffer: 8000000,
    env: { ...process.env, PYTHONPATH: 'src', PYTHONDONTWRITEBYTECODE: '1' } });
  assert.equal(result.status, expected, result.stdout + result.stderr);
  return result;
}
async function initialize(loaderAuthority) {
  const genesisHash = await rpc.call('getGenesisHash'), version = await rpc.call('getVersion');
  const wire = { receipt: wireReceipt() };
  save('wire-budget', wire.receipt);
  const f = fixture({ solo: false });
  const feePayer = Keypair.generate();
  const keyNames = ['creator', 'founder', 'treasury', 'oracle', 'mint', 'source', 'founderOut', 'treasuryOut', 'recipient'];
  const keyBytes = Object.fromEntries(keyNames.map(name => [name, Array.from(f[name].secretKey)]));
  keyBytes.recovery = f.recovery.map(k => Array.from(k.secretKey));
  keyBytes.backups = f.backups.map(group => group.map(k => Array.from(k.secretKey)));
  keyBytes.feePayer = Array.from(feePayer.secretKey);
  keyBytes.initialLoaderAuthority = Array.from(loaderAuthority.secretKey);
  save('test-keys', keyBytes); // Retain recovery material before the first client transaction.
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
  const state = { schema: 'K4V-PRIVATE-PERSISTENT-TEST-v1', binding, manifest,
    config: f.config, report_sequence: '0', keys_sha256: hash(readFileSync(join(out, 'test-keys.json'))),
    program_sha256: CODE_SHA256, receipts };
  atomicState(state);
  return state;

}
async function observe(state, label, minimumSlot = 0) {
  await until(async () => (await readClock(rpc)).slot >= minimumSlot, 120000, 'replay-finality');
  await readBoundPolicy(rpc, state.binding);
  const path = join(out, 'observation-' + label + '.json');
  for (let attempt = 0; ; attempt++) {
    const result = spawnSync('python3', ['src/launch_v7_rpc_exporter.py', '--rpc-url', rpc.endpoint,
      '--manifest', join(out, 'manifest.json'), '--output', path],
    { encoding: 'utf8', env: { ...process.env, PYTHONPATH: 'src', PYTHONDONTWRITEBYTECODE: '1' } });
    if (result.status === 0) break;
    let error; try { error = JSON.parse(result.stdout).error; } catch { /* fail below */ }
    assert(error === 'RPC_CLOCK_BANK_MISMATCH' && attempt < 2, result.stdout + result.stderr);
    await sleep(300);
  }
  const snapshot = JSON.parse(readFileSync(path));
  assert.equal(snapshot.verification.valid, true);
  observations.push({ label, path: 'observation-' + label + '.json', slot: snapshot.snapshot.slot,
    now: snapshot.snapshot.now, sha256: hash(readFileSync(path)) });
  console.log('RPC_VERIFIED ' + label + ' slot=' + snapshot.snapshot.slot);
  return snapshot;
}
function sameApplication(before, after) {
  const a = before.snapshot.accounts, b = after.snapshot.accounts;
  assert.deepEqual(Object.keys(a).sort(), Object.keys(b).sort(), 'ACCOUNT_GRAPH_CHANGED');
  for (const name of Object.keys(a)) {
    if (name === 'clock') continue;
    assert.deepEqual(b[name], a[name], 'RESTART_CHANGED_ACCOUNT_' + name);
  }
  assert(BigInt(after.snapshot.now) >= BigInt(before.snapshot.now), 'BANK_TIME_REGRESSED');
  assert(BigInt(after.snapshot.slot) >= BigInt(before.snapshot.slot), 'BANK_SLOT_REGRESSED');
}
function loadState() {
  const state = JSON.parse(readFileSync(join(out, 'state.json')));
  assert.equal(state.schema, 'K4V-PRIVATE-PERSISTENT-TEST-v1');
  assert.equal(state.program_sha256, CODE_SHA256);
  assert.equal(statSync(join(out, 'test-keys.json')).mode & 0o077, 0, 'KEYS_NOT_PRIVATE');
  assert.equal(hash(readFileSync(join(out, 'test-keys.json'))), state.keys_sha256, 'KEYS_CHANGED');
  assert.deepEqual(JSON.parse(readFileSync(join(out, 'manifest.json'))), state.manifest);
  return state;
}
async function continueReport(state, label) {
  await readBoundPolicy(rpc, state.binding);
  const keyData = JSON.parse(readFileSync(join(out, 'test-keys.json')));
  const creator = Keypair.fromSecretKey(Uint8Array.from(keyData.creator));
  const oracle = Keypair.fromSecretKey(Uint8Array.from(keyData.oracle));
  assert.equal(creator.publicKey.toBase58(), state.binding.creator);
  assert.equal(oracle.publicKey.toBase58(), state.binding.oracle);
  const sequence = BigInt(state.report_sequence) + 1n;
  const reportAt = (await readClock(rpc)).now;
  await send(label, [instruction('report_capacity', { oracle: oracle.publicKey, policy: state.binding.policy },
    [integer(BigInt(state.config.shared_hard_cap)), integer(reportAt, true), integer(sequence), integer(0n)])],
  creator, [creator, oracle]);
  state.report_sequence = sequence.toString();
  state.receipts = [...state.receipts.filter(r => !receipts.some(n => n.signature === r.signature)), ...receipts];
  atomicState(state);
}
async function ledgerBound(method) {
  assert(['minimumLedgerSlot', 'getFirstAvailableBlock'].includes(method));
  const response = await fetch(rpc.endpoint, { method: 'POST', redirect: 'error',
    signal: AbortSignal.timeout(15000), headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params: [] }) });
  assert(response.ok);
  const value = await response.json();
  assert(value.jsonrpc === '2.0' && value.id === 1 && !value.error && Number.isSafeInteger(value.result));
  return value.result;
}
async function verifyHistory(state, label) {
  const bounds = { minimum_ledger_slot: await ledgerBound('minimumLedgerSlot'),
    first_available_block: await ledgerBound('getFirstAvailableBlock') };
  assert.equal(bounds.minimum_ledger_slot, 0, 'GENESIS_LEDGER_PRUNED');
  assert.equal(bounds.first_available_block, 0, 'GENESIS_BLOCK_PRUNED');
  const statuses = [];
  for (const receipt of state.receipts) {
    inspectEnvelope(receipt, receipt.messageHash);
    let value;
    await until(async () => {
      ({ value } = await rpc.call('getSignatureStatuses', [[receipt.signature], { searchTransactionHistory: true }]));
      if (value[0]) assert.equal(value[0].err, null);
      return value[0]?.confirmationStatus === 'finalized';
    }, 90000, 'historical-status-' + receipt.label);
    assert.equal(value[0].err, null);
    assert.equal(value[0].slot, receipt.result.slot, 'TRANSACTION_SLOT_CHANGED');
    statuses.push({ signature: receipt.signature, ...value[0] });
  }
  save('history-' + label, { bounds, statuses });
}
function observer(state, label) {
  const dir = join(out, 'journal');
  if (!existsSync(dir)) python(['tools/soak_observer.py', 'init', '--directory', dir,
    '--manifest', join(out, 'manifest.json'), '--origin', 'signed-bootstrap-declared', '--max-gap-seconds', '900']);
  // Prior head comes from the retained state; the backup binds this state too.
  const args = ['tools/soak_observer.py', 'record', '--directory', dir,
    '--manifest', join(out, 'manifest.json'), '--rpc-url', rpc.endpoint, '--samples', '1'];
  if (state.journal_head) args.push('--expect-head', state.journal_head);
  const result = python(args);
  const summary = JSON.parse(result.stdout.trim().split('\n').at(-1));
  state.journal_head = summary.head_sha256; atomicState(state);
  save('journal-' + label, summary);
  return summary;
}
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => {
  interrupted = true; if (validator) validator.kill('SIGTERM');
});
try {
  if (command === 'resume') {
    const state = loadState(), label = 'resume-' + (BigInt(state.report_sequence) + 1n);
    await startNode(label);
    await verifyHistory(state, label);
    await observe(state, label + '-before');
    await continueReport(state, label);
    const after = await observe(state, label + '-after');
    observer(state, label);
    await stopNode(label);
    save('lifecycle-' + label, lifecycle);
    console.log(json({ valid: true, scope: 'BOUNDED_EXISTING_LEDGER_RESUME', slot: after.snapshot.slot,
      report_sequence: state.report_sequence, validator_stopped: true }));
  } else {
    const loaderAuthority = Keypair.generate();
    await startNode('bootstrap', loaderAuthority);
    let state = await initialize(loaderAuthority);
    await continueReport(state, 'report-1');
    const beforeRestart = await observe(state, 'before-restart');
    observer(state, 'before-restart');
    // Prove the backup path refuses the very validator it will later back up.
    const refused = spawnSync('python3', ['tools/soak_backup.py', 'backup', '--source', out,
      '--destination', join(root, 'live-backup-must-not-exist')], { encoding: 'utf8' });
    assert.notEqual(refused.status, 0);
    assert(refused.stderr.includes('VALIDATOR_STILL_RUNNING'), refused.stdout + refused.stderr);
    assert(!existsSync(join(root, 'live-backup-must-not-exist')));
    await stopNode('before-restart');
    await startNode('same-ledger-restart');
    state = loadState();
    const afterRestart = await observe(state, 'after-restart', beforeRestart.snapshot.slot);
    sameApplication(beforeRestart, afterRestart);
    await verifyHistory(state, 'same-ledger');
    await continueReport(state, 'report-2');
    const beforeBackup = await observe(state, 'before-backup');
    observer(state, 'before-backup');
    await stopNode('before-backup');
    const backup = JSON.parse(python(['tools/soak_backup.py', 'backup', '--source', out,
      '--destination', join(root, 'backup')]).stdout);
    writeFileSync(join(root, 'backup-receipt.json'), json(backup), { flag: 'wx', mode: 0o600 });
    python(['tools/soak_backup.py', 'restore', '--source', join(root, 'backup'),
      '--destination', join(root, 'restored'), '--expect-head', backup.backup_sha256]);
    out = join(root, 'restored'); lockRun(out);
    state = loadState();
    await startNode('restored-ledger');
    const restored = await observe(state, 'after-restore', beforeBackup.snapshot.slot);
    sameApplication(beforeBackup, restored);
    await verifyHistory(state, 'restored');
    await continueReport(state, 'report-3');
    const final = await observe(state, 'after-restored-transaction');
    await verifyHistory(state, 'final');
    const journal = observer(state, 'after-restore');
    await stopNode('after-restore');
    save('lifecycle', lifecycle);
    // Only this explicitly selected evidence is public. Ledgers contain Agave's test keys too.
    const publicDir = join(root, 'public'); mkdirSync(publicDir, { mode: 0o700 });
    const { copyFileSync, cpSync } = await import('node:fs');
    for (const item of observations) copyFileSync(join(out, item.path), join(publicDir, item.path),
      (await import('node:fs')).constants.COPYFILE_EXCL);
    writeFileSync(join(publicDir, 'manifest.json'), json(state.manifest), { flag: 'wx' });
    writeFileSync(join(publicDir, 'signed-transactions.json'), json(state.receipts), { flag: 'wx' });
    for (const name of ['same-ledger', 'restored', 'final'])
      copyFileSync(join(out, 'history-' + name + '.json'), join(publicDir, 'history-' + name + '.json'),
        (await import('node:fs')).constants.COPYFILE_EXCL);
    cpSync(join(out, 'journal'), join(publicDir, 'journal'), { recursive: true, errorOnExist: true, force: false });
    const source = { commit: spawnSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout.trim(),
      tree: spawnSync('git', ['rev-parse', 'HEAD^{tree}'], { encoding: 'utf8' }).stdout.trim(),
      worktree_clean: spawnSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).stdout.trim() === '',
      runtime_sha256: Object.fromEntries(['tools/soak_persistent.mjs', 'tools/soak_backup.py'].map(path =>
        [path, hash(readFileSync(path))])) };
    const result = { schema: 'K4V-PERSISTENT-SOAK-ACCEPTANCE-v1', valid: true,
      scope: 'BOUNDED_SIGNED_BOOTSTRAP_NODE_RESTART_OFFLINE_BACKUP_RESTORE',
      genesis_hash: state.binding.genesisHash, program_sha256: CODE_SHA256,
      policy: state.binding.policy, finalized_client_transactions: state.receipts.length,
      observations, lifecycle, backup, journal, source, latest_slot: final.snapshot.slot,
      same_ledger_restart_verified: true, offline_backup_restore_verified: true,
      restored_signer_transaction_verified: true, live_backup_refused: true,
      full_run_retained_locally: true, test_keys_retained_private: true,
      validator_stopped: true, daemon_installed: false, public_chain_transactions: 0,
      clock_override: false, policy_token_account_injection: false, natural_90_180_day_soak: false,
      continuous_bootstrap_to_maturity_history: false, production_ready: false, independent_human_review: false };
    writeFileSync(join(publicDir, 'acceptance.json'), json(result), { flag: 'wx' });
    console.log(json(result));
  }
} catch (error) {
  try { save('failure-' + Date.now(), { message: error.message, stack: error.stack, lifecycle }); } catch { /* retain original */ }
  throw error;
} finally {
  await stopNode('finally');
  for (const lock of locks.reverse()) rmdirSync(lock);
}
