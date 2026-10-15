/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const documentId = '019a0010-0000-7000-8000-000000000041';
const versionId = '019a0010-0000-7000-8000-000000000042';

test('既存history listは100件とopaque cursorを送り、detail/filesも明示purposeを保持する', async () => {
  const cursor = 'opaque+/=?日本語'; const first = { items: [], nextCursor: cursor }, last = { items: [], nextCursor: null };
  const fetcher = jest.fn().mockResolvedValueOnce(Response.json(first)).mockResolvedValueOnce(Response.json(last)).mockResolvedValueOnce(Response.json({ versionId })).mockResolvedValueOnce(Response.json({ items: [] }));
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(await documentApi.listDocumentVersions(documentId, 'history')).toEqual(first);
  expect(await documentApi.listDocumentVersions(documentId, 'history', cursor)).toEqual(last);
  expect(await documentApi.getDocumentVersion(documentId, versionId, 'history')).toEqual({ versionId });
  expect(await documentApi.listVersionFiles(documentId, versionId, 'history')).toEqual({ items: [] });
  const requests = fetcher.mock.calls.map(([request]: [Request]) => request); const urls = requests.map(request => new URL(request.url));
  expect(requests.map(request => request.method)).toEqual(['GET', 'GET', 'GET', 'GET']);
  expect([...urls[0]!.searchParams.entries()]).toEqual([['purpose', 'history'], ['pageSize', '100']]);
  expect([...urls[1]!.searchParams.entries()]).toEqual([['purpose', 'history'], ['pageSize', '100'], ['cursor', cursor]]);
  expect(urls[2]!.pathname).toBe(`/v1/documents/${documentId}/versions/${versionId}`); expect(urls[2]!.search).toBe('?purpose=history');
  expect(urls[3]!.pathname).toBe(`/v1/documents/${documentId}/versions/${versionId}/files`); expect(urls[3]!.search).toBe('?purpose=history');
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_NOT_FOUND', 404], ['DEPENDENCY_UNAVAILABLE', 503]])('既存historyの%sを空結果に変えず伝播する', async (code, status) => {
  const problem = { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: status === 503 };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: Number(status) }) });
  await expect(documentApi.listDocumentVersions(documentId, 'history', 'server-cursor')).rejects.toEqual(problem);
  await expect(documentApi.getDocumentVersion(documentId, versionId, 'history')).rejects.toEqual(problem);
  await expect(documentApi.listVersionFiles(documentId, versionId, 'history')).rejects.toEqual(problem);
});

test('既存原本wrapperは明示4ID/historyのGETとAbortSignalをbinary transportへ渡す', async () => {
  const location = Object.getOwnPropertyDescriptor(globalThis, 'location');
  Object.defineProperty(globalThis, 'location', { configurable: true, value: { origin: 'https://synthetic.invalid' } });
  const fetcher = jest.fn().mockResolvedValueOnce(new Response('original', { headers: { 'Content-Type': 'application/pdf' } }));
  jest.spyOn(globalThis, 'fetch').mockImplementation(fetcher);
  let binaryApi!: typeof documentApi;
  jest.isolateModules(() => { binaryApi = require('../src/api/document-api').documentApi; });
  try {
    const input = { documentId, versionId, contentItemId: 'item/日本語', representationId: 'rep+?', purpose: 'history' as const };
    const controller = new AbortController(); const blob = await binaryApi.downloadVersionFile(input, { signal: controller.signal });
    expect(await blob.text()).toBe('original');
    const [url, options] = fetcher.mock.calls[0]!;
    expect(String(url)).toBe(`https://synthetic.invalid/v1/documents/${documentId}/versions/${versionId}/files/item%2F%E6%97%A5%E6%9C%AC%E8%AA%9E/rep%2B%3F?purpose=history`);
    expect(options).toMatchObject({ method: 'GET', credentials: 'same-origin', signal: expect.any(AbortSignal) });
    fetcher.mockImplementationOnce(() => new Promise(() => undefined));
    const pending = binaryApi.downloadVersionFile(input, { signal: controller.signal }); controller.abort();
    await expect(pending).rejects.toThrow('cancelled/aborted'); expect(fetcher.mock.calls[1]![1].signal.aborted).toBe(true);
  } finally { if (location) Object.defineProperty(globalThis, 'location', location); else Reflect.deleteProperty(globalThis, 'location'); }
});
