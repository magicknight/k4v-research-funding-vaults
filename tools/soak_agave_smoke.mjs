// Short observation of the unchanged E11B node, including an observer restart.
// The original fixture shuts down its validator; this does not start a long soak.
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync, createWriteStream } from 'node:fs';
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { finished } from 'node:stream/promises';
import { trackLocalChild, stopLocalChild } from './e11_process.mjs';

const out = resolve('target/e-soak');
assert(!existsSync(out), 'REFUSE_TO_REUSE_SMOKE_OUTPUT');
assert(!existsSync('target/e11b/agave/manifest.json'), 'REFUSE_STALE_BOOTSTRAP_MANIFEST');
mkdirSync(out, { recursive: true });
const log = createWriteStream(resolve(out, 'bootstrap.log'));
const child = spawn(process.execPath, ['tools/e11b_agave_rehearsal.mjs'], {
  detached: true, stdio: ['ignore', 'pipe', 'pipe'],
});
const tracked = trackLocalChild(child);
child.stdout.pipe(log, { end: false });
child.stderr.pipe(log, { end: false });
const tasks = new Set();
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function python(args) {
  const worker = spawn('python3', ['tools/soak_observer.py', ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
  const state = trackLocalChild(worker);
  const task = { worker, state };
  tasks.add(task);
  let stdout = '', stderr = '';
  worker.stdout.on('data', chunk => { stdout += chunk; });
  worker.stderr.on('data', chunk => { stderr += chunk; });
  try {
    const status = await state.closed;
    assert.equal(status.code, 0, stdout + stderr);
    let result;
    try { result = JSON.parse(stdout); }
    catch { result = JSON.parse(stdout.trim().split('\n').at(-1)); }
    return { raw: stdout, result };
  } finally { tasks.delete(task); }
}
let completed = false;
try {
  const start = Date.now();
  let lastStatus = 0;
  while (!existsSync('target/e11b/agave/manifest.json')) {
    if (tracked.error || child.exitCode !== null || child.signalCode !== null)
      throw tracked.error ?? new Error('BOOTSTRAP_EXITED_BEFORE_MANIFEST');
    assert(Date.now() - start < 360000, 'BOOTSTRAP_MANIFEST_TIMEOUT');
    if (Date.now() - lastStatus > 20000) {
      console.log('Waiting for unchanged E11B signed initialization and natural T0');
      lastStatus = Date.now();
    }
    await pause(1000);
  }
  const manifest = resolve('target/e11b/agave/manifest.json');
  const directory = resolve(out, 'journal');
  await python(['init', '--directory', directory, '--manifest', manifest,
    '--origin', 'signed-bootstrap-declared', '--max-gap-seconds', '60']);
  // Retry complete observations only. Every refused attempt remains in the log.
  let summary;
  for (let session = 0; session < 2; session++) {
    let accepted = false;
    for (let attempt = 0; attempt < 4; attempt++) {
      try {
        const args = ['record', '--directory', directory, '--manifest', manifest,
          '--rpc-url', 'http://127.0.0.1:19599', '--samples', '2', '--interval-seconds', '3'];
        if (summary) args.push('--expect-head', summary.head_sha256);
        const observation = await python(args);
        writeFileSync(resolve(out, `observer-${session}-${attempt}.jsonl`), observation.raw, { flag: 'wx' });
        summary = observation.result;
        accepted = true;
        break;
      } catch (error) {
        const replay = await python(['verify', '--directory', directory]);
        summary = JSON.parse(replay.raw);
        if (attempt === 3) throw error;
        await pause(1000);
      }
    }
    assert(accepted);
    await pause(3000);
  }
  const replay = await python(['verify', '--directory', directory, '--expect-head', summary.head_sha256]);
  summary = JSON.parse(replay.raw);
  assert(summary.samples >= 4 && summary.observer_sessions >= 2);
  assert(summary.observed_bank_span_seconds > 0 && summary.observed_wall_span_seconds > 0);
  assert.equal(summary.unclosed_observer_sessions, 0);
  assert.deepEqual(summary.anomalies, []);
  assert.equal(summary.natural_90_180_day_soak, false);
  const status = await tracked.closed;
  assert.equal(status.code, 0, 'E11B_BOOTSTRAP_REHEARSAL_FAILED');
  const receipt = JSON.parse(readFileSync('target/e11b/agave/receipt.json', 'utf8'));
  assert(receipt.valid && receipt.bootstrap === 'six-distinct-role-keys-plus-separate-fee-payer');
  assert.equal(receipt.clock_override, false);
  assert.equal(receipt.policy_token_account_injection, false);
  const result = {
    schema: 'K4V-E-SOAK-SMOKE-v1', valid: true, scope: 'SHORT_ACTUAL_AGAVE_OBSERVATION_AND_OBSERVER_RESTART',
    journal: summary, bootstrap_finalized_transactions: receipt.finalized_client_transactions,
    program_sha256: receipt.program_sha256, genesis_hash: receipt.genesis_hash,
    policy_token_account_injection: false, clock_override: false,
    bootstrap_driver: 'UNCHANGED_FROZEN_E11B', validator_restart_tested: false,
    validator_stopped_after_smoke: true, long_running_task_started: false,
    natural_90_180_day_soak: false, continuous_bootstrap_to_maturity_history: false,
    production_ready: false, independent_human_review: false, public_chain_transactions: 0,
  };
  writeFileSync(resolve(out, 'acceptance.json'), JSON.stringify(result, null, 2) + '\n', { flag: 'wx' });
  console.log(JSON.stringify(result));
  completed = true;
} finally {
  for (const { worker, state } of tasks) await stopLocalChild(worker, state);
  if (!completed && child.pid) {
    // The fixture owns its child validator. On abnormal wrapper termination,
    // terminate only this newly created process group, not arbitrary node PIDs.
    try { process.kill(-child.pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
  await stopLocalChild(child, tracked);
  log.end();
  await finished(log);
}
