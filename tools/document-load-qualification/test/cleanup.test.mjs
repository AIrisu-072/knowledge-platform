import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { EvidenceReport } from '../../document-poc-runtime/harness.mjs';

const cleanupModule = await import('../cleanup.mjs').catch(() => ({}));
const never = () => new Promise(() => {});
const cid = 'a'.repeat(64), runId = 'owned-run';
const quick = { budgetMs: 500, stopMs: 5, forceMs: 5, proxyMs: 5, ownerMs: 5, removeMs: 5 };
async function cleanup(options, limits) {
  assert.equal(typeof cleanupModule.cleanupOwnedRuntime, 'function');
  return cleanupModule.cleanupOwnedRuntime(options, limits);
}
function owned(pid = 1) {
  const signals = [];
  let finish;
  const done = new Promise(resolve => { finish = resolve; });
  const child = { pid, exitCode: null, signalCode: null,
    kill(signal) { signals.push(signal); child.signalCode = signal; finish({ code: null, signal }); return true; } };
  return { child, done, signals, finish };
}
function docker(owner = runId) {
  const calls = [];
  return { calls, run: async (...args) => { calls.push(args); return args[0] === 'postgres-owner' ? owner : ''; } };
}

test('owned cleanup confirms graceful processes, proxy closure and exactly owned Docker removal', async () => {
  const process = owned(), commands = docker(), env = { PATH: '/test' };
  let proxyClosed = false, stopTimeout;
  const result = await cleanup({ processes: [process], stop: async (p, timeout) => {
    stopTimeout = timeout; p.child.exitCode = 0; p.finish({ code: 0 });
  }, proxy: { close: async () => { proxyClosed = true; } }, cid, runId, run: commands.run, env });
  assert.equal(result.failed, false); assert.equal(proxyClosed, true); assert.equal(stopTimeout, 15_000);
  assert.deepEqual(result.cleanup, [{ pid: 1, result: 'gracefully-stopped' },
    { resource: 'owned-database-proxy', result: 'closed' }, { resource: 'owned-postgres', result: 'removed' }]);
  assert.deepEqual(commands.calls, [
    ['postgres-owner', 'docker', ['inspect', '--format', '{{index .Config.Labels "kp.document-poc.run"}}', cid], env, 10_000],
    ['postgres-cleanup', 'docker', ['rm', '--force', cid], env, 30_000],
  ]);
});

test('already exited or never spawned processes are skipped without waiting for closed pipes', async () => {
  const processes = [owned(1), owned(2), owned(3)];
  processes[0].child.exitCode = 0; processes[1].child.signalCode = 'SIGTERM'; processes[2].child.spawnFailure = true;
  const result = await cleanup({ processes, stop: () => assert.fail('must not stop an exited process') }, quick);
  assert.deepEqual(result, { cleanup: [], failed: false });
  assert.ok(processes.every(p => p.signals.length === 0));
});

test('a failed graceful stop remains failed after confirmed forced cleanup', async () => {
  const process = owned();
  const result = await cleanup({ processes: [process], stop: () => { throw Error('stop failed'); } }, quick);
  assert.equal(result.failed, true); assert.deepEqual(process.signals, ['SIGKILL']);
  assert.deepEqual(result.cleanup, [{ pid: 1, result: 'forced-test-cleanup' }]);
});

test('a hung graceful stop is bounded before forced cleanup', { timeout: 1000 }, async () => {
  const process = owned();
  const result = await cleanup({ processes: [process], stop: never }, quick);
  assert.equal(result.failed, true); assert.deepEqual(process.signals, ['SIGKILL']);
  assert.deepEqual(result.cleanup, [{ pid: 1, result: 'forced-test-cleanup' }]);
});

test('a hung forced-close promise is unconfirmed and does not prevent later owned cleanup', { timeout: 1000 }, async () => {
  const process = owned(), commands = docker(); let proxyClosed = false;
  process.child.kill = signal => { process.signals.push(signal); return true; };
  const result = await cleanup({ processes: [process], stop: async () => { throw Error('stop failed'); },
    proxy: { close: async () => { proxyClosed = true; } }, cid, runId, run: commands.run }, quick);
  assert.equal(result.failed, true); assert.equal(proxyClosed, true);
  assert.deepEqual(result.cleanup[0], { pid: 1, result: 'cleanup-unconfirmed' });
  assert.equal(commands.calls.at(-1)[0], 'postgres-cleanup');
});

test('a throwing forced kill is unconfirmed and later cleanup still runs', async () => {
  const process = owned(); let proxyClosed = false;
  process.child.kill = () => { throw Error('kill failed'); };
  const result = await cleanup({ processes: [process], stop: async () => { throw Error('stop failed'); },
    proxy: { close: async () => { proxyClosed = true; } } }, quick);
  assert.equal(result.failed, true); assert.equal(proxyClosed, true);
  assert.deepEqual(result.cleanup[0], { pid: 1, result: 'cleanup-unconfirmed' });
});

for (const [name, close] of [['throws', () => { throw Error('close failed'); }], ['hangs', never]]) {
  test(`proxy closure that ${name} fails closed while owned Docker cleanup continues`, { timeout: 1000 }, async () => {
    const commands = docker();
    const result = await cleanup({ processes: [], proxy: { close }, cid, runId, run: commands.run }, quick);
    assert.equal(result.failed, true); assert.deepEqual(result.cleanup[0], { resource: 'owned-database-proxy', result: 'cleanup-unconfirmed' });
    assert.equal(commands.calls.at(-1)[0], 'postgres-cleanup');
  });
}

test('Docker label mismatch never removes a foreign container', async () => {
  const commands = docker('another-run');
  const result = await cleanup({ processes: [], cid, runId, run: commands.run }, quick);
  assert.equal(result.failed, true); assert.deepEqual(commands.calls.map(x => x[0]), ['postgres-owner']);
  assert.deepEqual(result.cleanup, [{ resource: 'owned-postgres', result: 'cleanup-unconfirmed' }]);
});

test('invalid container IDs cannot reach Docker', async () => {
  const result = await cleanup({ processes: [], cid: 'foreign --all', runId,
    run: () => assert.fail('must not invoke Docker for an invalid ID') }, quick);
  assert.equal(result.failed, true);
});

for (const [name, inspect] of [['throws', () => { throw Error('inspect failed'); }], ['hangs', never]]) {
  test(`Docker ownership inspection that ${name} cannot trigger removal`, { timeout: 1000 }, async () => {
    const calls = [];
    const result = await cleanup({ processes: [], cid, runId, run: async name => { calls.push(name); return inspect(); } }, quick);
    assert.equal(result.failed, true); assert.deepEqual(calls, ['postgres-owner']);
  });
}

test('Docker removal with no completion confirmation is bounded and failed', { timeout: 1000 }, async () => {
  const result = await cleanup({ processes: [], cid, runId,
    run: async name => name === 'postgres-owner' ? runId : never() }, quick);
  assert.equal(result.failed, true); assert.deepEqual(result.cleanup, [{ resource: 'owned-postgres', result: 'cleanup-unconfirmed' }]);
});

test('the overall cleanup deadline bounds hanging work even when per-operation caps are larger', { timeout: 1000 }, async () => {
  const process = owned(); let proxyStarted = false, dockerStarted = false;
  const result = await cleanup({ processes: [process], stop: never,
    proxy: { close: () => { proxyStarted = true; } }, cid, runId, run: () => { dockerStarted = true; } },
  { budgetMs: 5, stopMs: 100_000 });
  assert.equal(result.failed, true); assert.equal(proxyStarted, false); assert.equal(dockerStarted, false);
  assert.ok(result.cleanup.some(entry => entry.result === 'cleanup-budget-exhausted'));
});

test('a completion arriving after the cleanup budget cannot pass or start later operations', async () => {
  const process = owned(); let now = 0, proxyStarted = false;
  const result = await cleanup({ processes: [process], stop: async p => { now = 121_000; p.child.exitCode = 0; p.finish({ code: 0 }); },
    proxy: { close: () => { proxyStarted = true; } } }, { now: () => now });
  assert.equal(result.failed, true); assert.equal(proxyStarted, false);
  assert.ok(result.cleanup.some(entry => entry.result === 'cleanup-budget-exhausted'));
});

test('each Docker command receives the smaller remaining cleanup budget', async () => {
  let now = 0; const timeouts = [];
  const result = await cleanup({ processes: [], cid, runId, run: async (name, executable, args, env, timeout) => {
    timeouts.push(timeout); now += 10; return name === 'postgres-owner' ? runId : '';
  } }, { budgetMs: 100, now: () => now });
  assert.equal(result.failed, false); assert.deepEqual(timeouts, [100, 90]);
});

test('runtime uses bounded cleanup and Git probes only for explicit ten-thousand or local scale mode', async () => {
  const source = await readFile(new URL('../../document-poc-runtime/run.mjs', import.meta.url), 'utf8');
  assert.match(source, /const provenanceTimeoutMs = tenThousandMode \|\| localScaleMode \? 10_000 : undefined;/);
  const gitCalls = [...source.matchAll(/await run\('[^']+', 'git', \[[^\]]+\]([^)]*)\)/g)];
  assert.equal(gitCalls.length, 6);
  assert.ok(gitCalls.every(match => match[1] === ', undefined, provenanceTimeoutMs'));
  assert.match(source, /if \(tenThousandMode \|\| localScaleMode\) \{\s+const result = await cleanupOwnedRuntime\(/);
  assert.match(source, /if \(result\.failed\) \{ failed = true; report\.data\.status = 'failed'; \}/);
  assert.ok(source.indexOf('await cleanupOwnedRuntime(') < source.indexOf('await report.finish();'));
});

for (const mode of ['ten-thousand','local-scale']) test(`actual ${mode} runtime finalization cannot qualify or export after unconfirmed cleanup`, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'document-load-cleanup-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const report = new EvidenceReport(directory, ['qualification']);
  await report.stage('qualification', async () => {});
  const source = await readFile(new URL('../../document-poc-runtime/run.mjs', import.meta.url), 'utf8');
  const finalization = source.slice(source.indexOf('  const cleanup = [];\n'), source.lastIndexOf('\n}'));
  const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
  const finalize = new AsyncFunction('context', `
    const { process, processes, stopProcess, proxy, cid, runId, run, report, interrupted,
      visualEnabled, console, cleanupOwnedRuntime, assert, tenThousandMode, localScaleMode, writeLocalScaleReceipt } = context;
    let failed = false;
    ${finalization}
  `);
  const process = { env: {} };let receiptAttempted=false;
  await finalize({ process, processes: [], proxy: { close: () => { throw Error('private close failure'); } },
    report, interrupted: false, visualEnabled: false, console: { log() {} }, assert, tenThousandMode:mode==='ten-thousand',localScaleMode:mode==='local-scale',writeLocalScaleReceipt:()=>{receiptAttempted=true;throw Error('Unexpected receipt');},
    cleanupOwnedRuntime: options => cleanup(options, quick) });
  assert.equal(report.data.status, 'failed'); assert.equal(report.data.acceptanceQualified, false);
  assert.equal(receiptAttempted,false);assert.equal(process.exitCode, 1); assert.deepEqual(report.data.stages.at(-1), { name: 'cleanup', status: 'failed' });
  const persisted = JSON.parse(await readFile(report.path, 'utf8'));
  assert.equal(persisted.status, 'failed'); assert.equal(persisted.acceptanceQualified, false);
});
