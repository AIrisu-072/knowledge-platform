import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const repositoryRoot = new URL('../../../', import.meta.url);

test('generated SDK operations match every OpenAPI operationId', async () => {
  const [openapi, sdk] = await Promise.all([
    readFile(new URL('spec/api/openapi.yaml', repositoryRoot), 'utf8'),
    readFile(new URL('packages/document-api-client/src/generated/sdk.gen.ts', repositoryRoot), 'utf8'),
  ]);
  const operationIds = [...new Set(
    [...openapi.matchAll(/^\s+operationId:\s*([A-Za-z][A-Za-z0-9]*)\s*$/gm)]
      .map((match) => match[1]),
  )].sort();
  const generatedOperations = [...new Set(
    [...sdk.matchAll(/^export const (\w+)/gm)].map((match) => match[1]),
  )].sort();

  assert.deepEqual(generatedOperations, operationIds);
  assert.equal(operationIds.length, 34);
});
