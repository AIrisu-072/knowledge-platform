import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { OriginalVersionDownload } from '../src/components/shared/OriginalVersionDownload';
import { documentApi } from '../src/application/document-workspace';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';
import { folderAccessPolicyOperations, sendFolderAccessPolicyOperation } from '../src/application/document-folder-access-policy';
jest.mock('../src/application/document-workspace', () => ({ documentApi: { listVersionFiles: jest.fn(), downloadVersionFile: jest.fn() } }));
function deferred<T>() { let resolve!: (v: T) => void; const promise = new Promise<T>(r => { resolve = r; }); return { promise, resolve }; }
const key = ['document-version-files', 'document', 'version', 'published'];
const files = { items: [{ contentItemId: 'item', representationId: 'original', displayName: '原本.pdf' }] };
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => client.clear()));
function setup() {
  const api = documentApi as unknown as { listVersionFiles: jest.Mock; downloadVersionFile: jest.Mock }; api.listVersionFiles.mockReset().mockResolvedValue(files); api.downloadVersionFile.mockReset();
  const pending = deferred<Blob>(); api.downloadVersionFile.mockReturnValue(pending.promise);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: Infinity } } }); clients.push(client);
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn().mockReturnValue('blob:synthetic') }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  const element = (versionId = 'version') => <QueryClientProvider client={client}><OriginalVersionDownload documentId="document" versionId={versionId} purpose="published" label="原本を取得" /></QueryClientProvider>;
  const rendered = render(element()); return { api, pending, client, element, ...rendered };
}
test('現在のfiles readで選んだ原本を既存signal transportで取得する', async () => { const h = setup(); fireEvent.click(await screen.findByRole('button', { name: '原本を取得' })); await act(async () => h.pending.resolve(new Blob(['current']))); expect(h.api.downloadVersionFile).toHaveBeenCalledWith({ documentId: 'document', versionId: 'version', contentItemId: 'item', representationId: 'original', purpose: 'published' }, { signal: expect.any(AbortSignal) }); expect(URL.createObjectURL).toHaveBeenCalledTimes(1); expect(HTMLAnchorElement.prototype.click).toHaveBeenCalledTimes(1); });
test.each(['invalidate', 'reset', 'remove', 'refetch-denied', 'selection', 'unmount'] as const)('%sと同tickの古いBlobは保存せず中断する', async kind => {
  const h = setup(); fireEvent.click(await screen.findByRole('button', { name: '原本を取得' }));
  const signal = h.api.downloadVersionFile.mock.calls[0]![1]?.signal;
  // Commit the changed props before delivering bytes; an uncommitted React update is not a selection.
  if (kind === 'selection') h.rerender(h.element('other'));
  await act(async () => {
    if (kind === 'invalidate') void h.client.invalidateQueries({ queryKey: key, exact: true, refetchType: 'none' });
    else if (kind === 'reset') void h.client.resetQueries({ queryKey: key, exact: true });
    else if (kind === 'remove') h.client.removeQueries({ queryKey: key, exact: true });
    else if (kind === 'refetch-denied') { h.api.listVersionFiles.mockRejectedValue(new Error('403')); void h.client.refetchQueries({ queryKey: key, exact: true }); }
    else if (kind === 'unmount') h.unmount();
    h.pending.resolve(new Blob(['late']));
  });
  expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled(); expect(signal?.aborted).toBe(true);
});
test('ACL成功の全体read resetと同tickの原本Blobは保存しない', async () => {
  const h = setup(); fireEvent.click(await screen.findByRole('button', { name: '原本を取得' })); h.api.listVersionFiles.mockRejectedValue(new Error('403'));
  await act(async () => {
    void sendFolderAccessPolicyOperation({ store: folderAccessPolicyOperations(h.client), targetFolderId: 'folder', context: { kind: 'selected', folderId: 'folder', sourceParentId: 'root', pageLimit: 1, name: '資料' }, request: { operationId: 'policy', mode: 'inherit', expectedPolicyRevision: 7, reason: '理由' }, send: async () => ({ operationId: 'policy', resourceId: 'folder', resultingRevision: 8, changed: true, occurredAt: '2026-10-07T00:00:00Z' }), invalidate: () => refreshFolderMoveReads(h.client) });
    h.pending.resolve(new Blob(['late']));
  });
  await waitFor(() => expect(folderAccessPolicyOperations(h.client).get()?.status).toBe('succeeded')); expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled();
});
