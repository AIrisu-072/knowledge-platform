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
