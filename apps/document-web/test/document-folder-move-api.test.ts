/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const targetId = '019a0010-0000-7000-8000-000000000002';
const body = { operationId: '019a0010-0000-7000-8000-000000000001', fromParentId: 'parent', toParentId: 'destination', expectedFolderRevision: 8, reason: '合成理由' };
test('既存SDKのmove POSTへ5項目だけ送信しreceiptを返す', async () => {
  const result = { operationId: body.operationId, resourceId: targetId, resultingRevision: 9, changed: true, occurredAt: '2026-10-06T07:00:00Z' };
  const fetcher = jest.fn(async () => Response.json(result)); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(documentApi).toHaveProperty('moveFolder', expect.any(Function)); expect(await documentApi.moveFolder(targetId, body)).toEqual(result);
  const request = (fetcher.mock.calls as unknown as [Request][])[0]![0]; expect(request.method).toBe('POST'); expect(request.url).toBe(`https://synthetic.invalid/v1/folders/${targetId}:move`); expect(await request.json()).toEqual(body);
});
test('moveの正規Problemを呼出側へthrowする', async () => {
  const problem = { type: 'about:blank', title: 'Synthetic', status: 409, code: 'FOLDER_CYCLE', traceId: 'synthetic', retryable: false };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: 409 }) });
  await expect(documentApi.moveFolder(targetId, body)).rejects.toEqual(problem);
});
