import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, symlink, stat, chmod, writeFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { localScaleReports, fingerprint, runId } from './fixtures/local-scale-reports.mjs';
import { verifyLocalScaleReceipt } from '../local-receipt.mjs';

const api = await import('../local-receipt-export.mjs').catch(() => ({}));
const invalid = /^Error: Local scale receipt export failed$/;
const exportName = 'local-scale-export';
const receiptPath = root => join(root, exportName, 'qualification.json');
function inputs() {
  const { cleanup, ...reports } = localScaleReports();
  return { chain: { status: 'SUCCEEDED', ...reports }, options: { sourceHead: fingerprint.code, acceptanceQualified: true,
    finalShutdown: cleanup.records.slice(0, 2), cleanup: { failed: false, cleanup: cleanup.records.slice(2) } } };
}
async function directory(t) {
  const root = await mkdtemp(join(tmpdir(), 'local-scale-export-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}
async function write(root, chain, options) {
  assert.equal(typeof api.writeLocalScaleReceipt, 'function');
  return api.writeLocalScaleReceipt(root, chain, options);
}

test('writes only a bounded compact receipt after final acceptance and actual cleanup records', async t => {
  const root = await directory(t), { chain, options } = inputs();
  chain.privatePath = '/private/PRIVATE_SENTINEL';
  const result = await write(root, chain, options);
  const bytes = await readFile(receiptPath(root)), envelope = JSON.parse(bytes);
  assert.deepEqual(result, { sha256: envelope.sha256, byteLength: bytes.length });
  assert.ok(bytes.length <= 1024 * 1024);
  assert.equal(bytes.at(-1), 10);
  assert.deepEqual(await readdir(root), [exportName]);
  assert.deepEqual(await readdir(join(root, exportName)), ['qualification.json']);
  assert.equal((await stat(join(root, exportName))).mode & 0o777, 0o700);
  assert.equal((await stat(receiptPath(root))).mode & 0o777, 0o600);
  assert.ok(!bytes.includes('PRIVATE_SENTINEL'));
  assert.equal(verifyLocalScaleReceipt(bytes, { ...fingerprint, runId, sha256: result.sha256 }).integrityVerified, true);
  assert.deepEqual(envelope.receipt.cleanup.records, [...options.finalShutdown, ...options.cleanup.cleanup]);
});

test('failed acceptance, incomplete chain, cleanup and source binding never create output', async t => {
  const changes = [
    value => { value.options.acceptanceQualified = false; }, value => { delete value.options.acceptanceQualified; },
    value => { value.chain.status = 'FAILED'; }, value => { value.chain.hundredThousand.status = 'ABORTED'; },
    value => { value.chain.hundredThousand.counts.confirmedPublishedDocuments--; },
    value => { value.options.cleanup.failed = true; }, value => { delete value.options.cleanup.failed; },
    value => { value.options.cleanup.cleanup.pop(); }, value => { value.options.finalShutdown = []; },
    value => { value.options.finalShutdown[0].pid = 103; }, value => { value.options.finalShutdown[0].result = 'forced-test-cleanup'; },
    value => { value.options.sourceHead = 'f'.repeat(40); }, value => { value.options.sourceHead = 'PRIVATE_SENTINEL'; },
    value => { value.options.path = '/private/PRIVATE_SENTINEL'; }, value => { value.options.cleanup.secret = 'PRIVATE_SENTINEL'; },
    value => { value.options.finalShutdown[0].secret = 'PRIVATE_SENTINEL'; },
  ];
  for (const change of changes) {
    const root = await directory(t), value = inputs(); change(value);
    await assert.rejects(write(root, value.chain, value.options), invalid);
    assert.deepEqual(await readdir(root), []);
  }
});

test('export is exclusive and preserves a prior file or stale export directory', async t => {
  const root = await directory(t), { chain, options } = inputs();
  await write(root, chain, options);
  const original = await readFile(receiptPath(root));
  await assert.rejects(write(root, chain, options), invalid);
  assert.deepEqual(await readFile(receiptPath(root)), original);
  const stale = await directory(t);
  await mkdir(join(stale, exportName), { mode: 0o700 });
  await assert.rejects(write(stale, chain, options), invalid);
  assert.deepEqual(await readdir(join(stale, exportName)), []);
});

test('symlinked runtime roots, ancestors and export targets cannot redirect receipt output', async t => {
  const root = await directory(t), other = await directory(t), { chain, options } = inputs();
  const linkedRoot = join(root, 'linked-root'); await symlink(other, linkedRoot);
  await assert.rejects(write(linkedRoot, chain, options), invalid);
  await mkdir(join(other, 'runtime'), { mode: 0o700 });
  await assert.rejects(write(join(linkedRoot, 'runtime'), chain, options), invalid);
  assert.deepEqual(await readdir(join(other, 'runtime')), []);
  const targetRoot = await directory(t); await symlink(other, join(targetRoot, exportName));
  await assert.rejects(write(targetRoot, chain, options), invalid);
  assert.deepEqual(await readdir(other), ['runtime']);
  const fileRoot = await directory(t); await mkdir(join(fileRoot, exportName), { mode: 0o700 });
  const target = join(other, 'keep.json'); await writeFile(target, 'KEEP', { mode: 0o600 });
  await symlink(target, receiptPath(fileRoot));
  await assert.rejects(write(fileRoot, chain, options), invalid);
  assert.equal(await readFile(target, 'utf8'), 'KEEP');
});

test('a non-private or non-directory runtime root fails without leaking paths', async t => {
  const root = await directory(t), { chain, options } = inputs();
  await chmod(root, 0o755);
  await assert.rejects(write(root, chain, options), invalid);
  assert.deepEqual(await readdir(root), []);
  await chmod(root, 0o700);
  const regularFile = join(root, 'PRIVATE_SENTINEL'); await writeFile(regularFile, 'KEEP');
  await assert.rejects(write(regularFile, chain, options), invalid);
  await assert.rejects(write(join(root, 'MISSING_PRIVATE_SENTINEL'), chain, options), invalid);
  assert.equal(await readFile(regularFile, 'utf8'), 'KEEP');
});

test('untrusted cleanup and option getters are rejected before evaluation', async t => {
  const root = await directory(t), value = inputs(); let evaluated = false;
  Object.defineProperty(value.options, 'acceptanceQualified', { enumerable: true, get() { evaluated = true; return true; } });
  await assert.rejects(write(root, value.chain, value.options), invalid);
  assert.equal(evaluated, false);
  assert.deepEqual(await readdir(root), []);
});
