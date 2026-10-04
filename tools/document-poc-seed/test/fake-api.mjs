// Transport-contract fixture only. This is not real backend acceptance.
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
const grant = (subjectId, actions) => ({ subjectKind: 'group', identityProvider: 'poc', subjectId, actions });
export const grants = [grant('poc-users', ['read', 'readHistory', 'write', 'publish', 'administer']), grant('poc-agents', ['read', 'readHistory'])];
export function fakeApi() {
  const rootId = randomUUID();
  const folders = new Map([[rootId, { folderId: rootId, name: 'Root', revision: 0, parentFolderId: null }]]);
  const policies = new Map([[rootId, { bindingMode: 'explicit', policyRevision: 1, effectiveGrants: grants }]]);
  const documents = new Map();
  const operations = new Map();
  const requests = [];
  let loseResponse;
  let actor = 'poc-human';
  const response = (value, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } });
  const fileList = version => ({ items: [{ contentItemId: version.contentItemId, representationId: version.representationId, logicalPath: 'primary', ordinal: 0, role: 'primary', displayName: version.filename, mediaType: 'text/plain', sizeBytes: Buffer.byteLength(version.content) }] });
  const detail = doc => ({ documentId: doc.documentId, documentVersionId: doc.versions.at(-1).versionId, title: doc.title, folderId: doc.folderId, revision: doc.revision, metadata: doc.metadata, currentVersionId: doc.currentVersionId, lifecycleState: doc.versions.at(-1).lifecycleState });
  const fetcher = async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url);
    const path = url.pathname;
    const method = request.method;
    requests.push({ method, path });
    let value;
    let status = 200;
    if (method === 'GET') {
      if (path === '/v1/session') return response({ principal: { identityProvider: 'poc', principalId: actor }, invocationKind: actor === 'poc-human' ? 'human_interactive' : 'agent' });
      if (path === '/v1/folders/root') return response(folders.get(rootId));
      const folderChildren = path.match(/^\/v1\/folders\/([^/]+)\/children$/);
      if (folderChildren) return response({ items: [...folders.values()].filter(f => f.parentFolderId === folderChildren[1]), nextCursor: null });
      const folderPolicy = path.match(/^\/v1\/folders\/([^/]+)\/access-policy$/);
      if (folderPolicy) return response(policies.get(folderPolicy[1]) ?? { bindingMode: 'inherit', policyRevision: 0, effectiveGrants: grants });
      if (path.startsWith('/v1/document-creation-outcomes/')) {
        const doc = documents.get(path.split('/')[3]);
        assert.equal(url.searchParams.get('documentVersionId'), doc.versions[0].versionId);
        assert.equal(url.searchParams.get('fileId'), doc.versions[0].fileId);
        return response({ documentId: doc.documentId, documentVersionId: doc.versions[0].versionId, fileId: doc.versions[0].fileId });
      }
      if (path === '/v1/documents') return response({ view: 'authoring', items: [...documents.values()].filter(d => d.folderId === url.searchParams.get('folderId')).map(detail), nextCursor: null });
      const found = path.match(/^\/v1\/documents\/([^/]+)(.*)$/);
      if (found) {
        const doc = documents.get(found[1]);
        assert.ok(doc, `unknown fixture document ${found[1]}`);
        const suffix = found[2];
        if (!suffix) return response(detail(doc));
        if (suffix === '/access-policy') return response({ ...policies.get(doc.folderId), bindingMode: 'inherit', policyRevision: 0, effectiveSource: { kind: 'folder', id: doc.folderId } });
        if (suffix === '/versions') return response({ items: doc.versions.filter(v => url.searchParams.get('purpose') !== 'authoring' || v.lifecycleState === 'working'), nextCursor: null });
        if (suffix === '/revisions') return response({ items: doc.versions.filter(v => v.lifecycleState === 'published').map((v, i) => ({ revisionId: v.revisionId, documentVersionId: v.versionId, major: i + 1, minor: 0, label: `${i + 1}.0` })), nextCursor: null });
        const versionPath = suffix.match(/^\/versions\/([^/]+)(.*)$/);
        if (versionPath) {
          const version = doc.versions.find(v => v.versionId === versionPath[1]);
          assert.ok(url.searchParams.get('purpose') !== 'authoring' || version.lifecycleState === 'working', 'authoring purpose cannot read published versions');
          if (!versionPath[2]) return response(version);
          if (versionPath[2] === '/files') return response(fileList(version));
          if (versionPath[2].startsWith('/files/')) return new Response(version.content, { headers: { 'content-type': 'text/plain' } });
        }
      }
      throw new Error(`Unknown fixture GET ${path}`);
    }
    const body = request.headers.get('content-type')?.startsWith('multipart/') ? undefined : await request.json();
    const replay = body?.operationId && operations.get(body.operationId);
    if (replay) { assert.deepEqual(replay.body, body); return response(replay.result, replay.status); }
    if (path === '/v1/folders') {
      folders.set(body.folderId, { folderId: body.folderId, name: body.name, revision: 0, parentFolderId: body.parentFolderId });
      value = { operationId: body.operationId, resourceId: body.folderId, resultingRevision: 0, changed: true };
      status = 201;
    } else if (path.match(/^\/v1\/folders\/[^/]+\/access-policy$/)) {
      const id = path.split('/')[3];
      policies.set(id, { bindingMode: 'explicit', policyRevision: body.expectedPolicyRevision + 1, effectiveGrants: body.grants });
      value = { operationId: body.operationId, resourceId: id, resultingRevision: body.expectedPolicyRevision + 1, changed: true };
    } else if (path === '/v1/documents') {
      const form = await request.formData();
      const metadata = JSON.parse(await form.get('request').text());
      const file = form.get('file');
      const documentId = randomUUID(), documentVersionId = randomUUID(), fileId = randomUUID();
      documents.set(documentId, { ...metadata, documentId, revision: 1, metadata: metadata.documentMetadata, currentVersionId: null, versions: [{ versionId: documentVersionId, fileId, contentItemId: randomUUID(), representationId: randomUUID(), filename: file.name, content: await file.text(), title: metadata.title, metadata: metadata.versionMetadata, versionNo: 1, revisionId: randomUUID(), lifecycleState: 'working' }] });
      value = { documentId, documentVersionId, fileId };
      status = 201;
    } else if (path.match(/^\/v1\/documents\/[^/]+\/versions$/)) {
      const wire = await request.text();
      const json = JSON.parse(wire.split('Content-Type: application/json\r\n\r\n')[1].split('\r\n')[0]);
      const prior = operations.get(json.operationId);
      if (prior) { assert.deepEqual(prior.body, json); return response(prior.result, 201); }
      const doc = documents.get(path.split('/')[3]);
      const content = wire.split('X-Part-Id: primary\r\n\r\n')[1].split('\r\n--')[0];
      doc.versions.push({ versionId: json.targetVersionId, fileId: json.items[0].fileId, contentItemId: randomUUID(), representationId: randomUUID(), filename: json.items[0].originalFilename, content, title: json.title, versionNo: 2, revisionId: randomUUID(), lifecycleState: 'working', metadata: {} });
      doc.revision++;
      value = { operationId: json.operationId, documentId: doc.documentId, targetVersionId: json.targetVersionId, resultingRevision: doc.revision };
      operations.set(json.operationId, { body: json, result: value, status: 201 });
      status = 201;
    } else if (path.endsWith(':publish')) {
      const doc = documents.get(path.split('/')[3]);
      const versionId = path.split('/')[5].split(':')[0];
      doc.versions.find(v => v.versionId === versionId).lifecycleState = 'published';
      doc.currentVersionId = versionId;
      doc.revision++;
      value = { publishOperationId: body.operationId, documentId: doc.documentId, documentVersionId: versionId, resultingDocumentRevision: doc.revision };
    } else throw new Error(`Unknown fixture ${method} ${path}`);
    if (body?.operationId) operations.set(body.operationId, { body, result: value, status });
    if (loseResponse?.({ method, path })) { loseResponse = undefined; throw new TypeError('simulated lost response after committed mutation'); }
    return response(value, status);
  };
  return { fetch: fetcher, rootId, folders, policies, documents, operations, requests,
    loseNextResponse(predicate) { loseResponse = predicate; },
    setActor(value) { actor = value; },
    mutations() { return requests.filter(r => r.method !== 'GET'); },
  };
}
