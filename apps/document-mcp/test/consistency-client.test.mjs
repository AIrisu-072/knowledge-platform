// A mock upstream verifies the HARNESS only; never composition-root acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
const bundle = new URL('../dist/consistency.cjs', import.meta.url);
const documentId = '11111111-1111-4111-8111-111111111111';
const versionId = '22222222-2222-4222-8222-222222222222';
const revisionId = '33333333-3333-4333-8333-333333333333';

test('checkpoint reader traverses an actual stdio subprocess and retains exact shared fields and files', async t => {
  assert.ok(existsSync(bundle), 'The real-stdio checkpoint bundle must be built');
  const { readMcpSharedState } = createRequire(import.meta.url)(bundle.pathname);
  const detail = { documentId, documentVersionId: versionId, currentVersionId: versionId, revision: 2, title: 'Synthetic', metadata: { sample: true }, displayVersion: { versionId }, displayRevision: { revisionId } };
  const revisions = { items: [{ revisionId, documentVersionId: versionId }], nextCursor: null };
  const files = { items: [{ contentItemId: 'item', representationId: 'representation', sizeBytes: 12 }] };
  const seen = [];
  const server = createServer((req, res) => {
    const url = new URL(req.url, 'http://localhost'); seen.push({ method: req.method, path: url.pathname, purpose: url.searchParams.get('purpose') });
    const body = url.pathname === '/v1/session' ? { principal: { identityProvider: 'poc', principalId: 'poc-agent' }, invocationKind: 'agent' }
      : url.pathname.endsWith('/revisions') ? revisions : url.pathname.endsWith('/files') ? files : detail;
    res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(body));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  const result = await readMcpSharedState(`http://127.0.0.1:${server.address().port}`, documentId);
  assert.deepEqual(result.state, { detail, revisions, files: [{ versionId, files }] });
  assert.deepEqual(result.transcript.map(call => call.name), ['document_get', 'document_list_revisions', 'document_list_files']);
  assert.equal(seen.length, 4); assert.ok(seen.every(call => call.method === 'GET'));
  assert.equal(seen.at(-1).purpose, 'history');
  assert.deepEqual(result.transcript[0].response, detail);
});
test('checkpoint reader refuses truncated history instead of comparing a partial snapshot', async t => {
  assert.ok(existsSync(bundle), 'The real-stdio checkpoint bundle must be built');
  const { readMcpSharedState } = createRequire(import.meta.url)(bundle.pathname);
  const server = createServer((req, res) => {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify(req.url === '/v1/session' ? { principal: { identityProvider: 'poc', principalId: 'poc-agent' }, invocationKind: 'agent' }
      : req.url.includes('/revisions') ? { items: [], nextCursor: 'more' } : { documentId, currentVersionId: versionId }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  await assert.rejects(readMcpSharedState(`http://127.0.0.1:${server.address().port}`, documentId), /complete/);
});
