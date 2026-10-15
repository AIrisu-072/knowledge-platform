import assert from 'node:assert/strict';

/** Test-resource cleanup only. Deadlines cannot guarantee OS-level termination. */
export async function cleanupOwnedRuntime({ processes, stop, proxy, cid, runId, run, env }, {
  budgetMs = 120_000, stopMs = 15_000, forceMs = 5_000, proxyMs = 10_000,
  ownerMs = 10_000, removeMs = 30_000, now = () => performance.now(),
} = {}) {
  const cleanup = [], deadline = now() + budgetMs;
  let failed = false, expired = false;
  const remaining = () => expired ? 0 : deadline - now();
  const exited = owned => owned.child.exitCode !== null || owned.child.signalCode || owned.child.spawnFailure;
  async function bounded(cap, operation) {
    const available = remaining();
    if (available <= 0) throw Error('Owned cleanup budget exhausted');
    const timeoutMs = Math.min(cap, available);
    let timer;
    try {
      const result = await Promise.race([
        Promise.resolve().then(() => operation(timeoutMs)),
        new Promise((_, reject) => { timer = setTimeout(() => {
          if (timeoutMs === available) expired = true;
          reject(Error('Owned cleanup confirmation timed out'));
        }, timeoutMs); }),
      ]);
      if (remaining() <= 0) throw Error('Owned cleanup budget exhausted');
      return result;
    } finally { clearTimeout(timer); }
  }
  for (const owned of processes) {
    if (exited(owned)) continue;
    try {
      await bounded(stopMs, timeout => stop(owned, timeout));
      cleanup.push({ pid: owned.child.pid, result: 'gracefully-stopped' });
    } catch {
      // A failed graceful observation stays failed even if force-cleanup succeeds.
      failed = true;
      try {
        await bounded(forceMs, async () => {
          if (!exited(owned)) owned.child.kill('SIGKILL');
          await owned.done;
        });
        cleanup.push({ pid: owned.child.pid, result: 'forced-test-cleanup' });
      } catch { cleanup.push({ pid: owned.child.pid, result: 'cleanup-unconfirmed' }); }
    }
  }
  if (proxy) {
    try {
      await bounded(proxyMs, () => proxy.close());
      cleanup.push({ resource: 'owned-database-proxy', result: 'closed' });
    } catch {
      failed = true;
      cleanup.push({ resource: 'owned-database-proxy', result: 'cleanup-unconfirmed' });
    }
  }
  if (cid) {
    try {
      assert.match(cid, /^[a-f0-9]{64}$/);
      const owner = await bounded(ownerMs, timeout => run('postgres-owner', 'docker',
        ['inspect', '--format', '{{index .Config.Labels "kp.document-poc.run"}}', cid], env, timeout));
      assert.equal(owner, runId);
      await bounded(removeMs, timeout => run('postgres-cleanup', 'docker', ['rm', '--force', cid], env, timeout));
      cleanup.push({ resource: 'owned-postgres', result: 'removed' });
    } catch {
      failed = true;
      cleanup.push({ resource: 'owned-postgres', result: 'cleanup-unconfirmed' });
    }
  }
  if (remaining() <= 0) {
    failed = true;
    cleanup.push({ resource: 'owned-runtime', result: 'cleanup-budget-exhausted' });
  }
  return { cleanup, failed };
}
