import { QueryClient, QueryObserver, skipToken } from '@tanstack/react-query';
import { refreshFolderMoveReads, folderMoveOperations } from '../src/application/document-folder-move';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { metadataOperations } from '../src/application/document-metadata';
import { documentMoveOperations, sendDocumentMoveOperation, documentMoveRevisionError, documentMoveReasonValidation, canMoveDocument } from '../src/application/document-move';
import type { CommandsMoveDocument, MutationResult } from '@knowledge-platform/document-api-client';
const context = { documentId: 'target', title: '文書', folderId: 'source', folderName: '元所属', view: 'published' as const };
const destination = { kind: 'root' as const, folderId: 'destination', name: 'System Root' };
const request: CommandsMoveDocument = { operationId: 'operation', fromFolderId: 'source', toFolderId: 'destination', expectedDocumentRevision: 8, reason: '理由' };
const result: MutationResult = { operationId: 'operation', resourceId: 'target', changed: true, resultingRevision: 9, occurredAt: '2026-10-06T07:00:00Z' };
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
const stores: ReturnType<typeof documentMoveOperations>[] = [];
afterEach(() => stores.splice(0).forEach(store => { const operation = store.get(); if (operation) { store.put({ ...operation, status: 'succeeded' }); store.clearSettled(store.get()!); } }));
function harness() {
  const owner = {}; const store = documentMoveOperations(owner); stores.push(store); const send = jest.fn().mockResolvedValue(result); const invalidate = jest.fn().mockResolvedValue(undefined);
  return { owner, store, send, invalidate, run: (body = request) => sendDocumentMoveOperation({ store, targetDocumentId: context.documentId, context, destination, request: body, send, invalidate }) };
}
test.each([0, 1, Number.MAX_SAFE_INTEGER - 1])('実移動revision %pをsafeな+1で厳密照合する', async revision => {
  const h = harness(); h.send.mockResolvedValue({ ...result, resultingRevision: revision + 1 }); await h.run({ ...request, expectedDocumentRevision: revision }); expect(h.store.get()).toMatchObject({ status: 'succeeded', refresh: 'complete' });
});
test.each([0, Number.MAX_SAFE_INTEGER])('同Folderno-opも送信してrevision %pの据置receiptを確認する', async revision => {
  const h = harness(); h.send.mockResolvedValue({ ...result, changed: false, resultingRevision: revision }); await h.run({ ...request, toFolderId: 'source', expectedDocumentRevision: revision }); expect(h.send).toHaveBeenCalledTimes(1); expect(h.store.get()).toMatchObject({ status: 'succeeded', expectedChanged: false });
});
test.each([-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1])('不正実移動revision %pはPOST前に止める', async revision => {
  const h = harness(); await expect(h.run({ ...request, expectedDocumentRevision: revision })).rejects.toThrow(/revision/); expect(h.send).not.toHaveBeenCalled(); expect(h.store.get()).toBeUndefined();
});
test('理由のraw/Unicode/UTF8境界とno-opのMAX safe境界を既存契約に合わせる', () => {
  expect(documentMoveRevisionError(Number.MAX_SAFE_INTEGER, false)).toBeNull();
  for (const reason of ['', 'a\u0085b', 'あ'.repeat(342), '\ud800']) expect(documentMoveReasonValidation(reason)).toMatch(/^移動理由/);
  expect(documentMoveReasonValidation(`\u0085${'あ'.repeat(341)}a\u0085`)).toBeNull();
});
test('固定要求はownerで隔離されUNKNOWNから別入力を与えても同一path/body/context/destinationを再送する', async () => {
  const h = harness(); expect(documentMoveOperations(h.owner)).toBe(h.store); expect(documentMoveOperations({}).get()).toBeUndefined();
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); const saved = h.store.get()!;
  expect(Object.isFrozen(saved.request)).toBe(true); expect(Object.isFrozen(saved.context)).toBe(true); expect(Object.isFrozen(saved.destination)).toBe(true); expect(h.store.clearSettled(saved)).toBe(false);
  await sendDocumentMoveOperation({ store: h.store, targetDocumentId: 'other', context: { ...context, documentId: 'other' }, destination: { ...destination, folderId: 'other' }, request: { ...request, operationId: 'new', toFolderId: 'other' }, send: h.send, invalidate: h.invalidate });
  expect(h.send.mock.calls[1]![0]).toBe('target'); expect(h.send.mock.calls[1]![1]).toBe(saved.request); expect(h.store.get()).toMatchObject({ status: 'succeeded', context, destination });
});
test('pendingの二重送信とclearを防ぐ', async () => {
  const h = harness(); let resolve!: (value: MutationResult) => void; h.send.mockReturnValue(new Promise<MutationResult>(done => { resolve = done; })); const running = h.run(); const saved = h.store.get()!;
  expect(h.store.clearSettled(saved)).toBe(false); await h.run(); expect(h.send).toHaveBeenCalledTimes(1); resolve(result); await running;
  expect(h.store.clearSettled({ ...h.store.get()! })).toBe(false); expect(h.store.clearSettled(h.store.get()!)).toBe(true);
});
test.each([['VALIDATION_FAILED', 422], ['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['DOCUMENT_NOT_FOUND', 404], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['BUSINESS_RULE_REJECTED', 422], ['RESERVED_DOCUMENT', 409]])('初回正規%s/%pだけ確定拒否しUNKNOWN再送での同じ拒否は未確定を保持する', async (code, status) => {
  const h = harness(); h.send.mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); expect(h.store.get()?.status).toBe('rejected'); h.store.clearSettled(h.store.get()!);
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); const fixed = h.store.get()!.request;
  h.send.mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); expect(h.store.get()).toMatchObject({ status: 'unknown' }); expect(h.store.get()!.request).toBe(fixed);
});
test.each([problem('INTERNAL', 500), problem('DEPENDENCY_UNAVAILABLE', 503), problem('RESERVED_DOCUMENT', 422), { code: 'FORBIDDEN', status: 403 }, { ...problem('FORBIDDEN', 403), exactRetry: true }])('不確定エラー %pはUNKNOWNとして固定要求を保持する', async error => {
  const h = harness(); h.send.mockRejectedValue(error); await h.run(); expect(h.store.get()?.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled();
});
test.each([{ operationId: 'other' }, { resourceId: 'other' }, { changed: false }, { changed: 1 }, { resultingRevision: 8 }, { resultingRevision: 9.5 }, { resultingRevision: Number.MAX_SAFE_INTEGER + 1 }, { occurredAt: '2026-02-30T00:00:00Z' }, { occurredAt: '2026-10-06' }, { occurredAt: null }])('不正200 receipt %pから成功を推測しない', async patch => {
  const h = harness(); h.send.mockResolvedValue({ ...result, ...patch }); await h.run(); expect(h.store.get()?.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled();
});
test('現在READ失敗は確定receiptをUNKNOWNへ戻さず別のrefresh失敗を保持する', async () => {
  const h = harness(); h.invalidate.mockRejectedValue(problem('FORBIDDEN', 403)); await h.run(); expect(h.store.get()).toMatchObject({ status: 'succeeded', result, refresh: 'failed' });
});

test.each([null, undefined, ''])('元Folder ID %pをRootやURLで補完しない', folderId => {
  expect(canMoveDocument({ documentId: 'target', title: '文書', folderId, folderName: '元所属', revision: 8, capabilities: { moveDocument: { status: 'available' } } } as never)).toBe(false);
});
test.each([{ title: '' }, { folderName: null }, { revision: -1 }, { revision: 0.5 }, { capabilities: { moveDocument: { status: 'disabled', reason: 'permission' } } }])('文書readの不正または現在disabled %pを新規提示しない', patch => {
  const detail = { documentId: 'target', title: '文書', folderId: 'source', folderName: '元所属', revision: 8, capabilities: { moveDocument: { status: 'available' } } };
  expect(canMoveDocument({ ...detail, ...patch } as never)).toBe(false);
});

test.each([{ documentId: 42 }, { title: {} }, { folderId: 42 }, { folderName: true }])('不正なread型 %pを新規移動へ流さない', patch => {
  const detail = { documentId: 'target', title: '文書', folderId: 'source', folderName: '元所属', revision: 8, capabilities: { moveDocument: { status: 'available' } } };
  expect(canMoveDocument({ ...detail, ...patch } as never)).toBe(false);
});

test.each(['pending', 'unknown', 'rejected', 'succeeded'] as const)('global read resetはDocument move %sとmetadata・Folder・query operation/providerを保全する', async status => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  const documentStore = documentMoveOperations(client); stores.push(documentStore);
  documentStore.put({ status, targetDocumentId: 'target', context: Object.freeze(context), destination: Object.freeze(destination), expectedChanged: true, request: Object.freeze(request) });
  const documentFixed = documentStore.get();
  const createStore = rootFolderOperations(client); createStore.put({ status: 'unknown', request: Object.freeze({ operationId: 'create', folderId: 'new', parentFolderId: 'source', expectedParentRevision: 2, name: '新', reason: '理由' }) });
  const renameStore = folderRenameOperations(client); renameStore.put({ status: 'unknown', targetFolderId: 'folder', currentName: '資料', expectedChanged: true, context: { kind: 'selected', folderId: 'folder', sourceParentId: 'source', pageLimit: 1, name: '資料' }, request: Object.freeze({ operationId: 'rename', expectedFolderRevision: 2, name: '新', reason: '理由' }) });
  const folderStore = folderMoveOperations(client); folderStore.put({ status: 'unknown', targetFolderId: 'folder', currentName: '資料', expectedChanged: true, context: { kind: 'selected', folderId: 'folder', sourceParentId: 'source', pageLimit: 1, name: '資料' }, destination, request: Object.freeze({ operationId: 'move', fromParentId: 'source', toParentId: 'destination', expectedFolderRevision: 2, reason: '理由' }) });
  const metadata = metadataOperations(client); metadata.put('target', { status: 'unknown', request: { operationId: 'metadata', expectedDocumentRevision: 8, set: {}, unset: [], reason: '理由' } });
  const fixed = [createStore.get(), renameStore.get(), folderStore.get(), metadata.get('target')];
  const blob = new Blob(['fixed bytes']); const intent = Object.freeze({ body: Object.freeze({ operationId: 'fixed' }), files: new Map([['file', blob]]) }); const saved = { status, intent };
  const keys = [['document-working-operation', 'target'], ['document-schedule-cancel', 'target', 'version'], ['document-lifecycle-operation', 'target'], ['organization', 'identity', 'task-selection']];
  const observers = keys.map(queryKey => { const observer = new QueryObserver(client, { queryKey, queryFn: skipToken, enabled: false, initialData: null, gcTime: Infinity }); const unsubscribe = observer.subscribe(() => undefined); client.setQueryData(queryKey, saved); return { queryKey, unsubscribe }; });
  client.setQueryData(['document', 'target', 'published'], { folderId: 'old' });
  await refreshFolderMoveReads(client);
  expect(client.getQueryData(['document', 'target', 'published'])).toBeUndefined(); expect(documentStore.get()).toBe(documentFixed);
  [createStore.get(), renameStore.get(), folderStore.get(), metadata.get('target')].forEach((operation, index) => expect(operation).toBe(fixed[index]));
  observers.forEach(({ queryKey, unsubscribe }) => { expect(client.getQueryData(queryKey)).toBe(saved); expect((client.getQueryData(queryKey) as typeof saved).intent.files.get('file')).toBe(blob); unsubscribe(); });
  for (const store of [createStore, renameStore, folderStore]) { const operation = store.get()!; store.put({ ...operation, status: 'succeeded' } as never); store.clearSettled(store.get()! as never); }
  metadata.clear('target'); client.clear();
});
