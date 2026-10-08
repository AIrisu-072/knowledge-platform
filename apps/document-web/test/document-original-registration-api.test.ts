import { BinaryTransportBridge, recoverDocumentCreation } from '@knowledge-platform/document-api-client';
import { documentApi } from '../src/api/document-api';

jest.mock('@knowledge-platform/document-api-client', () => ({
  BinaryTransportBridge: jest.fn().mockImplementation(() => ({
    prepareDocumentItemsUpload: jest.fn().mockReturnValue({ body: new Blob(['fixed']), contentType: 'multipart/form-data; boundary=fixed' }),
    createDocumentItems: jest.fn().mockResolvedValue({ documentId: 'document' }),
  })), recoverDocumentCreation: jest.fn(),
}), { virtual: true });

const bridge = (BinaryTransportBridge as jest.Mock).mock.results[0]!.value;
test('事前準備で各原本をpartへ一対一に束縛し、そのmultipartを再構築せず送る', async () => {
  Object.defineProperty(crypto, 'randomUUID', { configurable: true, value: () => `part-${bridge.prepareDocumentItemsUpload.mock.calls.length}-${Math.random()}` });
  const request = { folderId: 'folder', title: '文書', documentMetadata: {}, versionMetadata: {} };
  const originals = [{ file: new File(['A'], 'A.txt', { type: 'text/plain' }), logicalPath: 'a', ordinal: 0 },
    { file: new File(['B'], 'B.txt', { type: 'text/plain' }), logicalPath: 'b', ordinal: 1 }];
  const prepared = documentApi.prepareDocumentWithOriginals(request, originals);
  expect(bridge.createDocumentItems).not.toHaveBeenCalled();
  expect(prepared.upload.request.items.map(item => [item.logicalPath, item.ordinal, item.originalFilename])).toEqual([['a', 0, 'A.txt'], ['b', 1, 'B.txt']]);
  prepared.upload.request.items.forEach((item, index) => expect(prepared.upload.files.get(item.partId)).toBe(originals[index]!.file));
  await documentApi.createDocumentWithOriginals(request, originals, prepared);
  expect(bridge.prepareDocumentItemsUpload).toHaveBeenCalledTimes(1);
  expect(bridge.createDocumentItems).toHaveBeenCalledWith(prepared.upload, prepared.multipart);
});

test('回復GETは全FileIDsの順序を維持しlegacy回復には追加queryを付けない', async () => {
  const recover = recoverDocumentCreation as jest.Mock;
  recover.mockResolvedValue({ data: {} });
  const ids = { documentId: 'document', documentVersionId: 'version', fileId: 'first', fileIds: ['first', 'second'] };
  await documentApi.recoverDocumentCreation(ids);
  expect(recover).toHaveBeenLastCalledWith({ throwOnError: true, path: { documentId: 'document' }, query: { documentVersionId: 'version', fileId: 'first', fileIds: 'first,second' } });
  await documentApi.recoverDocumentCreation({ documentId: 'document', documentVersionId: 'version', fileId: 'first' });
  expect(recover).toHaveBeenLastCalledWith({ throwOnError: true, path: { documentId: 'document' }, query: { documentVersionId: 'version', fileId: 'first' } });
});
