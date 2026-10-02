import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstat, mkdir, mkdtemp, rename, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as provenance from '../runtime-provenance.mjs';
import { summarize } from '../ci-summary.mjs';

const runId = '12345678-1234-4abc-8abc-123456789abc', sourceHead = 'a'.repeat(40), cid = 'c'.repeat(64);
const ports = { human: 41001, agent: 41002, postgres: 41003, proxy: 41004 };
const fixtureHash = 'f'.repeat(64);
async function fixture(action) {
  const directory = await mkdtemp(join(tmpdir(), 'runtime-provenance-'));
  const storage = join(directory, 'storage'), manifestPath = join(directory, 'manifest.json');
  await mkdir(storage);
  const manifest = { schemaVersion: 1, baseUrl: 'http://127.0.0.1:41001', fixtureHash, ignored: 'private manifest content' };
  await writeFile(manifestPath, JSON.stringify(manifest));
  const input = { runId, sourceHead, human: manifest.baseUrl, agent: 'http://127.0.0.1:41002',
    database: 'postgres://postgres:credential@127.0.0.1:41003/kp_document_poc',
    proxy: 'postgres://postgres:credential@127.0.0.1:41004/kp_document_poc',
    cid, container: `${cid} ${runId}`, databaseIdentity: 'kp_document_poc:16384', binding: '127.0.0.1:41003', storage, manifestPath };
  try { await action(input, manifest); } finally { await rm(directory, { recursive: true, force: true }); }
}
const hash = values => createHash('sha256').update(JSON.stringify(values)).digest('hex');
function reportFor(receipt) {
  return { runId, gitHead: sourceHead, database: { ownership: 'harness-owned' },
    runtimeProvenance: { initial: receipt, beforeRestart: structuredClone(receipt), afterRestart: structuredClone(receipt) },
    stages: ['seed-replay', 'restart', 'browser-persistence'].map(name => ({ name, status: 'passed' })) };
}

test('owned receipt reads actual fixture and storage identity, emitting only bounded run/head/ports/hashes', async () => {
  await fixture(async input => {
    const receipt = await provenance.observeOwnedRuntime(input), storage = await lstat(input.storage, { bigint: true });
    assert.deepEqual(receipt, { runId, sourceHead, ports, fixtureHash,
      databaseIdentitySha256: hash(['document-poc-runtime/database/v1', runId, sourceHead, cid, 'kp_document_poc', '16384']),
      storageIdentitySha256: hash(['document-poc-runtime/storage/v1', runId, sourceHead, String(storage.dev), String(storage.ino)]) });
    for (const privateValue of [cid, 'credential', input.storage, input.manifestPath, input.database, 'private manifest content']) {
      assert.ok(!JSON.stringify(receipt).includes(privateValue));
    }
    const otherRun = '87654321-4321-4abc-9abc-cba987654321';
    const changed = await provenance.observeOwnedRuntime({ ...input, runId: otherRun, container: `${cid} ${otherRun}` });
    assert.notEqual(changed.databaseIdentitySha256, receipt.databaseIdentitySha256);
    assert.notEqual(changed.storageIdentitySha256, receipt.storageIdentitySha256);
    const otherHead = await provenance.observeOwnedRuntime({ ...input, sourceHead: 'b'.repeat(40) });
    assert.notEqual(otherHead.databaseIdentitySha256, receipt.databaseIdentitySha256);
    assert.notEqual(otherHead.storageIdentitySha256, receipt.storageIdentitySha256);
  });
});

test('owned observation rejects unowned resources, invalid endpoints and a mismatched manifest', async () => {
  await fixture(async (input, manifest) => {
    for (const change of [ { runId: 'arbitrary/private' }, { sourceHead: 'x'.repeat(40) }, { cid: 'short' },
      { container: `${'d'.repeat(64)} ${runId}` }, { container: `${cid} 87654321-4321-4abc-9abc-cba987654321` },
      { databaseIdentity: 'other:16384' }, { databaseIdentity: 'kp_document_poc:0' }, { binding: '127.0.0.1:41005' },
      { human: 'http://127.0.0.1:0' }, { human: 'http://127.0.0.1:65536' }, { human: 'http://remote:41001' },
      { human: 'http://user:credential@127.0.0.1:41001' }, { agent: input.human },
      { proxy: 'postgres://postgres:different@127.0.0.1:41004/kp_document_poc' },
      { database: 'postgres://postgres:credential@127.0.0.1:41003/other' } ]) {
      await assert.rejects(provenance.observeOwnedRuntime({ ...input, ...change }), /runtime provenance/);
    }
    for (const change of [{ schemaVersion: 2 }, { fixtureHash: 'https://private' }, { baseUrl: 'http://127.0.0.1:41005' }]) {
      await writeFile(input.manifestPath, JSON.stringify({ ...manifest, ...change }));
      await assert.rejects(provenance.observeOwnedRuntime(input), /runtime provenance/);
    }
    await writeFile(input.manifestPath, JSON.stringify(manifest));
    await rename(input.storage, `${input.storage}-original`);
    await symlink(`${input.storage}-original`, input.storage);
    await assert.rejects(provenance.observeOwnedRuntime(input), /runtime provenance/);
  });
});

test('restart receipt fails on replacement storage, database, container, fixture, ports, run or head', async () => {
  await fixture(async (input, manifest) => {
    const initial = await provenance.observeOwnedRuntime(input);
    provenance.assertSameRuntime(initial, await provenance.observeOwnedRuntime(input));
    const replacements = [
      await provenance.observeOwnedRuntime({ ...input, databaseIdentity: 'kp_document_poc:16385' }),
      await provenance.observeOwnedRuntime({ ...input, cid: 'd'.repeat(64), container: `${'d'.repeat(64)} ${runId}` }),
      await provenance.observeOwnedRuntime({ ...input, sourceHead: 'b'.repeat(40) }),
      { ...initial, runId: '87654321-4321-4abc-9abc-cba987654321' },
      { ...initial, ports: { ...ports, agent: 41005 } },
    ];
    await rename(input.storage, `${input.storage}-original`); await mkdir(input.storage);
    replacements.push(await provenance.observeOwnedRuntime(input));
    await writeFile(input.manifestPath, JSON.stringify({ ...manifest, fixtureHash: 'e'.repeat(64) }));
    replacements.push(await provenance.observeOwnedRuntime(input));
    for (const replacement of replacements) assert.throws(() => provenance.assertSameRuntime(initial, replacement), /runtime provenance/);
  });
});

test('CI summary requires same run/head receipts across restart and emits no private observation inputs', async () => {
  await fixture(async input => {
    const receipt = await provenance.observeOwnedRuntime(input), report = reportFor(receipt);
    report.runtimeProvenance.raw = input;
    const result = summarize(report).runtime;
    assert.deepEqual(result, { ownership: 'harness-owned', runId, ports, fixtureHash,
      databaseIdentitySha256: receipt.databaseIdentitySha256, storageIdentitySha256: receipt.storageIdentitySha256,
      restartIdentityVerified: true });
    for (const privateValue of [cid, 'credential', input.storage, input.manifestPath, 'postgres://', 'http://']) assert.ok(!JSON.stringify(result).includes(privateValue));
    for (const mutate of [r => { r.runId = '87654321-4321-4abc-9abc-cba987654321'; }, r => { r.gitHead = 'b'.repeat(40); },
      r => { r.runtimeProvenance.beforeRestart = undefined; }, r => { r.runtimeProvenance.afterRestart.fixtureHash = 'e'.repeat(64); },
      r => { r.stages[1].status = 'failed'; }]) {
      const copy = structuredClone(report); mutate(copy); assert.equal(summarize(copy).runtime.restartIdentityVerified, false);
    }
  });
});

test('summary rejects every malformed numeric/hash/UUID field and never claims external ownership', async () => {
  await fixture(async input => {
    const receipt = await provenance.observeOwnedRuntime(input);
    for (const value of [0, 65536, -1, 1.5, '41001', null, {}, 'https://private']) {
      const report = reportFor(structuredClone(receipt)); report.runtimeProvenance.initial.ports.human = value;
      const result = summarize(report).runtime;
      assert.equal(result.restartIdentityVerified, false); assert.equal(result.ports, 'unverified');
    }
    for (const field of ['runId', 'sourceHead', 'fixtureHash', 'databaseIdentitySha256', 'storageIdentitySha256']) {
      const report = reportFor({ ...receipt, [field]: 'private/path\nhttps://credential' });
      const result = summarize(report).runtime;
      assert.equal(result.restartIdentityVerified, false); assert.ok(!JSON.stringify(result).includes('private'));
    }
    const external = summarize({ ...reportFor(receipt), database: { ownership: 'caller-asserted-disposable' } }).runtime;
    assert.equal(external.ownership, 'unverified-external'); assert.equal(external.databaseIdentitySha256, 'unverified');
    assert.equal(external.storageIdentitySha256, 'unverified'); assert.equal(external.restartIdentityVerified, false);
  });
});

test('runner observes actual owned resources after seed and across restart without changing shared-state proofs', async () => {
  const { readFile } = await import('node:fs/promises');
  const run = await readFile(new URL('../run.mjs', import.meta.url), 'utf8');
  assert.match(run, /const upstreamDatabase = database;/);
  assert.match(run, /observeOwnedRuntime\(\{/);
  assert.match(run, /'\{\{\.Id\}\} \{\{index \.Config.Labels "kp.document-poc.run"\}\}'/);
  assert.match(run, /SELECT current_database\(\) \|\| ':' \|\| oid::text FROM pg_database WHERE datname = current_database\(\)/);
  assert.match(run, /assert\.equal\(sourceHead, report\.data\.gitHead/);
  assert.match(run, /database: upstreamDatabase, proxy: proxy\.url, storage, manifestPath/);
  assert.ok(run.indexOf("recordRuntime('initial')") > run.indexOf("await seed('seed-replay')"));
  assert.ok(run.indexOf("recordRuntime('beforeRestart')") < run.indexOf("humanProcess = await start('poc-human', 3)"));
  assert.ok(run.indexOf("recordRuntime('afterRestart')") > run.indexOf("await browser('persistence')"));
  assert.match(run, /assertSameRuntime\(report\.data\.runtimeProvenance\.initial, observed\)/);
  assert.match(run, /if \(report\.data\.database\.ownership !== 'harness-owned'\) return;/);
});

test('private provenance probes keep raw observations in memory and replace process errors with a fixed category', async () => {
  assert.equal(typeof provenance.privateProvenanceProbe, 'function');
  const raw = 'synthetic-private-container-observation';
  assert.equal(await provenance.privateProvenanceProbe(process.execPath, ['-e', 'process.stdout.write(process.argv[1])', raw], process.env), raw);
  await assert.rejects(provenance.privateProvenanceProbe(process.execPath,
    ['-e', 'process.stderr.write(process.argv[1]); process.exit(7)', 'postgres://credential/private'], process.env),
  error => error.message === 'Owned runtime provenance probe failed' && !error.cause && !('stderr' in error));
  await assert.rejects(provenance.privateProvenanceProbe(process.execPath,
    ['-e', 'process.stdout.write("x".repeat(8192))'], process.env), /Owned runtime provenance probe failed/);
});

test('private provenance probe timeout rejects a signal-resistant owned diagnostic client', { timeout: 15_000 }, async () => {
  const started = Date.now();
  await assert.rejects(provenance.privateProvenanceProbe(process.execPath,
    ['-e', 'process.on("SIGTERM", () => {}); setTimeout(() => process.exit(0), 11_200)'], process.env),
  /Owned runtime provenance probe failed/);
  assert.ok(Date.now() - started < 11_200, 'Probe must stop before the resistant client exits voluntarily');
});
