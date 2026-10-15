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
  assert.equal(operationIds.length, 38);
});

test('本人既読の新GET/VIEW/RESETと旧PUTのSDK契約を保持する', async () => {
  const sdk = await readFile(new URL('packages/document-api-client/src/generated/sdk.gen.ts', repositoryRoot), 'utf8');
  for (const name of ['getCurrentDocumentVersionReadState', 'recordDocumentVersionView', 'resetDocumentVersionReadState', 'markDocumentVersionRead']) {
    assert.match(sdk, new RegExp('export const ' + name + '\\b'));
  }
  const types = await readFile(new URL('packages/document-api-client/src/generated/types.gen.ts', repositoryRoot), 'utf8');
  for (const name of ['CurrentReadState', 'ReadStateMutationResult', 'ReadStateMutationRequest']) {
    assert.match(types, new RegExp('export type ' + name + '\\b'));
  }
});
