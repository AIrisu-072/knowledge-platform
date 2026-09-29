import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';

const repository = resolve(import.meta.dirname, '../..');
const temporary = mkdtempSync(join(tmpdir(), 'document-api-contract-'));
let contract;
try {
  const bundled = join(temporary, 'openapi.json');
  execFileSync(process.execPath, [
    join(repository, 'node_modules/@redocly/cli/bin/cli.js'),
    'bundle', join(repository, 'spec/api/openapi.yaml'),
    '--ext', 'json', '--output', bundled,
  ], { cwd: repository, stdio: 'pipe' });
  contract = JSON.parse(readFileSync(bundled, 'utf8'));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

const operations = [
  ['get', '/v1/documents'],
  ['post', '/v1/documents'],
  ['get', '/v1/documents/{documentId}'],
  ['get', '/v1/documents/{documentId}/versions'],
  ['post', '/v1/documents/{documentId}/versions'],
  ['get', '/v1/documents/{documentId}/versions/{versionId}'],
  ['put', '/v1/documents/{documentId}/versions/{versionId}'],
  ['post', '/v1/documents/{documentId}/versions/{versionId}:rebase'],
  ['post', '/v1/documents/{documentId}/versions/{versionId}:publish'],
  ['post', '/v1/documents/{documentId}/versions/{versionId}:withdraw'],
  ['post', '/v1/documents/{documentId}/versions/{versionId}:schedule-publication'],
  ['post', '/v1/documents/{documentId}/versions/{versionId}:cancel-publication-schedule'],
  ['post', '/v1/documents/{documentId}:end-publication'],
  ['patch', '/v1/documents/{documentId}/metadata'],
  ['post', '/v1/documents/{documentId}:move'],
  ['get', '/v1/documents/{documentId}/access-policy'],
  ['put', '/v1/documents/{documentId}/access-policy'],
  ['put', '/v1/documents/{documentId}/versions/{versionId}/read-state'],
  ['get', '/v1/documents/{documentId}/history'],
  ['get', '/v1/documents/{documentId}/versions/{versionId}/files'],
  ['get', '/v1/documents/{documentId}/versions/{versionId}/files/{contentItemId}/{representationId}'],
  ['post', '/v1/documents/{documentId}/comparisons'],
  ['get', '/v1/folders/root'],
  ['get', '/v1/folders/{folderId}/children'],
  ['post', '/v1/folders'],
  ['patch', '/v1/folders/{folderId}'],
  ['post', '/v1/folders/{folderId}:move'],
  ['get', '/v1/folders/{folderId}/access-policy'],
  ['put', '/v1/folders/{folderId}/access-policy'],
];

function operation(method, path) {
  return contract.paths?.[path]?.[method];
}

test('OpenAPI 3.2.1 exposes every approved Document operation with unique IDs', () => {
  assert.equal(contract.openapi, '3.2.1');
  const ids = [];
  for (const [method, path] of operations) {
    const endpoint = operation(method, path);
    assert.ok(endpoint, `missing ${method.toUpperCase()} ${path}`);
    assert.match(endpoint.operationId ?? '', /^[a-z][A-Za-z0-9]+$/);
    ids.push(endpoint.operationId);
  }
  assert.equal(new Set(ids).size, ids.length, 'operationId collision');
});

test('the contract describes multipart writes and audited binary download', () => {
  for (const path of ['/v1/documents', '/v1/documents/{documentId}/versions']) {
    assert.ok(operation('post', path)?.requestBody?.content?.['multipart/form-data'], path);
  }
  const download = operation('get', '/v1/documents/{documentId}/versions/{versionId}/files/{contentItemId}/{representationId}');
  assert.equal(download?.responses?.['200']?.content?.['application/octet-stream']?.schema?.format, 'binary');
  assert.ok(download?.parameters?.some((parameter) => parameter.name === 'purpose'));
});

test('machine error registry and RFC 9457 extension are present', () => {
  const registry = readFileSync(join(repository, 'spec/errors/error-registry.yaml'), 'utf8');
  for (const code of [
    'VALIDATION_FAILED', 'AUTHENTICATION_REQUIRED', 'FORBIDDEN', 'DOCUMENT_NOT_FOUND',
    'REVISION_CONFLICT', 'OPERATION_CONFLICT', 'CURSOR_STALE', 'STALE_VERSION',
    'STALE_COMPARISON_INPUT', 'BUSINESS_RULE_REJECTED', 'RESERVED_DOCUMENT',
    'FOLDER_CYCLE', 'ROOT_PROTECTED', 'IDENTITY_UNAVAILABLE', 'PUBLISH_QUALITY_REJECTED',
    'UNSUPPORTED_MEDIA_TYPE', 'DEPENDENCY_UNAVAILABLE', 'TIMEOUT',
    'COMMIT_OUTCOME_UNKNOWN', 'INTEGRITY_VIOLATION', 'INTERNAL',
  ]) {
    assert.match(registry, new RegExp(`\\b${code}\\b`));
  }
  const problem = contract.components?.schemas?.Problem;
  assert.ok(problem?.required?.includes('code'));
  assert.ok(problem?.required?.includes('retryable'));
  assert.ok(problem?.properties?.errors?.items);
});

test('comparison coverage, cursor, examples and nullable policy binding survive bundling', () => {
  const comparison = operation('post', '/v1/documents/{documentId}/comparisons');
  assert.ok(comparison?.responses?.['200']?.content?.['application/json']?.schema?.oneOf);
  assert.ok(contract.components?.schemas?.ComparisonCoverage);
  const list = operation('get', '/v1/documents');
  assert.ok(list?.parameters?.some((parameter) => parameter.name === 'cursor'));
  assert.ok(list?.responses?.['200']?.content?.['application/json']?.example);
  assert.ok(contract.components?.schemas?.AccessPolicyRead?.properties?.policyId?.type?.includes('null'));
});
