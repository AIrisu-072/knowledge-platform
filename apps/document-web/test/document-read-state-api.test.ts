/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const documentId = '019a0107-0000-7000-8000-000000000010';
const versionId = '019a0107-0000-7000-8000-000000000011';
const body = { operationId: '019a0107-0000-7000-8000-000000000020', expectedReadStateRevision: 1 };
const state = { documentId, versionId, firstReadAt: '2026-10-07T00:00:00Z', needsRecheck: false, readStateRevision: 1, isRead: true };
test('current_state_get_uses_fixed_version_and_real_abort_signal', async () => {
  expect(documentApi).toHaveProperty('getCurrentDocumentVersionReadState', expect.any(Function));
  const fetcher = jest.fn(async () => Response.json(state)); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher }); const controller = new AbortController();
  expect(await documentApi.getCurrentDocumentVersionReadState(documentId, versionId, { signal: controller.signal })).toEqual(state);
  const request = (fetcher.mock.calls as unknown as [Request][])[0]![0]; expect(request.method).toBe('GET'); expect(request.url).toBe(`https://synthetic.invalid/v1/documents/${documentId}/versions/${versionId}/read-state`); controller.abort(); expect(request.signal.aborted).toBe(true);
});
test.each(['VIEW', 'RESET'] as const)('%s_post_uses_only_two_fixed_body_fields_on_its_exact_path', async kind => {
  const name = kind === 'VIEW' ? 'recordDocumentVersionView' : 'resetDocumentVersionReadState'; expect(documentApi).toHaveProperty(name, expect.any(Function));
  const result = { operationId: body.operationId, documentId, versionId, kind, expectedReadStateRevision: 1, changed: true, occurredAt: state.firstReadAt, resultingReadState: { firstReadAt: state.firstReadAt, needsRecheck: kind === 'RESET', readStateRevision: 2, isRead: kind === 'VIEW' } };
  const fetcher = jest.fn(async () => Response.json(result)); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(await documentApi[name](documentId, versionId, body)).toEqual(result); const request = (fetcher.mock.calls as unknown as [Request][])[0]![0]; expect(request.method).toBe('POST'); expect(request.url).toBe(`https://synthetic.invalid/v1/documents/${documentId}/versions/${versionId}/read-state/${kind.toLowerCase()}`); expect(await request.json()).toEqual(body);
});
test('canonical_state_problem_is_preserved_for_feature_only_denial', async () => {
  expect(documentApi).toHaveProperty('getCurrentDocumentVersionReadState', expect.any(Function));
  const problem = { type: 'about:blank', title: 'Synthetic', status: 403, code: 'FORBIDDEN', traceId: 'synthetic', retryable: false }; client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: 403 }) });
  await expect(documentApi.getCurrentDocumentVersionReadState(documentId, versionId)).rejects.toEqual(problem);
});
