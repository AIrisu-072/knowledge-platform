import { patchDocumentMetadata } from '@knowledge-platform/document-api-client';
import { documentApi } from '../src/api/document-api';

jest.mock('@knowledge-platform/document-api-client', () => ({
  BinaryTransportBridge: jest.fn(), patchDocumentMetadata: jest.fn(),
}), { virtual: true });

test('既存generated PATCHへ対象・固定payload・throwOnErrorを渡しMutationResultだけを返す', async () => {
  const body = { operationId: '019a0010-0000-7000-8000-000000000001', expectedDocumentRevision: 7,
    set: { category: '' }, unset: ['document_type'], reason: '合成変更' };
  const result = { operationId: body.operationId, resourceId: 'document-id', resultingRevision: 8, changed: true, occurredAt: '2026-10-05T00:00:00Z' };
  const patch = patchDocumentMetadata as jest.Mock;
  patch.mockResolvedValue({ data: result });
  expect(await documentApi.patchDocumentMetadata('document-id', body)).toBe(result);
  expect(patch).toHaveBeenCalledWith({ throwOnError: true, path: { documentId: 'document-id' }, body });
  expect(patch.mock.calls[0][0].body).toBe(body);
});

test('生成PATCHの拒否を結果へ変換せず呼出元へ返す', async () => {
  const error = { code: 'FORBIDDEN', status: 403 };
  (patchDocumentMetadata as jest.Mock).mockRejectedValue(error);
  await expect(documentApi.patchDocumentMetadata('document-id', { operationId: 'op', expectedDocumentRevision: 7, set: {}, unset: [], reason: '合成変更' })).rejects.toBe(error);
});
