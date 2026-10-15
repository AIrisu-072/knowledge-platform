/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const documentId = '019a0010-0000-7000-8000-000000000041';

test('正式改訂の先頭はcursor無し、続きはopaque cursorを変更せずpageSize100で送る', async () => {
  const cursor = 'opaque+/=?日本語';
  const first = { items: [], nextCursor: cursor }; const last = { items: [], nextCursor: null };
  const fetcher = jest.fn().mockResolvedValueOnce(Response.json(first)).mockResolvedValueOnce(Response.json(last));
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  const result = await documentApi.listDocumentRevisions(documentId);
  expect(result).toEqual(first);
  expect(await documentApi.listDocumentRevisions(documentId, result.nextCursor!)).toEqual(last);
  const [initial, next] = fetcher.mock.calls.map(([request]: [Request]) => new URL(request.url));
  expect(initial!.pathname).toBe(`/v1/documents/${documentId}/revisions`);
  expect([...initial!.searchParams.entries()]).toEqual([['pageSize', '100']]);
  expect([...next!.searchParams.entries()]).toEqual([['pageSize', '100'], ['cursor', cursor]]);
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_NOT_FOUND', 404]])('正式改訂の%sを空の末尾へ変換せず伝播する', async (code, status) => {
  const problem = { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: Number(status) }) });
  await expect(documentApi.listDocumentRevisions(documentId, 'server-cursor')).rejects.toEqual(problem);
});
