import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, symlink, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { validatePlan, admitStage, summarizeTimings, observeResources } from '../safety.mjs';

const fingerprint = { corpus: 'sha256:corpus', code: 'a'.repeat(40), runtime: 'runtime:fixed' };
const plan = (stage = 'small', overrides = {}) => ({
  stage, documentCount: stage === 'small' ? 10 : stage,
  fingerprint: { ...fingerprint }, safetyFactor: 2,
  budgets: { diskReserveBytes: 1000, minAvailableMemoryBytes: 1000, maxWallTimeMs: 1e8, maxRssBytes: 1e9 },
  deadlineAt: new Date(Date.now() + 1e8).toISOString(),
  ...overrides,
});
const observation = (overrides = {}) => ({
  observedAt: new Date().toISOString(), diskFreeBytes: 1e10, storageDiskFreeBytes: 1e10, databaseDiskFreeBytes: 1e10,
  availableMemoryBytes: 1e10, rssBytes: 100, storageBytes: 1000, databaseBytes: 2000,
  rssCoverage: 'process-tree', databaseFilesystemVerified: true,
  limitations: [], ...overrides,
});
const report = (stage = 'small', overrides = {}) => ({
  schemaVersion:1,evidenceClass:'owned-real-process',restart:{identityRetained:true,processesReplaced:true,before:{sourceHead:fingerprint.code,databaseIdentitySha256:'b'.repeat(64),storageIdentitySha256:'c'.repeat(64)},after:{sourceHead:fingerprint.code,databaseIdentitySha256:'b'.repeat(64),storageIdentitySha256:'c'.repeat(64)},processes:{before:[100,200],after:[101,201]}},evidence:{documentIds:Array.from({length:stage==='small'?10:stage},(_,i)=>String(i))},status: 'SUCCEEDED', stage, documentCount: stage === 'small' ? 10 : stage,
  fingerprint: { ...fingerprint },
  metrics: { totalElapsedMs: 1000, diskGrowthBytes: 10000, peakRssBytes: 20000 },
  ...(stage === 1000 ? { previousReport: report() } : stage === 10000 ? { previousReport: report(1000) } : {}),
  ...overrides,
});

test('validatePlan accepts all bounded stages and rejects mismatched counts', () => {
  for (const stage of ['small', 1000, 10000, 100000]) assert.deepEqual(validatePlan(plan(stage)), { valid: true, errors: [] });
  for (const count of [0, 21, -1, 1.2, NaN, null, '10']) assert.equal(validatePlan(plan('small', { documentCount: count })).valid, false);
  assert.equal(validatePlan(plan(1000, { documentCount: 999 })).valid, false);
  assert.equal(validatePlan(plan(500)).valid, false);
});

test('validatePlan rejects missing, non-finite and coercible budgets, factors and fingerprints', () => {
  for (const invalid of [null, NaN, Infinity, -1, '1000', undefined]) {
    for (const field of Object.keys(plan().budgets)) {
      assert.equal(validatePlan(plan('small', { budgets: { ...plan().budgets, [field]: invalid } })).valid, false, field);
    }
  }
  for (const invalid of [undefined, null, {}, [], 'plan']) assert.equal(validatePlan(invalid).valid, false);
  for (const factor of [0, 0.99, null, NaN, Infinity, '2']) assert.equal(validatePlan(plan('small', { safetyFactor: factor })).valid, false);
  for (const bad of [null, {}, { ...fingerprint, code: '' }, { ...fingerprint, corpus: 1 }]) assert.equal(validatePlan(plan('small', { fingerprint: bad })).valid, false);
  assert.equal(validatePlan(plan('small', { deadlineAt: 'tomorrow' })).valid, false);
});

test('admission allows measured small baseline and complete successful stage chains', () => {
  assert.equal(admitStage(plan(), null, observation()).status, 'ADMITTED');
  for (const stage of [1000, 10000, 100000]) {
    const previous = stage === 1000 ? report() : report(stage / 10);
    const decision = admitStage(plan(stage), previous, observation());
    assert.equal(decision.status, 'ADMITTED', JSON.stringify(decision));
    assert.equal(decision.projection.totalElapsedMs, previous.metrics.totalElapsedMs * stage / previous.documentCount * 2);
    assert.equal(decision.projection.diskGrowthBytes, previous.metrics.diskGrowthBytes * stage / previous.documentCount * 2);
    assert.equal(decision.projection.peakRssBytes, previous.metrics.peakRssBytes * 2);
  }
});

test('admission refuses skipped, failed, mismatched or incomplete stage histories', () => {
  for (const previous of [null, report(), report(1000, { status: 'FAILED' }), report(1000, { previousReport: null }), report(1000, { fingerprint: { ...fingerprint, code: 'other' } })]) {
    assert.equal(admitStage(plan(10000), previous, observation()).status, 'NOT_ADMITTED');
  }
  const deepMismatch = report(10000);
  deepMismatch.previousReport.previousReport.fingerprint.corpus = 'other';
  assert.equal(admitStage(plan(100000), deepMismatch, observation()).status, 'NOT_ADMITTED');
  const cyclic = report(1000); cyclic.previousReport = cyclic;
  assert.equal(admitStage(plan(10000), cyclic, observation()).status, 'NOT_ADMITTED');
});

test('admission refuses every missing or nonnumeric resource and preceding measured metric', () => {
  for (const field of ['diskFreeBytes', 'availableMemoryBytes', 'rssBytes', 'storageBytes', 'databaseBytes']) {
    for (const invalid of [undefined, null, NaN, Infinity, '1000', -1]) {
      assert.equal(admitStage(plan(), null, observation({ [field]: invalid })).status, 'NOT_ADMITTED', field);
    }
  }
  for (const field of ['totalElapsedMs', 'diskGrowthBytes', 'peakRssBytes']) {
    for (const invalid of [undefined, null, NaN, Infinity, '1000', -1]) {
      const previous = report();
      previous.metrics[field] = invalid;
      assert.equal(admitStage(plan(1000), previous, observation()).status, 'NOT_ADMITTED', field);
    }
  }
});

test('admission requires observed database filesystem and complete process-tree coverage', () => {
  for (const override of [{ databaseFilesystemVerified: false }, { databaseFilesystemVerified: undefined }, { rssCoverage: 'roots-only' }, { rssCoverage: undefined }]) {
    assert.equal(admitStage(plan(), null, observation(override)).status, 'NOT_ADMITTED');
  }
});

test('admission refuses resource reserves, wall-time, RSS and deadline exhaustion', () => {
  const previous = report();
  const p = plan(1000);
  for (const override of [{ diskFreeBytes: 2000999 }, { availableMemoryBytes: 40999 }, { rssBytes: p.budgets.maxRssBytes + 1 }]) {
    assert.equal(admitStage(p, previous, observation(override)).status, 'NOT_ADMITTED');
  }
  for (const budgets of [{ ...p.budgets, maxWallTimeMs: 199999 }, { ...p.budgets, maxRssBytes: 39999 }]) {
    assert.equal(admitStage({ ...p, budgets }, previous, observation()).status, 'NOT_ADMITTED');
  }
  assert.equal(admitStage({ ...p, deadlineAt: new Date(Date.now() + 1000).toISOString() }, previous, observation()).status, 'NOT_ADMITTED');
  assert.equal(admitStage(plan('small', { deadlineAt: new Date(Date.now() - 1).toISOString() }), null, observation()).status, 'NOT_ADMITTED');
});

test('timings use nearest-rank percentiles without mutating samples', () => {
  const samples = [100, ...Array.from({ length: 99 }, (_, i) => i + 1)];
  assert.deepEqual(summarizeTimings(samples), { count: 100, minMs: 1, p50Ms: 50, p95Ms: 95, p99Ms: 99, maxMs: 100, meanMs: 50.5 });
  assert.equal(samples[0], 100);
  assert.deepEqual(summarizeTimings([4]), { count: 1, minMs: 4, p50Ms: 4, p95Ms: 4, p99Ms: 4, maxMs: 4, meanMs: 4 });
  assert.deepEqual(summarizeTimings([]), { count: 0, minMs: null, p50Ms: null, p95Ms: null, p99Ms: null, maxMs: null, meanMs: null });
  for (const invalid of [null, [1, NaN], [1, Infinity], [1, null], [-1], ['1']]) assert.throws(() => summarizeTimings(invalid), /finite nonnegative/i);
});

test('timings reject sparse samples rather than publishing missing percentiles', () => {
  assert.throws(() => summarizeTimings(Array(2)), /finite nonnegative/i);
  assert.throws(() => summarizeTimings([1, , 3]), /finite nonnegative/i);
  assert.equal(summarizeTimings([0.25, 0.5, 0.75]).p50Ms, 0.5);
  assert.equal(summarizeTimings([Number.MAX_VALUE, Number.MAX_VALUE]).meanMs, Number.MAX_VALUE);
});

test('admission fails closed on malformed top-level inputs and projection overflow', () => {
  for (const input of [undefined, null, [], 'wrong', NaN]) {
    assert.equal(admitStage(input, null, observation()).status, 'NOT_ADMITTED');
    assert.equal(admitStage(plan(), null, input).status, 'NOT_ADMITTED');
    assert.equal(admitStage(plan(1000), input, observation()).status, 'NOT_ADMITTED');
  }
  const previous = report(); previous.metrics.totalElapsedMs = Number.MAX_VALUE;
  assert.equal(admitStage(plan(1000), previous, observation()).status, 'NOT_ADMITTED');
});

test('resources measure real filesystem, memory, PID RSS and caller-supplied database bytes', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'document-load-safety-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'nested'));
  await writeFile(path.join(root, 'a.pdf'), Buffer.alloc(11));
  await writeFile(path.join(root, 'nested', 'b.pdf'), Buffer.alloc(17));
  await symlink(root, path.join(root, 'nested', 'loop'));
  const measured = await observeResources({ storageRoot: root, databaseRoot: root, pids: [process.pid], databaseBytes: 31 });
  assert.equal(measured.storageBytes, 28);
  assert.equal(measured.databaseBytes, 31);
  assert.ok(measured.diskFreeBytes > 0);
  assert.ok(measured.availableMemoryBytes > 0);
  assert.ok(measured.rssBytes > 0);
  assert.equal(measured.databaseFilesystemVerified, true);
  assert.equal(measured.rssCoverage, 'process-tree');
  assert.ok(Number.isFinite(Date.parse(measured.observedAt)));
  assert.ok(measured.limitations.some(value => /symlink/i.test(value)));
});

test('resource observations report unavailable measurements and bounded traversal honestly', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'document-load-safety-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(path.join(root, 'a.pdf'), 'abc');
  await writeFile(path.join(root, 'b.pdf'), 'abcd');
  const missing = await observeResources({ storageRoot: path.join(root, 'missing'), pids: [], databaseBytes: null });
  assert.equal(missing.storageBytes, null);
  assert.equal(missing.diskFreeBytes, null);
  assert.equal(missing.rssBytes, null);
  assert.equal(missing.databaseBytes, null);
  assert.ok(missing.limitations.length >= 3);
  const bounded = await observeResources({ storageRoot: root, pids: [process.pid], databaseBytes: 0, maxStorageEntries: 1 });
  assert.equal(bounded.storageBytes, null);
  assert.ok(bounded.limitations.some(value => /bound|limit/i.test(value)));
  assert.equal(bounded.databaseFilesystemVerified, false);
  const missingDatabase = await observeResources({ storageRoot: root, databaseRoot: path.join(root, 'missing'), pids: [process.pid], databaseBytes: 0 });
  assert.equal(missingDatabase.databaseFilesystemVerified, false);
  assert.equal(missingDatabase.diskFreeBytes, null);
  const absentPid = await observeResources({ storageRoot: root, databaseRoot: root, pids: [2147483647], databaseBytes: 0 });
  assert.equal(absentPid.rssBytes, null);
  assert.equal(absentPid.rssCoverage, 'unavailable');
});

test('RSS includes live descendants and deduplicates overlapping supplied process roots', async (t) => {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
  t.after(async () => { child.kill(); await once(child, 'exit').catch(() => {}); });
  await once(child, 'spawn');
  const measured = await observeResources({ storageRoot: os.tmpdir(), pids: [process.pid, process.pid], databaseBytes: 0, maxStorageEntries: 1 });
  assert.ok(measured.measuredPids.includes(process.pid));
  assert.ok(measured.measuredPids.includes(child.pid));
  assert.equal(measured.measuredPids.length, new Set(measured.measuredPids).size);
  assert.ok(measured.rssBytes > 0);
});

test('resource observation refuses symlink roots and malformed explicit measurements', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'document-load-safety-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'actual'));
  await symlink(path.join(root, 'actual'), path.join(root, 'linked'));
  const measured = await observeResources({ storageRoot: path.join(root, 'linked'), databaseRoot: root, pids: ['1'], databaseBytes: NaN });
  for (const field of ['storageBytes', 'databaseBytes', 'diskFreeBytes', 'rssBytes']) assert.equal(measured[field], null, field);
  assert.equal(measured.rssCoverage, 'unavailable');
  const sameFilesystem = await observeResources({ storageRoot: root, databaseSharesStorageFilesystem: true, pids: [process.pid], databaseBytes: 0 });
  assert.equal(sameFilesystem.databaseFilesystemVerified, true);
  assert.ok(sameFilesystem.limitations.some(value => /assertion/i.test(value)));
  const invalidBound = await observeResources({ storageRoot: root, pids: [process.pid], databaseBytes: 0, maxStorageObservationMs: NaN });
  assert.equal(invalidBound.storageBytes, null);
});
test('larger-stage admission rejects unit-test receipts without real restart provenance',()=>{assert.equal(admitStage(plan(1000),{...report(),evidenceClass:'test-double'},observation()).status,'NOT_ADMITTED');});
