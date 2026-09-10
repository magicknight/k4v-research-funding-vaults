// Capture close before shutdown; signal exits leave exitCode null.
export function trackLocalChild(child) {
  let spawnError;
  child.once('error', error => { spawnError = error; });
  const closed = new Promise(resolve => child.once('close', (code, signal) => resolve({ code, signal })));
  return { closed, get error() { return spawnError; } };
}
async function within(promise, milliseconds) {
  let timer;
  try {
    return await Promise.race([promise.then(value => ({ value })),
      new Promise(resolve => { timer = setTimeout(() => resolve(null), milliseconds); })]);
  } finally { clearTimeout(timer); }
}
export async function stopLocalChild(child, tracked, graceMs = 10000) {
  if (child.exitCode === null && child.signalCode === null && !tracked.error) child.kill('SIGTERM');
  let outcome = await within(tracked.closed, graceMs);
  if (outcome) return outcome.value;
  child.kill('SIGKILL');
  outcome = await within(tracked.closed, graceMs);
  if (!outcome) throw new Error('LOCAL_CHILD_SHUTDOWN_TIMEOUT');
  return outcome.value;
}
