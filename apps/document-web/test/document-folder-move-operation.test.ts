import { QueryClient, QueryObserver, skipToken } from '@tanstack/react-query';
import { folderMoveOperations, sendFolderMoveOperation, refreshFolderMoveReads, moveRevisionError, moveReasonValidation } from '../src/application/document-folder-move';
import { rootFolderOperations, type SelectedFolderContext } from '../src/application/document-root-folder';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { metadataOperations } from '../src/application/document-metadata';
import { runWorkingOperation } from '../src/application/document-working-version';
import type { CommandsMoveFolder, MutationResult } from '@knowledge-platform/document-api-client';
jest.mock('../src/application/document-workspace', () => ({ documentApi: { rebaseWorkingVersion: jest.fn().mockRejectedValue(new Error('lost')) } }));
const context: SelectedFolderContext = { kind: 'selected', folderId: 'target', sourceParentId: 'source', name: '資料', pageLimit: 2 };
const destination = { kind: 'root' as const, folderId: 'destination', name: 'System Root' };
const request: CommandsMoveFolder = { operationId: 'operation', fromParentId: 'source', toParentId: 'destination', expectedFolderRevision: 8, reason: '理由' };
const result: MutationResult = { operationId: 'operation', resourceId: 'target', changed: true, resultingRevision: 9, occurredAt: '2026-10-06T07:00:00Z' };
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
const stores: ReturnType<typeof folderMoveOperations>[] = [];
afterEach(() => stores.splice(0).forEach(store => { const operation = store.get(); if (operation) { store.put({ ...operation, status: 'succeeded' }); store.clearSettled(store.get()!); } }));
function harness() {
  const owner = {}; const store = folderMoveOperations(owner); stores.push(store); const send = jest.fn().mockResolvedValue(result); const invalidate = jest.fn().mockResolvedValue(undefined);
  return { owner, store, send, invalidate, run: (body = request) => sendFolderMoveOperation({ store, targetFolderId: context.folderId, context, destination, currentName: context.name, request: body, send, invalidate }) };
}
test.each([0, 1, Number.MAX_SAFE_INTEGER - 1])('実移動revision %pをsafeな+1で厳密照合する', async revision => {
  const h = harness(); h.send.mockResolvedValue({ ...result, resultingRevision: revision + 1 }); await h.run({ ...request, expectedFolderRevision: revision }); expect(h.store.get()).toMatchObject({ status: 'succeeded', refresh: 'complete' });
});
test.each([0, Number.MAX_SAFE_INTEGER])('同親no-opも送信してrevision %pの据置receiptを確認する', async revision => {
  const h = harness(); h.send.mockResolvedValue({ ...result, changed: false, resultingRevision: revision }); await h.run({ ...request, toParentId: 'source', expectedFolderRevision: revision }); expect(h.send).toHaveBeenCalledTimes(1); expect(h.store.get()).toMatchObject({ status: 'succeeded', expectedChanged: false });
});
test.each([-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1])('不正実移動revision %pはPOST前に止める', async revision => {
  const h = harness(); await expect(h.run({ ...request, expectedFolderRevision: revision })).rejects.toThrow(/revision/); expect(h.send).not.toHaveBeenCalled(); expect(h.store.get()).toBeUndefined();
});
test('理由のraw/Unicode/UTF8境界とno-opのMAX safe境界を既存契約に合わせる', () => {
  expect(moveRevisionError(Number.MAX_SAFE_INTEGER, false)).toBeNull();
  for (const reason of ['', 'a\u0085b', 'あ'.repeat(342), '\ud800']) expect(moveReasonValidation(reason)).toMatch(/^移動理由/);
  expect(moveReasonValidation(`\u0085${'あ'.repeat(341)}a\u0085`)).toBeNull();
});
test('固定要求はownerで隔離されUNKNOWNから別入力を与えても同一path/body/context/destinationを再送する', async () => {
  const h = harness(); expect(folderMoveOperations(h.owner)).toBe(h.store); expect(folderMoveOperations({}).get()).toBeUndefined();
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); const saved = h.store.get()!;
  expect(Object.isFrozen(saved.request)).toBe(true); expect(Object.isFrozen(saved.context)).toBe(true); expect(Object.isFrozen(saved.destination)).toBe(true); expect(h.store.clearSettled(saved)).toBe(false);
  await sendFolderMoveOperation({ store: h.store, targetFolderId: 'other', context: { ...context, folderId: 'other' }, destination: { ...destination, folderId: 'other' }, currentName: 'other', request: { ...request, operationId: 'new', toParentId: 'other' }, send: h.send, invalidate: h.invalidate });
  expect(h.send.mock.calls[1]![0]).toBe('target'); expect(h.send.mock.calls[1]![1]).toBe(saved.request); expect(h.store.get()).toMatchObject({ status: 'succeeded', context, destination, currentName: context.name });
});
test('pendingの二重送信とclearを防ぐ', async () => {
  const h = harness(); let resolve!: (value: MutationResult) => void; h.send.mockReturnValue(new Promise<MutationResult>(done => { resolve = done; })); const running = h.run(); const saved = h.store.get()!;
  expect(h.store.clearSettled(saved)).toBe(false); await h.run(); expect(h.send).toHaveBeenCalledTimes(1); resolve(result); await running;
  expect(h.store.clearSettled({ ...h.store.get()! })).toBe(false); expect(h.store.clearSettled(h.store.get()!)).toBe(true);
});
test.each([['VALIDATION_FAILED', 422], ['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['ROOT_PROTECTED', 409], ['BUSINESS_RULE_REJECTED', 422], ['FOLDER_CYCLE', 409], ['RESERVED_DOCUMENT', 409]])('初回正規%s/%pだけ確定拒否しUNKNOWN再送での同じ拒否は未確定を保持する', async (code, status) => {
  const h = harness(); h.send.mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); expect(h.store.get()?.status).toBe('rejected'); h.store.clearSettled(h.store.get()!);
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); const fixed = h.store.get()!.request;
  h.send.mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); expect(h.store.get()).toMatchObject({ status: 'unknown' }); expect(h.store.get()!.request).toBe(fixed);
});
test.each([problem('INTERNAL', 500), problem('DEPENDENCY_UNAVAILABLE', 503), problem('FOLDER_CYCLE', 422), problem('RESERVED_DOCUMENT', 422), { code: 'FORBIDDEN', status: 403 }, { ...problem('FORBIDDEN', 403), exactRetry: true }])('不確定エラー %pはUNKNOWNとして固定要求を保持する', async error => {
  const h = harness(); h.send.mockRejectedValue(error); await h.run(); expect(h.store.get()?.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled();
});
test.each([{ operationId: 'other' }, { resourceId: 'other' }, { changed: false }, { changed: 1 }, { resultingRevision: 8 }, { resultingRevision: 9.5 }, { resultingRevision: Number.MAX_SAFE_INTEGER + 1 }, { occurredAt: '2026-02-30T00:00:00Z' }, { occurredAt: '2026-10-06' }, { occurredAt: null }])('不正200 receipt %pから成功を推測しない', async patch => {
  const h = harness(); h.send.mockResolvedValue({ ...result, ...patch }); await h.run(); expect(h.store.get()?.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled();
});
test('現在READ失敗は確定receiptをUNKNOWNへ戻さず別のrefresh失敗を保持する', async () => {
  const h = harness(); h.invalidate.mockRejectedValue(problem('FORBIDDEN', 403)); await h.run(); expect(h.store.get()).toMatchObject({ status: 'succeeded', result, refresh: 'failed' });
});
test.each(['pending', 'unknown', 'rejected', 'succeeded'])('global READ resetでもquery-backedの3操作familyの%sと全固定object/blobを保全する', async status => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  const blob = new Blob(['fixed bytes']); const intent = Object.freeze({ body: Object.freeze({ operationId: 'fixed' }), files: new Map([['file', blob]]) });
  const saved = { status, intent }; const operationKeys = [['document-working-operation', 'doc'], ['document-schedule-cancel', 'doc', 'version'], ['document-lifecycle-operation', 'doc']];
  const observers = operationKeys.map(queryKey => { const observer = new QueryObserver(client, { queryKey, queryFn: skipToken, enabled: false, initialData: null, gcTime: Infinity }); const unsubscribe = observer.subscribe(() => undefined); client.setQueryData(queryKey, saved); return unsubscribe; });
  const readKeys = [['folder-tree', 'parent', 'pages'], ['document', 'doc'], ['documents', { cursor: 'old' }], ['document-edit-manifest', 'doc'], ['revision-comparison', 'doc', 'base', 'target'], ['organization', 'principal', 'assignment', 'document-context', 'task', 'attempt', 'doc'], ['organization', 'principal', 'assignment', 'evidence-context', 'task', 'attempt', 'doc'], ['organization', 'principal', 'assignment', 'agent-context', 'task', 'attempt', 'execution'], ['organization', 'principal', 'assignment', 'snapshot', 'snapshot']];
  const read = { opaqueCursor: 'old', title: 'old authorized read' }; readKeys.forEach(key => client.setQueryData(key, read)); client.setQueryData(['unrelated'], read);
  // Resetting tasks would transiently remove the selected task and clear the provider's Agent draft.
  const workKeys = [['organization-session'], ['organization', 'principal', 'assignment', 'tasks', 'context'], ['organization', 'principal', 'assignment', 'task', 'task', 'attempt'], ['organization', 'principal', 'assignment', 'return-instruction', 'instruction']]; workKeys.forEach(key => client.setQueryData(key, read));
  const create = rootFolderOperations(client); create.put({ request: { operationId: 'create', folderId: 'new', parentFolderId: 'source', expectedParentRevision: 0, name: '新', reason: '理由' }, status: 'unknown' }); const createSaved = create.get();
  const rename = folderRenameOperations(client); rename.put({ targetFolderId: 'target', context, currentName: context.name, expectedChanged: true, request: { operationId: 'rename', expectedFolderRevision: 8, name: '別', reason: '理由' }, status: 'unknown' }); const renameSaved = rename.get();
  const metadata = metadataOperations(client); metadata.put('doc', { request: { operationId: 'metadata', expectedDocumentRevision: 8, set: {}, unset: [], reason: '理由' }, status: 'unknown' }); const metadataSaved = metadata.get('doc');
  await refreshFolderMoveReads(client);
  operationKeys.forEach(key => expect(client.getQueryData(key)).toBe(saved)); expect((client.getQueryData(operationKeys[0]!) as typeof saved).intent.files.get('file')).toBe(blob);
  readKeys.forEach(key => expect(client.getQueryData(key)).toBeUndefined()); expect(client.getQueryData(['unrelated'])).toBe(read);
  workKeys.forEach(key => expect(client.getQueryData(key)).toBe(read));
  expect(create.get()).toBe(createSaved); expect(rename.get()).toBe(renameSaved); expect(metadata.get('doc')).toBe(metadataSaved); expect(window.dispatchEvent(new Event('beforeunload', { cancelable: true }))).toBe(false);
  create.put({ ...createSaved!, status: 'succeeded' }); create.clearSettled(create.get()!); rename.put({ ...renameSaved!, status: 'succeeded' }); rename.clearSettled(rename.get()!); metadata.clear('doc'); observers.forEach(unsubscribe => unsubscribe()); client.clear();
});
test('global READ resetでactive Folder pagesは古いopaque cursorを捨て先頭undefinedから読む', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } }); const key = ['folder-tree', 'parent', 'pages']; const calls: unknown[] = [];
  const { InfiniteQueryObserver } = await import('@tanstack/react-query');
  const observer = new InfiniteQueryObserver(client, { queryKey: key, staleTime: Infinity, initialPageParam: undefined as string | undefined, queryFn: async ({ pageParam }) => { calls.push(pageParam); return { items: [], nextCursor: pageParam === undefined ? 'fresh' : null }; }, getNextPageParam: last => last.nextCursor ?? undefined });
  client.setQueryData(key, { pages: [{ items: [], nextCursor: 'old' }, { items: [], nextCursor: null }], pageParams: [undefined, 'old'] }); const unsubscribe = observer.subscribe(() => undefined); calls.length = 0;
  await refreshFolderMoveReads(client); expect(calls).toEqual([undefined]); expect(client.getQueryData(key)).toMatchObject({ pageParams: [undefined] }); unsubscribe(); client.clear();
});
test('実WORKING UNKNOWNのbeforeunload guardと固定要求はREAD refresh後も生きる', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await runWorkingOperation(client, { kind: 'rebase', documentId: 'doc', sourceVersionId: 'version', currentVersionId: 'current', body: { operationId: 'fixed', expectedRevision: 8 } }); const saved = client.getQueryData(['document-working-operation', 'doc']);
  await refreshFolderMoveReads(client); expect(client.getQueryData(['document-working-operation', 'doc'])).toBe(saved); expect(window.dispatchEvent(new Event('beforeunload', { cancelable: true }))).toBe(false); client.clear();
});
