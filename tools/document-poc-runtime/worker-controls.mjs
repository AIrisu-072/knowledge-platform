// Test-only controls for copies created by the owned real-runtime harness.
import assert from 'node:assert/strict';
import { constants } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { basename, join } from 'node:path';
import { createHash } from 'node:crypto';

const workers = { dsi: 'document-semantic-inspection-worker', diff: 'document-diff-worker' };
async function digest(handle) {
  const hash = createHash('sha256');
  for await (const chunk of handle.createReadStream({ start: 0, autoClose: false })) hash.update(chunk);
  return hash.digest('hex');
}
export async function withUnavailableWorker(runDirectory, worker, expectedSha256, action) {
  assert.ok(Object.hasOwn(workers, worker), 'Only fixed runtime workers are supported');
  assert.match(expectedSha256, /^[a-f0-9]{64}$/, 'A built-worker hash is required');
  const directory = await realpath(runDirectory);
  assert.match(basename(directory), /^run-[A-Za-z0-9_-]+$/, 'A private owned run directory is required');
  const parent = await lstat(directory);
  assert.ok(parent.isDirectory() && (parent.mode & 0o077) === 0, 'The run directory must be private');
  assert.equal(parent.uid, process.getuid(), 'The run directory must belong to this runtime');
  const path = join(directory, workers[worker]);
  // Never follow a symlink or touch a shared/hardlinked build binary. The open
  // descriptor binds both chmod operations and hashes to the same private copy.
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat(), named = await lstat(path);
    assert.ok(before.isFile() && named.isFile() && before.nlink === 1, 'Worker must be a private regular copy');
    assert.equal(before.ino, named.ino); assert.equal(before.dev, named.dev);
    assert.equal(before.uid, process.getuid());
    const modeBefore = before.mode & 0o777;
    assert.equal(modeBefore, 0o700, 'Unexpected original worker permissions');
    const sha256Before = await digest(handle);
    assert.equal(sha256Before, expectedSha256, 'Worker hash changed before fault injection');
    let value, sha256After, modeAfter;
    try {
      await handle.chmod(0o600);
      assert.equal((await handle.stat()).mode & 0o777, 0o600);
      value = await action();
    } finally {
      await handle.chmod(modeBefore);
      const restored = await handle.stat(), currentName = await lstat(path);
      modeAfter = restored.mode & 0o777;
      assert.equal(modeAfter, modeBefore, 'Worker permissions were not restored');
      assert.ok(currentName.isFile() && restored.nlink === 1, 'Worker copy changed during observation');
      assert.equal(currentName.ino, before.ino); assert.equal(currentName.dev, before.dev);
      sha256After = await digest(handle);
      assert.equal(sha256After, expectedSha256, 'Worker hash changed during fault injection');
    }
    return { value, observation: { worker, sha256Before, sha256After, modeBefore, disabledMode: 0o600, modeAfter } };
  } finally { await handle.close(); }
}

export function assertWorkerFailure(error, worker, forbidden = []) {
  const expected = worker === 'dsi'
    ? { status: 503, code: 'DEPENDENCY_UNAVAILABLE', title: 'Dependency unavailable', retryable: true }
    : { status: 500, code: 'INTERNAL', title: 'Internal error', retryable: false };
  assert.ok(Object.hasOwn(workers, worker));
  assert.equal(error?.status, expected.status, 'Actual HTTP transport status');
  assert.equal(error?.contentType?.split(';')[0].trim().toLowerCase(), 'application/problem+json', 'Actual problem media type');
  const problem = error?.problem;
  assert.ok(problem && typeof problem === 'object' && !Array.isArray(problem), 'Actual API failure required');
  for (const [key, value] of Object.entries(expected)) assert.equal(problem[key], value, key);
  assert.equal(problem.detail, expected.title, 'Worker details must remain secret-free');
  assert.equal(problem.type, `urn:knowledge-platform:problem:${expected.code}`);
  const id = '[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}';
  assert.match(error.instance, new RegExp(`^/v1/documents/${id}/${worker === 'dsi' ? 'versions' : 'comparisons'}$`));
  assert.equal(problem.instance, error.instance, 'Problem must identify the actual request');
  assert.match(problem.traceId, /^[a-f0-9]{32}$/); assert.notEqual(problem.traceId, '0'.repeat(32));
  assert.ok(problem.exactRetry === undefined || typeof problem.exactRetry === 'boolean', 'exactRetry must not carry nested data');
  const fields = new Set(['type', 'title', 'status', 'detail', 'instance', 'traceId', 'code', 'retryable', 'errors', 'recovery', 'exactRetry']);
  assert.ok(Object.keys(problem).every(key => fields.has(key)), 'An API failure cannot contain a successful or partial result');
  assert.deepEqual(problem.errors ?? [], []);
  assert.equal(problem.recovery ?? null, null);
  for (const value of forbidden) if (value) assert.ok(!JSON.stringify(problem).includes(value), 'Worker failure leaked private input');
  return { status: error.status, code: problem.code, retryable: problem.retryable };
}
export function assertRecoveredWorkerDiff(result, baseText, targetText) {
  assert.equal(result.projection, 'display'); assert.equal(result.verdict, 'different'); assert.equal(result.coverage, 'full');
  assert.deepEqual(result.unverifiedRegions, []); assert.equal(result.nextCursor, null);
  assert.ok(Array.isArray(result.items) && result.items.length > 0, 'Recovered Diff must contain actual changes');
  for (const item of result.items) for (const fragment of [item.base, item.target]) if (fragment) {
    assert.equal(fragment.kind, 'text'); assert.equal(fragment.truncated, false);
  }
  assert.ok(result.items.some(item => item.base?.kind === 'text' && item.base.text.includes(baseText)), 'Original text fragment missing');
  assert.ok(result.items.some(item => item.target?.kind === 'text' && item.target.text.includes(targetText)), 'Changed text fragment missing');
}
