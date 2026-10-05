/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';

// Exercise the real generated SDK and typed-client serialization without a listener or network.
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const body = { operationId: '019a0010-0000-7000-8000-000000000001', folderId: '019a0010-0000-7000-8000-000000000002', parentFolderId: '019a0010-0000-7000-8000-000000000003', expectedParentRevision: 17, name: 'é資料', reason: '合成理由' };

test('既存typed SDKのPOST /v1/foldersで全固定payloadをJSON送信しMutationResultのみ返す', async () => {
  const result = { operationId: body.operationId, resourceId: body.folderId, resultingRevision: 0, changed: true, occurredAt: '2026-10-05T12:00:00Z' };
  const fetcher = jest.fn(async () => Response.json(result, { status: 201 })); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(documentApi).toHaveProperty('createFolder', expect.any(Function));
  expect(await documentApi.createFolder(body)).toEqual(result);
  const request = (fetcher.mock.calls as unknown as [Request][])[0]![0];
  expect(request.method).toBe('POST'); expect(request.url).toBe('https://synthetic.invalid/v1/folders'); expect(request.headers.get('content-type')).toBe('application/json');
  expect(await request.json()).toEqual(body);
});

test('既存typed SDKのProblemはthrowOnErrorで呼出元へ伝播する', async () => {
  const problem = { type: 'about:blank', title: 'Synthetic', status: 409, code: 'REVISION_CONFLICT', traceId: 'synthetic', retryable: false };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: 409 }) });
  await expect(documentApi.createFolder(body)).rejects.toEqual(problem);
});
