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
  ['get', '/v1/document-creation-outcomes/{documentId}'],
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

function resolved(value) {
  while (value?.$ref?.startsWith('#/')) {
    value = value.$ref.slice(2).split('/').reduce((current, key) => current?.[key.replaceAll('~1', '/').replaceAll('~0', '~')], contract);
  }
  return value;
}

test('OpenAPI 3.2.1 exposes every approved Document operation with unique IDs', () => {
  assert.equal(contract.openapi, '3.2.1');
  const ids = [];
  for (const [method, path] of operations) {
    const endpoint = operation(method, path);
    assert.ok(endpoint, `missing ${method.toUpperCase()} ${path}`);
    assert.match(endpoint.operationId ?? '', /^[a-z][A-Za-z0-9]+$/);
    assert.equal(resolved(endpoint.responses?.['401'])?.content?.['application/problem+json']?.schema?.$ref, '#/components/schemas/Problem');
    ids.push(endpoint.operationId);
  }
  assert.equal(new Set(ids).size, ids.length, 'operationId collision');
});

test('the contract describes multipart writes and audited binary download', () => {
  for (const path of ['/v1/documents', '/v1/documents/{documentId}/versions']) {
    assert.ok(resolved(operation('post', path)?.requestBody)?.content?.['multipart/form-data'], path);
  }
  const download = operation('get', '/v1/documents/{documentId}/versions/{versionId}/files/{contentItemId}/{representationId}');
  assert.equal(resolved(download?.responses?.['200'])?.content?.['application/octet-stream']?.schema?.format, 'binary');
  assert.ok(download?.parameters?.some((parameter) => resolved(parameter)?.name === 'purpose'));
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
  const problem = resolved(contract.components?.schemas?.Problem);
  assert.ok(problem?.required?.includes('code'));
  assert.ok(problem?.required?.includes('retryable'));
  assert.ok(problem?.properties?.errors?.items);
  assert.ok(problem?.properties?.recovery);
  assert.ok(problem?.properties?.exactRetry);
});

test('comparison coverage, cursor, examples and nullable policy binding survive bundling', () => {
  const comparison = operation('post', '/v1/documents/{documentId}/comparisons');
  assert.ok(resolved(resolved(comparison?.responses?.['200'])?.content?.['application/json']?.schema)?.oneOf);
  assert.deepEqual(resolved(contract.components?.schemas?.DiffProjection)?.properties?.coverage?.enum, ['full', 'partial', 'none']);
  const list = operation('get', '/v1/documents');
  assert.ok(list?.parameters?.some((parameter) => resolved(parameter)?.name === 'cursor'));
  assert.ok(resolved(list?.responses?.['200'])?.content?.['application/json']?.example);
  assert.ok(resolved(contract.components?.schemas?.AccessPolicyRead)?.properties?.policyId?.type?.includes('null'));
});

test('write contracts bind replay IDs and do not accept actor self assertions', () => {
  const version = resolved(contract.components?.schemas?.VersionWrite);
  assert.ok(version.required.includes('operationId'));
  assert.ok(version.required.includes('targetVersionId'));
  assert.ok(version.required.includes('expectedRevision'));
  assert.ok(version.required.includes('title'));
  const versionItem = resolved(version.properties.items.items);
  assert.equal(resolved(versionItem.properties.renditions.items).required.includes('partId'), true);
  for (const path of ['/v1/documents/{documentId}/versions', '/v1/documents/{documentId}/versions/{versionId}', '/v1/documents/{documentId}/versions/{versionId}:rebase']) {
    const method = path.endsWith('/versions') ? 'post' : path.endsWith(':rebase') ? 'post' : 'put';
    const success = path.endsWith('/versions') ? '201' : '200';
    assert.equal(
      resolved(operation(method, path).responses[success]).content['application/json'].schema.$ref,
      '#/components/schemas/VersionMutationResult',
    );
  }
  const policy = resolved(contract.components?.schemas?.SetAccessPolicy);
  assert.equal(policy.oneOf.length, 2);
  assert.equal(resolved(policy.oneOf[0]).properties.mode.const, 'inherit');
  assert.equal(resolved(policy.oneOf[1]).properties.mode.const, 'explicit');

  const forbidden = new Set(['principal', 'actor', 'invocationKind', 'serviceExecutor', 'delegation']);
  const visited = new Set();
  function walk(value) {
    if (!value || typeof value !== 'object' || visited.has(value)) return;
    visited.add(value);
    if (value.properties) {
      for (const name of Object.keys(value.properties)) {
        assert.ok(!forbidden.has(name), `actor self assertion field ${name}`);
      }
    }
    for (const child of Object.values(value)) {
      if (Array.isArray(child)) child.forEach(walk);
      else walk(resolved(child));
    }
  }
  for (const body of Object.values(contract.components.requestBodies)) walk(resolved(body));
});

test('Diff source locators and comparison rows keep their evidence structure', () => {
  const locator = resolved(contract.components?.schemas?.SourceLocator);
  assert.ok(locator, 'SourceLocator schema is required');
  assert.equal(locator.oneOf.length, 9);
  assert.ok(locator.oneOf.every((variant) => variant.properties?.kind?.const));
  const change = resolved(contract.components?.schemas?.DiffProjection).properties.changes.items;
  assert.equal(change.$ref, '#/components/schemas/DiffChange');
  const row = resolved(contract.components?.schemas?.ComparisonTableProjection).properties.rows.items;
  assert.equal(row.$ref, '#/components/schemas/ComparisonRow');
  assert.deepEqual(resolved(contract.components?.schemas?.UnverifiedRegion).properties.reason.enum, [
    'unsupportedSemanticConstruct', 'corruptedSource', 'missingInspectionEvidence',
    'ambiguousAlignment', 'resourceLimit',
  ]);
});

test('read confirmation exposes its recorded timestamp without echoing the actor', () => {
  const result = resolved(contract.components?.schemas?.ReadStateResult);
  assert.ok(result.required.includes('firstReadAt'));
  assert.ok(result.required.includes('inserted'));
  assert.equal(result.properties.principal, undefined);
});

test('initial create recovery requires the complete generated identity tuple', () => {
  const recovery = operation('get', '/v1/document-creation-outcomes/{documentId}');
  const parameters = [
    ...(contract.paths['/v1/document-creation-outcomes/{documentId}'].parameters ?? []),
    ...(recovery.parameters ?? []),
  ].map(resolved);
  for (const name of ['documentId', 'documentVersionId', 'fileId']) {
    const parameter = parameters.find((candidate) => candidate.name === name);
    assert.ok(parameter, `create recovery missing ${name}`);
    assert.equal(parameter.required, true);
  }
  assert.equal(
    resolved(recovery.responses['200']).content['application/json'].schema.$ref,
    '#/components/schemas/CreateDocumentResult',
  );
});

test('read projections preserve authorized Application fields and pagination', () => {
  const published = resolved(contract.components.schemas.PublishedDocument);
  assert.ok(published.properties.folderId.type.includes('null'), 'hidden folder cannot be forced into a UUID');
  const authoring = resolved(contract.components.schemas.AuthoringDocument);
  assert.ok(authoring.required.includes('documentVersionId'));
  assert.ok(authoring.required.includes('lifecycleState'));
  assert.equal(authoring.properties.versions, undefined, 'a list row is not a complete version list');

  const list = resolved(contract.components.schemas.DocumentList);
  const historyItems = resolved(list.oneOf[2]).properties.items.items;
  assert.equal(historyItems.$ref, '#/components/schemas/HistoryDocument');
  const version = resolved(contract.components.schemas.Version);
  assert.ok(version.required.includes('versionNo'));
  assert.ok(version.required.includes('isCurrent'));
  assert.equal(version.properties.revision, undefined, 'Version has no version revision');
  const history = resolved(contract.components.schemas.History);
  assert.ok(history.required.includes('nextCursor'));
  const event = resolved(history.properties.items.items);
  assert.ok(event.required.includes('sourceKey'));
  assert.ok(event.properties.occurredAt.type.includes('null'));
  assert.ok(resolved(contract.components.schemas.VersionList).required.includes('nextCursor'));
  assert.ok(resolved(contract.components.schemas.FolderChildren).required.includes('nextCursor'));

  const policy = resolved(contract.components.schemas.AccessPolicyRead);
  assert.equal(policy.properties.effectiveSource.$ref, '#/components/schemas/PolicyTarget');
  const policyExample = resolved(contract.components.responses.AccessPolicyRead).content['application/json'].example;
  assert.equal(typeof policyExample.target, 'object');
  assert.equal(typeof policyExample.effectiveSource, 'object');
  for (const path of ['/v1/documents/{documentId}/versions', '/v1/documents/{documentId}/history', '/v1/folders/{folderId}/children']) {
    const parameters = operation('get', path).parameters.map((parameter) => resolved(parameter).name);
    assert.ok(parameters.includes('pageSize'), `${path} missing pageSize`);
    assert.ok(parameters.includes('cursor'), `${path} missing cursor`);
  }
});
