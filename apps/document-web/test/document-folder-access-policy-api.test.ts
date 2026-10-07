/** @jest-environment node */
import { client } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi } from '../src/api/document-api';

// Exercise the real generated SDK and typed-client serialization without a listener or network.
jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });
const targetId = '019a0010-0000-7000-8000-000000000002';
const body = { operationId: '019a0010-0000-7000-8000-000000000001', expectedPolicyRevision: 17, mode: 'inherit' as const, reason: '合成理由' };

test('既存typed SDKのPUT /v1/folders/{folderId}/access-policyで全固定payloadをJSON送信しMutationResultのみ返す', async () => {
  const result = { operationId: body.operationId, resourceId: targetId, resultingRevision: 18, changed: true, occurredAt: '2026-10-05T12:00:00Z' };
  const fetcher = jest.fn(async () => Response.json(result, { status: 200 })); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(documentApi).toHaveProperty('setFolderAccessPolicy', expect.any(Function));
  expect(await documentApi.setFolderAccessPolicy(targetId, body)).toEqual(result);
  const request = (fetcher.mock.calls as unknown as [Request][])[0]![0];
  expect(request.method).toBe('PUT'); expect(request.url).toBe(`https://synthetic.invalid/v1/folders/${targetId}/access-policy`); expect(request.headers.get('content-type')).toBe('application/json');
  expect(await request.json()).toEqual(body);
});

test('既存typed SDKのProblemはthrowOnErrorで呼出元へ伝播する', async () => {
  const problem = { type: 'about:blank', title: 'Synthetic', status: 409, code: 'REVISION_CONFLICT', traceId: 'synthetic', retryable: false };
  client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => Response.json(problem, { status: 409 }) });
  await expect(documentApi.setFolderAccessPolicy(targetId, body)).rejects.toEqual(problem);
});

test('既存SDKのGET policyを対象pathから読みenrichmentを保全する', async () => {
  const read = { target: { kind: 'folder', id: targetId }, bindingMode: 'inherit', policyId: null, policyRevision: 0, effectivePolicyId: 'policy', effectiveSource: { kind: 'folder', id: 'root' }, effectiveGrants: [] };
  const fetcher = jest.fn(async () => Response.json(read)); client.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: fetcher });
  expect(documentApi).toHaveProperty('getFolderAccessPolicy', expect.any(Function)); expect(await documentApi.getFolderAccessPolicy(targetId)).toEqual(read);
  const request = (fetcher.mock.calls as unknown as [Request][])[0]![0]; expect(request.method).toBe('GET'); expect(request.url).toBe(`https://synthetic.invalid/v1/folders/${targetId}/access-policy`);
});
