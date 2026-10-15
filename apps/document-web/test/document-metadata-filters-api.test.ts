/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';

jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });

test.each([
  { documentType: '   ' }, { owningDepartment: 'e\u0301' }, { category: 'null' },
  { documentType: '123', owningDepartment: 'true', category: '[1]' },
  { documentType: 'a'.repeat(1024), owningDepartment: '日'.repeat(341) + 'a', category: '😀'.repeat(256) },
  { documentType: 'quote"\\+/?%_', owningDepartment: 'Case', category: 'é' },
])('existing list SDK serializes exact metadata query text %p', async metadata => {
  const fetcher = jest.fn().mockResolvedValue(Response.json({ view: 'authoring', items: [], nextCursor: null }));
  client.setConfig({ baseUrl: 'http://synthetic.invalid', fetch: fetcher });
  await documentApi.listDocuments({ view: 'authoring', sort: 'title_asc', pageSize: 25, titleContains: 'keep', ...metadata });
  const request = fetcher.mock.calls[0]![0] as Request;
  expect(request.method).toBe('GET');
  const url = new URL(request.url);
  expect(url.pathname).toBe('/v1/documents');
  expect(url.searchParams.get('sort')).toBe('titleAsc');
  expect(url.searchParams.get('titleContains')).toBe('keep');
  for (const key of ['documentType', 'owningDepartment', 'category'] as const) {
    expect(url.searchParams.get(key)).toBe(metadata[key as keyof typeof metadata] ?? null);
  }
});

test.each([true, undefined])('existing list SDK sends only explicitly applied unread true: %p', async unreadOnly => {
  const fetcher = jest.fn().mockResolvedValue(Response.json({ view: 'published', items: [], nextCursor: null }));
  client.setConfig({ baseUrl: 'http://synthetic.invalid', fetch: fetcher });
  await documentApi.listDocuments({ view: 'published', ...(unreadOnly === true ? { unreadOnly: true } : {}) });
  const request = fetcher.mock.calls[0]![0] as Request;
  expect(request.method).toBe('GET');
  const url = new URL(request.url);
  expect(url.searchParams.get('view')).toBe('published');
  expect(url.searchParams.get('unreadOnly')).toBe(unreadOnly === true ? 'true' : null);
});


test.each([
  { createdFrom: '2026-10-01T00:00:00.000Z' },
  { createdBefore: '2026-10-01t00:00:00z' },
  { createdFrom: '2016-12-31T23:59:60Z', createdBefore: '2026-10-01T00:00:00.123456+09:00' },
])('validated GUI created range reaches existing SDK GET as exact raw text: %p', async range => {
  const { validateListSearch } = await import('../src/application/search-state');
  const fetcher = jest.fn().mockResolvedValue(Response.json({ view: 'history', items: [], nextCursor: null }));
  client.setConfig({ baseUrl: 'http://synthetic.invalid', fetch: fetcher });
  await documentApi.listDocuments(validateListSearch({ view: 'history', ...range, documentType: 'keep' }));
  const request = fetcher.mock.calls[0]![0] as Request;
  expect(request.method).toBe('GET');
  const url = new URL(request.url);
  for (const key of ['createdFrom', 'createdBefore'] as const) expect(url.searchParams.get(key)).toBe(range[key as keyof typeof range] ?? null);
  expect(url.searchParams.get('documentType')).toBe('keep');
});
