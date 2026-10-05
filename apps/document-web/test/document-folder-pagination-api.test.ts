/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';

// Use the actual typed adapter and generated SDK; only the fetch boundary is in memory.
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const folderId = '019a0010-0000-7000-8000-000000000041';

test('childrenの先頭はcursor無し、続きはサーバー文字列を変更せずpageSize200で送る', async () => {
  const cursor = 'opaque+/=?日本語';
  const first = { items: [], nextCursor: cursor, capabilities: {} };
  const last = { items: [], nextCursor: null, capabilities: {} };
  const fetcher = jest.fn().mockResolvedValueOnce(Response.json(first)).mockResolvedValueOnce(Response.json(last));
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  const result = await documentApi.listFolderChildren(folderId);
  expect(result).toEqual(first);
  expect(await documentApi.listFolderChildren(folderId, result.nextCursor!)).toEqual(last);
  const [initial, next] = fetcher.mock.calls.map(([request]: [Request]) => new URL(request.url));
  expect(initial!.pathname).toBe(`/v1/folders/${folderId}/children`);
  expect([...initial!.searchParams.entries()]).toEqual([['pageSize', '200']]);
  expect([...next!.searchParams.entries()]).toEqual([['pageSize', '200'], ['cursor', cursor]]);
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403]])('childrenの%sを空の末尾へ変換せず伝播する', async (code, status) => {
  const problem = { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: Number(status) }) });
  await expect(documentApi.listFolderChildren(folderId, 'server-cursor')).rejects.toEqual(problem);
});
