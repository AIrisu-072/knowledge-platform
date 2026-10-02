import assert from 'node:assert/strict';
import test from 'node:test';

import {
  BinaryTransportBridge,
  BinaryTransportError,
  DocumentApiProblemError,
} from '../.test-build/binary-transport.js';

const documentId = '00000000-0000-4000-8000-000000000001';
const versionId = '00000000-0000-4000-8000-000000000002';
const fileId = '00000000-0000-4000-8000-000000000003';
const renditionId = '00000000-0000-4000-8000-000000000004';
const operationId = '01890f7a-6f6e-7b0a-8000-000000000013';

function jsonResponse(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

test('initial document upload maps the generated JSON request and binary file to FormData', async () => {
  let captured;
  const bridge = new BinaryTransportBridge({
    baseUrl: 'https://documents.test',
    fetch: async (url, init) => {
      captured = { url: String(url), init };
      return jsonResponse({ documentId, documentVersionId: versionId, fileId }, 201);
    },
  });
  const request = {
    folderId: documentId,
    title: 'Initial document',
    documentMetadata: { category: 'policy' },
    versionMetadata: { source: 'upload' },
  };
  const file = new Blob(['initial bytes'], { type: 'text/plain' });

  const result = await bridge.createDocument({
    request,
    file,
    originalFilename: 'policy.txt',
    mediaType: 'text/plain',
  });

  assert.deepEqual(result, { documentId, documentVersionId: versionId, fileId });
  assert.equal(captured.url, 'https://documents.test/v1/documents');
  assert.equal(captured.init.method, 'POST');
  assert.ok(captured.init.body instanceof FormData);
  assert.equal(captured.init.headers?.['Content-Type'], undefined);
  assert.deepEqual(JSON.parse(await captured.init.body.get('request').text()), request);
  assert.equal(captured.init.body.get('file').name, 'policy.txt');
  assert.equal(captured.init.body.get('file').type, 'text/plain');
  assert.equal(await captured.init.body.get('file').text(), 'initial bytes');
});

test('version uploads bind manifest part IDs to bounded multipart file parts', async () => {
  let captured;
  const bridge = new BinaryTransportBridge({
    baseUrl: 'https://documents.test/api',
    fetch: async (url, init) => {
      captured = { url: String(url), init };
      return jsonResponse({ operationId, documentId, targetVersionId: versionId, versionNo: 2, baseVersionId: versionId, resultingRevision: 4 });
    },
  });
  const request = {
    operationId,
    targetVersionId: versionId,
    expectedRevision: 3,
    title: 'Revised document',
    items: [{
      logicalPath: 'primary',
      ordinal: 0,
      fileId,
      partId: 'primary-file',
      mediaType: 'text/plain',
      originalFilename: 'revised.txt',
      renditions: [{
        fileId: renditionId,
        partId: 'rendition-file',
        mediaType: 'application/pdf',
        originalFilename: 'preview.pdf',
      }],
    }],
  };

  await bridge.createVersion(documentId, {
    request,
    files: new Map([
      ['primary-file', new Blob(['revised bytes'])],
      ['rendition-file', new Blob(['rendition bytes'])],
    ]),
  });

  assert.equal(captured.url, `https://documents.test/api/v1/documents/${documentId}/versions`);
  assert.equal(captured.init.method, 'POST');
  assert.ok(captured.init.body instanceof Blob);
  assert.match(captured.init.headers['Content-Type'], /^multipart\/form-data; boundary=/);
  const wireBody = await captured.init.body.text();
  assert.ok(wireBody.includes('name="request"'));
  assert.ok(wireBody.includes(JSON.stringify(request)));
  assert.ok(wireBody.includes('name="files"; filename="binary"'));
  assert.ok(wireBody.includes('X-Part-Id: primary-file\r\n'));
  assert.ok(wireBody.includes('X-Part-Id: rendition-file\r\n'));
  assert.ok(wireBody.includes('revised bytes'));
  assert.ok(wireBody.includes('rendition bytes'));

  await assert.rejects(
    bridge.createVersion(documentId, { request, files: new Map() }),
    (error) => error instanceof BinaryTransportError,
  );
  await assert.rejects(
    bridge.createVersion(documentId, {
      request,
      files: new Map([
        ['primary-file', new Blob()],
        ['rendition-file', new Blob()],
        ['extra-file', new Blob()],
      ]),
    }),
    (error) => error instanceof BinaryTransportError,
  );
});

test('working-version update uses the encoded document and version path', async () => {
  let captured;
  const bridge = new BinaryTransportBridge({
    baseUrl: 'https://documents.test',
    fetch: async (url, init) => {
      captured = { url: String(url), init };
      return jsonResponse({ operationId, documentId, targetVersionId: versionId, versionNo: 2, baseVersionId: versionId, resultingRevision: 4 });
    },
  });
  const request = {
    operationId,
    targetVersionId: versionId,
    expectedRevision: 3,
    title: 'Revised document',
    items: [{ logicalPath: 'primary', ordinal: 0, fileId, partId: 'body', mediaType: 'text/plain', originalFilename: 'file.txt' }],
  };

  await bridge.updateWorkingVersion(documentId, versionId, {
    request,
    files: new Map([['body', new Blob(['new'])]]),
  });

  assert.equal(captured.url, `https://documents.test/v1/documents/${documentId}/versions/${versionId}`);
  assert.equal(captured.init.method, 'PUT');
});

test('binary downloads expose Blob and ReadableStream results', async () => {
  const bridge = new BinaryTransportBridge({
    baseUrl: 'https://documents.test',
    fetch: async (url) => {
      assert.equal(String(url), `https://documents.test/v1/documents/${documentId}/versions/${versionId}/files/${fileId}/${fileId}?purpose=published`);
      return new Response(new Uint8Array([1, 2, 3]), {
        status: 200,
        headers: { 'content-type': 'application/octet-stream' },
      });
    },
  });
  const path = { documentId, versionId, contentItemId: fileId, representationId: fileId };

  const blob = await bridge.downloadVersionFileBlob({ ...path, purpose: 'published' });
  assert.deepEqual([...new Uint8Array(await blob.arrayBuffer())], [1, 2, 3]);

  const stream = await bridge.downloadVersionFileStream({ ...path, purpose: 'published' });
  const reader = stream.getReader();
  assert.deepEqual([...((await reader.read()).value ?? [])], [1, 2, 3]);
});

test('RFC 9457 failures preserve the generated stable problem contract', async () => {
  const problem = {
    type: 'about:blank',
    title: 'Validation failed',
    status: 422,
    code: 'VALIDATION_FAILED',
    traceId: 'trace-1',
    retryable: false,
  };
  const bridge = new BinaryTransportBridge({
    baseUrl: 'https://documents.test',
    fetch: async () => new Response(JSON.stringify(problem), {
      status: 422,
      headers: { 'content-type': 'application/problem+json' },
    }),
  });

  await assert.rejects(
    bridge.downloadVersionFileBlob({
      documentId,
      versionId,
      contentItemId: fileId,
      representationId: fileId,
      purpose: 'published',
    }),
    (error) => error instanceof DocumentApiProblemError
      && error.status === 422
      && error.problem.code === 'VALIDATION_FAILED'
      && error.problem.traceId === 'trace-1',
  );
});
