import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { trackLocalChild, stopLocalChild } from './e11_process.mjs';
test('SIGTERM exit is collected although exitCode remains null', async () => {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
  const tracked = trackLocalChild(child);
  await once(child, 'spawn');
  const result = await stopLocalChild(child, tracked, 2000);
  assert.equal(result.signal, 'SIGTERM');
  assert.equal(child.exitCode, null);
  assert.equal(child.signalCode, 'SIGTERM');
  assert.deepEqual(await stopLocalChild(child, tracked, 2000), result);
});
test('already closed normal process does not require another exit event', async () => {
  const child = spawn(process.execPath, ['-e', 'process.exit(7)'], { stdio: 'ignore' });
  const tracked = trackLocalChild(child);
  const result = await tracked.closed;
  assert.equal(result.code, 7);
  assert.deepEqual(await stopLocalChild(child, tracked, 2000), result);
});
test('failed spawn closes without an unhandled error or endless wait', async () => {
  const child = spawn('/k4v-test-nonexistent-executable', [], { stdio: 'ignore' });
  const tracked = trackLocalChild(child);
  await tracked.closed;
  assert.equal(tracked.error.code, 'ENOENT');
  await stopLocalChild(child, tracked, 2000);
});
