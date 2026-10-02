import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, stat, chmod, rm, symlink, link } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import * as harness from '../harness.mjs';

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'run-worker-control-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const path = join(directory, 'document-semantic-inspection-worker');
  const bytes = Buffer.from('unit fixture for permission control only, never executed');
  await writeFile(path, bytes, { mode: 0o700 });
  return { directory, path, bytes, sha256: createHash('sha256').update(bytes).digest('hex') };
}
test('worker control removes execution only on the verified private copy and restores exact bytes/mode', async t => {
  const f = await fixture(t);
  const result = await harness.withUnavailableWorker(f.directory, 'dsi', f.sha256, async () => {
    assert.equal((await stat(f.path)).mode & 0o777, 0o600); return 'observed';
  });
  assert.equal(result.value, 'observed');
  assert.deepEqual(result.observation, { worker: 'dsi', sha256Before: f.sha256, sha256After: f.sha256, modeBefore: 0o700, disabledMode: 0o600, modeAfter: 0o700 });
  assert.equal((await stat(f.path)).mode & 0o777, 0o700); assert.deepEqual(await readFile(f.path), f.bytes);
});
test('worker control restores permissions even if the real-operation assertion fails', async t => {
  const f = await fixture(t);
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'dsi', f.sha256, async () => { throw Error('operation assertion failed'); }), /operation assertion failed/);
  assert.equal((await stat(f.path)).mode & 0o777, 0o700); assert.deepEqual(await readFile(f.path), f.bytes);
});
test('worker control rejects a changed hash, wrong mode, symlink or shared hardlink before mutation', async t => {
  const f = await fixture(t); let called = false;
  const work = async () => { called = true; };
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'dsi', '0'.repeat(64), work));
  await chmod(f.path, 0o600);
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'dsi', f.sha256, work));
  assert.equal((await stat(f.path)).mode & 0o777, 0o600); await chmod(f.path, 0o700);
  const alias = join(f.directory, 'document-diff-worker');
  await symlink(f.path, alias);
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'diff', f.sha256, work));
  await rm(alias); await link(f.path, alias);
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'diff', f.sha256, work));
  assert.equal((await stat(f.path)).mode & 0o777, 0o700); assert.equal(called, false);
});
test('worker control fails closed on unexpected copy changes while still restoring permissions', async t => {
  const f = await fixture(t);
  await assert.rejects(harness.withUnavailableWorker(f.directory, 'dsi', f.sha256, async () => { await writeFile(f.path, 'changed fixture'); }), /hash/);
  assert.equal((await stat(f.path)).mode & 0o777, 0o700);
});
function failure(worker, problemChanges = {}, transportChanges = {}) {
  const isDsi = worker === 'dsi', status = isDsi ? 503 : 500;
  const code = isDsi ? 'DEPENDENCY_UNAVAILABLE' : 'INTERNAL', title = isDsi ? 'Dependency unavailable' : 'Internal error';
  const instance = `/v1/documents/11111111-1111-4111-8111-111111111111/${isDsi ? 'versions' : 'comparisons'}`;
  return { status, contentType: 'application/problem+json', instance,
    problem: { status, code, title, detail: title, retryable: isDsi, type: `urn:knowledge-platform:problem:${code}`, instance, traceId: '1'.repeat(32), ...problemChanges }, ...transportChanges };
}
test('worker failure oracle rejects success, degraded pseudo-results, wrong classification or leaked fragments', () => {
  assert.deepEqual(harness.assertWorkerFailure(failure('dsi'), 'dsi', ['private worker marker']), { status: 503, code: 'DEPENDENCY_UNAVAILABLE', retryable: true });
  assert.deepEqual(harness.assertWorkerFailure(failure('diff'), 'diff', []), { status: 500, code: 'INTERNAL', retryable: false });
  for (const bad of [undefined, failure('dsi', { status: 200 }), failure('dsi', { verdict: 'unknown', coverage: 'none' }), failure('dsi', { items: [{ text: 'secret' }] }),
    failure('dsi', { detail: 'private worker marker' }), failure('dsi', { code: 'TIMEOUT' }), failure('dsi', {}, { contentType: 'text/html' }),
    failure('dsi', { traceId: 'private worker marker' }), failure('dsi', { type: 'private://input' }), failure('dsi', { instance: '/private/worker' }),
    failure('dsi', { errors: [{ message: 'private worker marker' }] }), failure('dsi', { recovery: { text: 'private worker marker' } })]) {
    assert.throws(() => harness.assertWorkerFailure(bad, 'dsi', ['private worker marker']));
  }
});
test('Diff recovery oracle requires full different with actual untruncated expected text fragments', () => {
  const good = { projection: 'display', verdict: 'different', coverage: 'full', unverifiedRegions: [], nextCursor: null,
    items: [{ base: { kind: 'text', text: 'original-value', truncated: false }, target: { kind: 'text', text: 'updated-value', truncated: false } }] };
  harness.assertRecoveredWorkerDiff(good, 'original-value', 'updated-value');
  for (const changed of [{ ...good, verdict: 'same' }, { ...good, coverage: 'partial' }, { ...good, unverifiedRegions: [{}] }, { ...good, items: [] },
    { ...good, items: [{ base: { kind: 'unavailable' }, target: good.items[0].target }] }, { ...good, items: [{ base: good.items[0].base, target: { ...good.items[0].target, text: 'old-value' } }] }]) {
    assert.throws(() => harness.assertRecoveredWorkerDiff(changed, 'original-value', 'updated-value'));
  }
});
test('worker failure oracle rejects a correct-looking Problem on the wrong HTTP transport status', () => {
  assert.throws(() => harness.assertWorkerFailure(failure('dsi', {}, { status: 502 }), 'dsi', []), /transport/);
});
test('worker failure oracle rejects nested fragments hidden in a permitted Problem field', () => {
  assert.throws(() => harness.assertWorkerFailure(failure('diff', { exactRetry: { items: [{ text: 'Synthetic diff worker acceptance sample-run' }] } }), 'diff', []), /exactRetry/);
});
