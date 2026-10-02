import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const main = fileURLToPath(new URL('../.build/tools/document-poc-seed/src/main.js', import.meta.url));
test('CLI rejects absent/production mode before contacting the API', () => {
  for (const mode of ['', 'production']) {
    const result = spawnSync(process.execPath, [main], { encoding: 'utf8', env: { ...process.env, KP_RUNTIME_MODE: mode, KP_DOCUMENT_API_BASE_URL: 'http://127.0.0.1:1' } });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /KP_RUNTIME_MODE=poc/);
  }
});
test('CLI rejects nonloopback and credential-bearing API URLs', () => {
  for (const url of ['http://example.test', 'http://secret:password@127.0.0.1:8080', 'http://127.0.0.1:8080/base']) {
    const result = spawnSync(process.execPath, [main], { encoding: 'utf8', env: { ...process.env, KP_RUNTIME_MODE: 'poc', KP_DOCUMENT_API_BASE_URL: url } });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /loopback HTTP origin/);
    assert.ok(!result.stderr.includes('password'));
  }
});
test('CLI redacts filesystem failures rather than printing private manifest paths', async (t) => {
  const { mkdtemp, writeFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const dir = await mkdtemp(join(tmpdir(), 'private-seed-location-'));
  t.after(() => rm(dir, { recursive: true }));
  const blocked = join(dir, 'private-token-file');
  await writeFile(blocked, 'not a directory');
  const result = spawnSync(process.execPath, [main], { encoding: 'utf8', env: { ...process.env,
    KP_RUNTIME_MODE: 'poc', KP_DOCUMENT_API_BASE_URL: 'http://127.0.0.1:1', KP_POC_SEED_MANIFEST: join(blocked, 'manifest.json') } });
  assert.equal(result.status, 1);
  assert.ok(!result.stderr.includes(dir));
  assert.ok(!result.stderr.includes('private-token-file'));
});
