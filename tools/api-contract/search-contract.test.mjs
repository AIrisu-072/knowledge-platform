import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';

const repository = resolve(import.meta.dirname, '../..');
const temporary = mkdtempSync(join(tmpdir(), 'search-api-contract-'));
let contract;
try {
  const bundled = join(temporary, 'search-openapi.json');
  execFileSync(process.execPath, [
    join(repository, 'node_modules/@redocly/cli/bin/cli.js'),
    'bundle', join(repository, 'spec/api/search-openapi.yaml'),
    '--ext', 'json', '--output', bundled,
  ], { cwd: repository, stdio: 'pipe' });
  contract = JSON.parse(readFileSync(bundled, 'utf8'));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

const operations = [
  ['post', '/v1/search', 'search'],
  ['post', '/v1/discover', 'discover'],
  ['get', '/v1/resources/{resourceId}', 'getResource'],
  ['get', '/v1/sources', 'listSources'],
];
function resolved(value) {
  if (!value?.$ref) return value;
  assert.ok(value.$ref.startsWith('#/'), value.$ref);
  return resolved(value.$ref.slice(2).split('/').reduce((node, key) => node?.[key], contract));
}

test('Search contract contains only its four Search operations', () => {
  assert.deepEqual(Object.keys(contract.paths).sort(), operations.map(([, path]) => path).sort());
  for (const [method, path, id] of operations) {
    const operation = contract.paths[path][method];
    assert.equal(operation.operationId, id);
    assert.deepEqual(operation.security, [{ SearchBearer: [] }]);
  }
  assert.equal(contract.components.securitySchemes.SearchBearer.scheme, 'bearer');
});

test('Search preserves its own problem spelling and closed request fields', () => {
  const problem = contract.components.schemas.Problem;
  assert.equal(problem.additionalProperties, false);
  assert.ok(problem.required.includes('trace_id'));
  assert.equal(problem.properties.traceId, undefined);
  assert.equal(problem.properties.type.const, 'about:blank');
  assert.deepEqual(Object.keys(contract.components.schemas.SearchQuery.properties).sort(),
    ['query', 'resourceTypes', 'sourceIds', 'coverage', 'pageSize', 'cursor'].sort());
  assert.equal(contract.components.schemas.SearchQuery.additionalProperties, false);
  assert.deepEqual(Object.keys(contract.components.schemas.DiscoveryInput.properties).sort(),
    ['need', 'query', 'coverage', 'graph'].sort());
  assert.equal(contract.components.schemas.DiscoveryInput.additionalProperties, false);
});

test('Search success and problem responses retain private transport headers', () => {
  for (const [method, path] of operations) {
    const responses = contract.paths[path][method].responses;
    assert.ok(responses['200']);
    assert.ok(responses['401']);
    for (const [status, reference] of Object.entries(responses)) {
      const response = resolved(reference);
      assert.equal(resolved(response.headers['Cache-Control']).schema.const, 'private, no-store');
      assert.equal(resolved(response.headers['X-Content-Type-Options']).schema.const, 'nosniff');
      assert.ok(response.content[status === '200' ? 'application/json' : 'application/problem+json']);
    }
    assert.equal(resolved(resolved(responses['401']).headers['WWW-Authenticate']).schema.const,
      'Bearer realm="search"');
  }
});
