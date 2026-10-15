/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const documentId = '019a0010-0000-7000-8000-000000000041';
const pair = { baseVersionId: '019a0010-0000-7000-8000-000000000042', targetVersionId: '019a0010-0000-7000-8000-000000000043', profile: 'document-diff-v0' as const, projection: 'display' as const, pageSize: 50 };

test('WORKING内容比較display POSTは固定pair/50とopaque cursorを変更せず、null/省略の終端を返す', async () => {
  const cursor = 'opaque+/=?日本語';
  const first = { items: [], unverifiedRegions: [], nextCursor: cursor };
  const last = { items: [], unverifiedRegions: [], nextCursor: null };
  const omitted = { items: [], unverifiedRegions: [] };
  const fetcher = jest.fn().mockResolvedValueOnce(Response.json(first)).mockResolvedValueOnce(Response.json(last)).mockResolvedValueOnce(Response.json(omitted));
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(await documentApi.compareDocumentVersions(documentId, pair)).toEqual(first);
  expect(await documentApi.compareDocumentVersions(documentId, { ...pair, cursor })).toEqual(last);
  expect(await documentApi.compareDocumentVersions(documentId, pair)).toEqual(omitted);
  const requests = fetcher.mock.calls.map(([request]: [Request]) => request);
  for (const request of requests) {
    expect(request.method).toBe('POST');
    expect(new URL(request.url).pathname).toBe(`/v1/documents/${documentId}/comparisons`);
    expect(new URL(request.url).search).toBe('');
  }
  expect(await requests[0]!.json()).toEqual(pair);
  expect(await requests[1]!.json()).toEqual({ ...pair, cursor });
  expect(await requests[2]!.json()).toEqual(pair);
});

test.each([['CURSOR_STALE', 409], ['STALE_COMPARISON_INPUT', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_VERSION_NOT_FOUND', 404], ['DEPENDENCY_UNAVAILABLE', 503]])('Version比較の%sを空の末尾へ変換せず伝播する', async (code, status) => {
  const problem = { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: status === 503 };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: Number(status) }) });
  await expect(documentApi.compareDocumentVersions(documentId, { ...pair, cursor: 'server-cursor' })).rejects.toEqual(problem);
});
