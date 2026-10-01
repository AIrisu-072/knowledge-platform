import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { runSeed } from '../.build/tools/document-poc-seed/src/seed.js';
import { fakeApi, grants } from './fake-api.mjs';

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'document-seed-'));
  t.after(() => rm(dir, { recursive: true }));
  const api = fakeApi();
  const options = { manifestPath: join(dir, 'manifest.json'), baseUrl: 'http://127.0.0.1:8080', fetch: api.fetch };
  return { api, options, seed: () => runSeed(options) };
}
test('generated client and binary bridge seed twice without repeating any mutation', async (t) => {
  const { api, seed } = await fixture(t);
  const first = await seed();
  const count = api.mutations().length;
  const auditCount = api.operations.size;
  assert.equal(api.documents.size, 5);
  assert.equal(api.folders.size, 4);
  assert.equal([...api.documents.values()].flatMap(d => d.versions).length, 6);
  assert.deepEqual(api.policies.get(first.folders.shared.folderId).effectiveGrants, grants);
  assert.deepEqual(api.policies.get(first.folders.sandbox.folderId).effectiveGrants, grants);
  assert.deepEqual(api.policies.get(first.folders.humanOnly.folderId).effectiveGrants, [grants[0]]);
  assert.deepEqual(await seed(), first);
  assert.equal(api.mutations().length, count);
  assert.equal(api.operations.size, auditCount);
});
test('lost folder response retries exact saved operation without duplicate mutations', async (t) => {
  const { api, seed } = await fixture(t);
  api.loseNextResponse(r => r.path === '/v1/folders');
  await assert.rejects(seed(), /lost response/);
  const operationIds = [...api.operations.keys()];
  assert.equal(api.folders.size, 2);
  await seed();
  assert.equal(api.folders.size, 4);
  assert.ok(api.operations.has(operationIds[0]));
  assert.equal(api.requests.filter(r => r.path === '/v1/folders' && r.method === 'POST').length, 4);
});
test('lost successful initial-create response fails closed on retry without a second POST', async (t) => {
  const { api, seed, options } = await fixture(t);
  api.loseNextResponse(r => r.path === '/v1/documents' && r.method === 'POST');
  await assert.rejects(seed());
  assert.equal(api.documents.size, 1);
  const mutations = api.mutations().length;
  await assert.rejects(seed(), /unknown.*create|create.*unknown/i);
  assert.equal(api.documents.size, 1);
  assert.equal(api.mutations().length, mutations);
  const state = JSON.parse(await readFile(options.manifestPath, 'utf8'));
  assert.equal(state.documents.regulation.create.pending, true);
  assert.equal(state.documents.regulation.create.result, undefined);
});
for (const [name, alter] of [
  ['metadata', api => { [...api.documents.values()][0].metadata.extra = 'changed'; }],
  ['content bytes', api => { [...api.documents.values()][0].versions[0].content = 'changed'; }],
  ['folder policy', api => { api.policies.get([...api.folders.keys()][1]).effectiveGrants = []; }],
]) test(`altered ${name} refuses a completed seed without overwriting`, async (t) => {
  const { api, seed } = await fixture(t);
  await seed();
  const mutations = api.mutations().length;
  alter(api);
  await assert.rejects(seed(), /conflict/i);
  assert.equal(api.mutations().length, mutations);
});
test('agent endpoint is rejected before mutations', async (t) => {
  const { api, seed } = await fixture(t);
  api.setActor('poc-agent');
  await assert.rejects(seed(), /poc-human/);
  assert.equal(api.mutations().length, 0);
});

test('a manifest lock blocks a concurrent seeder before network traffic', async (t) => {
  const { api, seed, options } = await fixture(t);
  const { writeFile } = await import('node:fs/promises');
  await writeFile(`${options.manifestPath}.lock`, 'another process');
  await assert.rejects(seed(), /locked/i);
  assert.equal(api.requests.length, 0);
});
test('altered version metadata is rejected without overwriting', async (t) => {
  const { api, seed } = await fixture(t);
  await seed();
  const count = api.mutations().length;
  [...api.documents.values()][0].versions[0].metadata = { unexpected: 'change' };
  await assert.rejects(seed(), /conflict/i);
  assert.equal(api.mutations().length, count);
});
test('seed refuses a same-name preexisting folder with no manifest provenance', async (t) => {
  const { api, seed } = await fixture(t);
  api.folders.set('other', { folderId: 'other', name: 'PoC Shared', revision: 0, parentFolderId: api.rootId });
  await assert.rejects(seed(), /conflict/i);
  assert.equal(api.mutations().length, 0);
});
for (const suffix of ['/access-policy', ':publish', '/versions']) {
  test(`lost ${suffix} response retries exact operation IDs without duplicates`, async (t) => {
    const { api, seed } = await fixture(t);
    api.loseNextResponse(r => r.method !== 'GET' && r.path.endsWith(suffix));
    await assert.rejects(seed());
    await seed();
    assert.equal(api.documents.size, 5);
    assert.equal([...api.documents.values()].flatMap(d => d.versions).length, 6);
    assert.equal(api.operations.size, 13); // 3 folder creates + 3 ACLs + 6 publishes + 1 version.
  });
}
test('an unexpected explicit document ACL with identical grants is rejected before publication', async (t) => {
  const { api, options } = await fixture(t);
  const wrappedFetch = async (input, init) => {
    const request = input instanceof Request ? input : new Request(input, init);
    const isPolicy = request.method === 'GET' && new URL(request.url).pathname.match(/^\/v1\/documents\/[^/]+\/access-policy$/);
    const response = await api.fetch(input, init);
    if (isPolicy) {
      return new Response(JSON.stringify({ ...await response.json(), bindingMode: 'explicit', policyRevision: 999 }), { headers: { 'content-type': 'application/json' } });
    }
    return response;
  };
  await assert.rejects(runSeed({ ...options, fetch: wrappedFetch }), /conflict/i);
  assert.equal(api.requests.filter(r => r.path.endsWith(':publish')).length, 0);
});
