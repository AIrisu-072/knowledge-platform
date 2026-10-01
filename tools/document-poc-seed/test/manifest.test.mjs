import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
const { newUuidV7, openManifest } = await import(process.env.SEED_TEST_SOURCE ? '../src/seed.ts' : '../.build/tools/document-poc-seed/src/seed.js');

test('fresh manifests persist UUIDv7 operation IDs and fixture hash before any request', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'document-seed-'));
  t.after(() => rm(dir, { recursive: true }));
  const path = join(dir, 'manifest.json');
  const manifest = await openManifest(path, 'http://127.0.0.1:8080/');
  assert.match(manifest.fixtureHash, /^[a-f0-9]{64}$/);
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(Object.keys(manifest.folders).length, 3);
  for (const folder of Object.values(manifest.folders)) {
    assert.match(folder.folderId, /^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/);
    assert.notEqual(folder.createOperationId, folder.policyOperationId);
  }
  assert.deepEqual(await openManifest(path, 'http://127.0.0.1:8080'), manifest);
  assert.deepEqual(JSON.parse(await readFile(path, 'utf8')), manifest);
});

test('changed fixture hash or endpoint refuses the existing manifest', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'document-seed-'));
  t.after(() => rm(dir, { recursive: true }));
  const path = join(dir, 'manifest.json');
  const manifest = await openManifest(path, 'http://127.0.0.1:8080');
  await assert.rejects(openManifest(path, 'http://127.0.0.1:8082'), /manifest.*conflict/i);
  manifest.fixtureHash = '0'.repeat(64);
  await writeFile(path, JSON.stringify(manifest));
  await assert.rejects(openManifest(path, 'http://127.0.0.1:8080'), /manifest.*conflict/i);
});

test('UUIDv7 has time bits, correct version and RFC variant with distinct random values', () => {
  const before = Date.now();
  const ids = Array.from({ length: 100 }, () => newUuidV7());
  const after = Date.now();
  assert.equal(new Set(ids).size, ids.length);
  for (const id of ids) {
    assert.match(id, /^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/);
    const timestamp = Number.parseInt(id.replaceAll('-', '').slice(0, 12), 16);
    assert.ok(timestamp >= before && timestamp <= after);
  }
});
