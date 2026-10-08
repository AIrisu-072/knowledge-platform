// Controlled HTTP fake-transport contract tests. These are NOT production acceptance.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';

const modulePath = '../.build/tools/document-load-qualification/src/api.js';
let adapter;
try { adapter = await import(modulePath); } catch (error) { if (error.code !== 'ERR_MODULE_NOT_FOUND') throw error; }
const humanUrl = 'http://127.0.0.1:41001';
const agentUrl = 'http://127.0.0.1:41002';
const root = { folderId: 'root', revision: 4, name: 'Root', parentFolderId: null };
const grants = [
  { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-users', actions: ['read', 'readHistory', 'write', 'publish', 'administer'] },
  { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-agents', actions: ['read', 'readHistory'] },
];
const json = (data, status = 200) => new Response(JSON.stringify(data), { status, headers: { 'content-type': 'application/json' } });
const policy = { bindingMode: 'explicit', policyRevision: 0, effectiveGrants: grants };
function harness(route = () => { throw new Error('Unexpected HTTP request'); }, options = {}) {
  assert.equal(typeof adapter?.DocumentProbe, 'function', 'DocumentProbe adapter has not been implemented');
  const requests = [], timings = [];
  const fetch = async (input, init) => {
    const request = input instanceof Request ? input : new Request(input, init);
    const url = new URL(request.url);
    requests.push({ request, url });
    if (url.pathname === '/v1/session') {
      const human = url.origin === humanUrl;
      return json({ principal: { identityProvider: 'poc', principalId: human ? 'poc-human' : 'poc-agent' }, invocationKind: human ? 'human_interactive' : 'agent', expiresAt: '2099-01-01T00:00:00Z' });
    }
    if (url.pathname === '/v1/folders/root') return json(root);
    if (url.pathname === '/v1/folders/root/access-policy') return json(policy);
    return route(request, url);
  };
  const probe = new adapter.DocumentProbe({ humanUrl, agentUrl, fetch, onTiming: timing => timings.push(timing), ...options });
  return { probe, requests, timings };
}
async function verified(route, options) { const h = harness(route, options); await h.probe.verifySessions(); return h; }

test('fake transport: requires a distinct loopback pair and fixed verified sessions before mutation', async () => {
  const h = harness();
  await assert.rejects(h.probe.publish('d', 'v', 0, 'op'), /verif/i);
  assert.equal(h.requests.length, 0);
  for (const bad of ['https://example.com', 'http://user:pass@localhost', 'http://localhost/path', 'http://localhost/?token=secret']) {
    assert.throws(() => new adapter.DocumentProbe({ humanUrl: bad, agentUrl }), /loopback/i);
  }
  assert.throws(() => new adapter.DocumentProbe({ humanUrl, agentUrl: humanUrl }), /distinct/i);
  const result = await h.probe.verifySessions();
  assert.equal(result.rootFolderId, 'root'); assert.deepEqual(result.root, root);
  assert.equal(result.agent.principal.principalId, 'poc-agent');
  assert.ok(h.requests.every(({ request }) => request.redirect === 'error'));
});

test('fake transport: refuses wrong session identity and never mutates', async () => {
  const requests = [];
  const h = harness(undefined, { fetch: async (input, init) => {
    const req = input instanceof Request ? input : new Request(input, init); requests.push(req);
    return json({ principal: { identityProvider: 'poc', principalId: 'other' }, invocationKind: 'human_interactive' });
  } });
  await assert.rejects(h.probe.verifySessions(), /poc-human/);
  await assert.rejects(h.probe.create({ folderId: 'f', title: 't', bytes: new Uint8Array([1]), filename: 'x', mediaType: 'text/plain' }), /verif/i);
  assert.ok(requests.every(r => r.method === 'GET'));
});

test('fake transport: folder creation and explicit fixed grants are separate journalable requests', async () => {
  const bodies = [];
  const h = await verified(async (request, url) => {
    bodies.push({ url: url.pathname, body: await request.json() });
    return json({ operationId: bodies.at(-1).body.operationId, resourceId: 'f', resultingRevision: 1 }, request.method === 'POST' ? 201 : 200);
  });
  const create = { operationId: 'create-op', folderId: 'f', parentFolderId: 'root', expectedParentRevision: 4, name: 'load-test', reason: 'bounded test' };
  const acl = { operationId: 'policy-op', expectedPolicyRevision: 0, mode: 'explicit', grants, reason: 'bounded test' };
  assert.equal((await h.probe.createFolder(create)).resourceId, 'f');
  assert.equal((await h.probe.setFolderPolicy('f', acl)).resultingRevision, 1);
  assert.deepEqual(bodies, [{ url: '/v1/folders', body: create }, { url: '/v1/folders/f/access-policy', body: acl }]);
  await assert.rejects(h.probe.setFolderPolicy('f', { ...acl, grants: [] }), /grants/i);
  assert.equal(bodies.length, 2);
});

test('fake transport: binary bridge creates exact initial tuple and metadata mutation keeps caller operation/revision', async () => {
  let upload, metadata;
  const tuple = { documentId: 'd', documentVersionId: 'v', fileId: 'file' };
  const h = await verified(async (request, url) => {
    if (url.pathname === '/v1/documents') {
      upload = await request.formData(); return json(tuple, 201);
    }
    metadata = await request.json(); return json({ operationId: metadata.operationId, resourceId: 'd', resultingRevision: 8 });
  });
  assert.deepEqual(await h.probe.create({ folderId: 'f', title: 'title', bytes: new Uint8Array([1, 2, 3]), filename: 'x.bin', mediaType: 'application/octet-stream' }), tuple);
  assert.deepEqual(JSON.parse(await upload.get('request').text()), { folderId: 'f', title: 'title', documentMetadata: { syntheticFixture: 'document-load-qualification-v1' }, versionMetadata: { syntheticFixture: 'document-load-qualification-v1' } });
  assert.deepEqual([...new Uint8Array(await upload.get('file').arrayBuffer())], [1, 2, 3]);
  await h.probe.updateMetadata('d', 7, 'metadata-op');
  assert.deepEqual(metadata, { operationId: 'metadata-op', expectedDocumentRevision: 7, set: { loadQualificationOperation: 'metadata-op' }, unset: [], reason: 'Bounded Document load qualification' });
  assert.ok(h.timings.every(t => Number.isFinite(t.elapsedMs) && t.elapsedMs >= 0 && typeof t.status === 'number'));
  assert.ok(h.timings.some(t => t.operation === 'create' && t.status === 201));
});

test('fake transport: version and publication bodies preserve all caller replay identities', async () => {
  const bodies = [];
  const h = await verified(async request => {
    if (request.headers.get('content-type').startsWith('multipart/')) {
      const body = await request.text(); bodies.push(body);
      return json({ operationId: 'version-op', documentId: 'd', targetVersionId: 'next', resultingRevision: 10 }, 201);
    }
    bodies.push(await request.json()); return json({ publishOperationId: 'publish-op', documentId: 'd', documentVersionId: 'next', resultingDocumentRevision: 11 });
  });
  const asset = { title: 'next title', bytes: new Blob(['new content']), filename: 'next.txt', mediaType: 'text/plain' };
  const result = await h.probe.createNextVersion('d', 9, 'version-op', 'next', 'file-next', asset);
  assert.equal(result.targetVersionId, 'next');
  assert.ok(bodies[0].includes(JSON.stringify({ operationId: 'version-op', targetVersionId: 'next', expectedRevision: 9, title: 'next title', items: [{ logicalPath: 'primary', ordinal: 0, fileId: 'file-next', partId: 'primary', mediaType: 'text/plain', originalFilename: 'next.txt' }] })));
  assert.ok(bodies[0].includes('X-Part-Id: primary')); assert.ok(bodies[0].includes('new content'));
  await h.probe.publish('d', 'next', 10, 'publish-op');
  assert.deepEqual(bodies[1], { operationId: 'publish-op', expectedRevision: 10 });
});

test('fake transport: lists all pages with endpoint-specific views and rejects duplicate IDs and cursor loops', async () => {
  let variant = 'good';
  const queries = [];
  const h = await verified((_request, url) => {
    queries.push(url);
    const cursor = url.searchParams.get('cursor');
    if (!cursor) return json({ items: [{ documentId: 'a' }], nextCursor: 'next' });
    return json({ items: [{ documentId: variant === 'duplicate' ? 'a' : 'b' }], nextCursor: variant === 'loop' ? 'next' : null });
  });
  assert.deepEqual(await h.probe.list('f'), ['a', 'b']);
  assert.equal(queries[0].searchParams.get('view'), 'authoring');
  assert.equal(queries[0].searchParams.get('includeDescendants'), 'false');
  await h.probe.list('f', 'agent');
  assert.equal(queries[2].origin, agentUrl); assert.equal(queries[2].searchParams.get('view'), 'published');
  variant = 'duplicate'; await assert.rejects(h.probe.list('f'), /duplicate/i);
  variant = 'loop'; await assert.rejects(h.probe.list('f'), /cursor/i);
});

test('fake transport: snapshots exact detail, revision details, version metadata and every file byte hash', async () => {
  const detail = { documentId: 'd', revision: 9, currentVersionId: 'v', metadata: { a: 1 } };
  const version = { versionId: 'v', versionNo: 1, metadata: { versionField: 2 } };
  const revisions = [{ revisionId: 'r2', metadataSnapshot: { a: 1 } }, { revisionId: 'r1', metadataSnapshot: {} }];
  const files = [{ contentItemId: 'i', representationId: 'p', mediaType: 'text/plain', sizeBytes: 3 }, { contentItemId: 'i', representationId: 'q', mediaType: 'text/plain', sizeBytes: 2 }];
  const h = await verified((_request, url) => {
    const p = url.pathname;
    if (p === '/v1/documents/d') return json(detail);
    if (p.endsWith('/access-policy')) return json(policy);
    if (p.endsWith('/revisions')) return json({ items: [url.searchParams.has('cursor') ? revisions[1] : revisions[0]], nextCursor: url.searchParams.has('cursor') ? null : 'older' });
    if (p.endsWith('/revisions/r1')) return json(revisions[1]);
    if (p.endsWith('/revisions/r2')) return json(revisions[0]);
    if (p.endsWith('/versions')) return json({ items: [version], nextCursor: null });
    if (p.endsWith('/versions/v')) return json(version);
    if (p.endsWith('/files')) return json({ items: files });
    if (p.endsWith('/p')) return new Response('abc');
    if (p.endsWith('/q')) return new Response('xy');
    throw new Error(p);
  });
  const result = await h.probe.snapshot('d');
  assert.deepEqual(result.detail, detail); assert.deepEqual(result.policy, policy);
  assert.deepEqual(result.revisions, revisions); assert.deepEqual(result.versions[0].detail, version);
  assert.deepEqual(result.versions[0].files, files.map((f, i) => ({ ...f, sha256: createHash('sha256').update(i ? 'xy' : 'abc').digest('hex'), downloadedBytes: i ? 2 : 3 })));
});

test('fake transport: bounded snapshot rejects advertised overflow before download', async () => {
  const h = await verified((_request, url) => {
    const p = url.pathname;
    if (p.endsWith('/d')) return json({ documentId: 'd' });
    if (p.endsWith('/access-policy')) return json(policy);
    if (p.endsWith('/revisions')) return json({ items: [], nextCursor: null });
    if (p.endsWith('/versions')) return json({ items: [{ versionId: 'v' }], nextCursor: null });
    if (p.endsWith('/versions/v')) return json({ versionId: 'v' });
    if (p.endsWith('/files')) return json({ items: [{ contentItemId: 'i', representationId: 'p', sizeBytes: 5 }] });
    throw new Error('download must not run');
  }, { maxFileBytes: 4 });
  await assert.rejects(h.probe.snapshot('d'), /byte|size|limit/i);
  assert.ok(!h.requests.some(({ url }) => url.pathname.endsWith('/p')));
});

test('fake transport: agent status is actual 200 or 403, denies via human-only explicit ACL', async () => {
  let status = 200, acl;
  const h = await verified(async (request, url) => {
    if (request.method === 'PUT') { acl = await request.json(); return json({ operationId: acl.operationId, resourceId: 'd', resultingRevision: 2 }); }
    assert.equal(url.origin, agentUrl); assert.equal(url.searchParams.get('view'), 'published');
    return json(status === 200 ? { documentId: 'd' } : { title: 'Forbidden' }, status);
  });
  assert.deepEqual(await h.probe.agentReadStatus('d'), { status: 200, allowed: true });
  await h.probe.denyAgent('d', 1, 'deny-op');
  assert.deepEqual(acl, { operationId: 'deny-op', expectedPolicyRevision: 1, mode: 'explicit', grants: [grants[0]], reason: 'Bounded Document load qualification' });
  status = 403; assert.deepEqual(await h.probe.agentReadStatus('d'), { status: 403, allowed: false });
  status = 500; await assert.rejects(h.probe.agentReadStatus('d'), /500/);
});

test('fake transport: unknown mutation outcome is not retried and diagnostics exclude server body secrets', async () => {
  let calls = 0;
  const h = await verified(() => { calls++; throw new Error('credential-secret and internal URL'); });
  await assert.rejects(h.probe.publish('d', 'v', 1, 'same-op'), error => !error.message.includes('credential-secret'));
  assert.equal(calls, 1);
  assert.equal(h.timings.at(-1).status, 'network_error');
});

test('fake transport: partial-body snapshot overflow is cancelled without buffering advertised-size lies', async () => {
  let cancelled = false;
  const h = await verified((_request, url) => {
    const p = url.pathname;
    if (p.endsWith('/d')) return json({ documentId: 'd' });
    if (p.endsWith('/access-policy')) return json(policy);
    if (p.endsWith('/revisions')) return json({ items: [], nextCursor: null });
    if (p.endsWith('/versions')) return json({ items: [{ versionId: 'v' }], nextCursor: null });
    if (p.endsWith('/versions/v')) return json({ versionId: 'v' });
    if (p.endsWith('/files')) return json({ items: [{ contentItemId: 'i', representationId: 'p', sizeBytes: 2 }] });
    return new Response(new ReadableStream({ pull(controller) { controller.enqueue(new Uint8Array([1, 2, 3])); }, cancel() { cancelled = true; } }));
  }, { maxFileBytes: 4 });
  await assert.rejects(h.probe.snapshot('d'), /byte.*limit/i);
  assert.equal(cancelled, true);
});

test('fake transport: mutation HTTP failure exposes only status and never retries or logs server details', async () => {
  let calls = 0;
  const h = await verified(() => { calls++; return json({ title: 'secret-cookie=value', detail: 'private password' }, 409); });
  await assert.rejects(h.probe.publish('d', 'v', 1, 'op'), error => error.status === 409 && !/secret|password/.test(error.message));
  assert.equal(calls, 1);
  assert.deepEqual(Object.keys(h.timings.at(-1)).sort(), ['elapsedMs', 'operation', 'status']);
  assert.equal(h.timings.at(-1).status, 409);
});

test('fake transport: cumulative snapshot cap blocks the next representation before download', async () => {
  let downloads = 0;
  const h = await verified((_request, url) => {
    const p = url.pathname;
    if (p.endsWith('/d')) return json({ documentId: 'd' });
    if (p.endsWith('/access-policy')) return json(policy);
    if (p.endsWith('/revisions')) return json({ items: [], nextCursor: null });
    if (p.endsWith('/versions')) return json({ items: [{ versionId: 'v' }], nextCursor: null });
    if (p.endsWith('/versions/v')) return json({ versionId: 'v' });
    if (p.endsWith('/files')) return json({ items: [{ contentItemId: 'i', representationId: 'p', sizeBytes: 3 }, { contentItemId: 'i', representationId: 'q', sizeBytes: 3 }] });
    downloads++; return new Response('abc');
  }, { maxSnapshotBytes: 5 });
  await assert.rejects(h.probe.snapshot('d'), /byte.*limit/i);
  assert.equal(downloads, 1);
});

test('fake transport: caller budget AbortSignal cancels the in-flight generated mutation once', async () => {
  const controller = new AbortController();
  let receivedSignal, notifyStarted;
  const started = new Promise(resolve => { notifyStarted = resolve; });
  const h = await verified(request => {
    receivedSignal = request.signal; notifyStarted();
    return new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')), { once: true });
      setTimeout(() => reject(new Error('Test transport watchdog')), 50).unref();
    });
  }, { signal: controller.signal });
  const pending = h.probe.publish('d', 'v', 1, 'op');
  const rejected = assert.rejects(pending);
  await started; controller.abort();
  assert.equal(receivedSignal.aborted, true, 'the caller budget signal must reach the HTTP request');
  await rejected;
  assert.equal(h.requests.filter(({ request }) => request.method === 'POST').length, 1);
});

test('fake transport: rejected publication exposes only allowlisted matching Problem code and HTTP status', async () => {
  for (const [code, problemStatus, expected] of [['PUBLISH_QUALITY_REJECTED',422,'PUBLISH_QUALITY_REJECTED'],['PRIVATE_SENTINEL',422,null],['PUBLISH_QUALITY_REJECTED',500,null],['INTEGRITY_VIOLATION',422,null]]) {
    const h = await verified(() => json({code,status:problemStatus,detail:'PRIVATE_SENTINEL /secret/password',traceId:'PRIVATE_SENTINEL',errors:[{message:'PRIVATE_SENTINEL'}]},422));
    await assert.rejects(h.probe.publish('d','v',0,'op'), error => {
      assert.deepEqual(error.diagnostic,{operation:'publish',httpStatus:422,problemCode:expected});
      assert.ok(!JSON.stringify(error).includes('PRIVATE_SENTINEL'));
      assert.ok(!error.message.includes('PRIVATE_SENTINEL'));
      return true;
    });
  }
});

test('fake transport: negative authoring snapshot never requests unpublished bytes through history', async () => {
  const h = await verified((_request, url) => {
    const p = url.pathname;
    if (p.endsWith('/d')) return json({ documentId: 'd', currentVersionId: null });
    if (p.endsWith('/access-policy')) return json(policy);
    if (p.endsWith('/revisions')) return json({ items: [], nextCursor: null });
    assert.equal(url.searchParams.get('purpose'), 'authoring');
    if (p.endsWith('/versions')) return json({ items: [{ versionId: 'v' }], nextCursor: null });
    if (p.endsWith('/versions/v')) return json({ versionId: 'v', lifecycleState: 'WORKING' });
    if (p.endsWith('/files')) return json({ items: [{ contentItemId: 'i', representationId: 'p', fileId: 'f', sizeBytes: 3 }] });
    return new Response('abc');
  });
  const snapshot = await h.probe.snapshot('d', { purpose: 'authoring' });
  assert.equal(snapshot.versions[0].files[0].downloadedBytes, 3);
  await assert.rejects(h.probe.snapshot('d', { purpose: 'invalid' }), /purpose/i);
});

test('fake transport: negative folder accepts only the fixed human-only grant', async () => {
  let calls = 0;
  const h = await verified(async request => { calls++; const body=await request.json();assert.deepEqual(body.grants,[grants[0]]);return json({resultingRevision:1}); });
  const request={mode:'explicit',operationId:'op',expectedPolicyRevision:0,reason:'negative corpus',grants:[grants[0]]};
  await h.probe.setFolderPolicy('negative-folder',request);
  await assert.rejects(h.probe.setFolderPolicy('negative-folder',{...request,grants:[grants[1]]}),/grants/i);
  await assert.rejects(h.probe.setFolderPolicy('negative-folder',{...request,grants:[{...grants[0],subjectId:'unrelated'}]}),/grants/i);
  assert.equal(calls,1);
});
