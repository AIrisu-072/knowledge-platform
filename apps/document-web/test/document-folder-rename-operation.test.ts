import { folderRenameOperations, sendFolderRenameOperation, renameFolderValidation, renameRevisionError, canRenameFolder } from '../src/application/document-folder-rename';
import { folderName, rootFolderOperations, rootFolderValidation } from '../src/application/document-root-folder';
import type { CommandsRenameFolder, FolderDetail, MutationResult } from '@knowledge-platform/document-api-client';

const targetFolderId = 'target';
const context = { kind: 'selected' as const, folderId: targetFolderId, sourceParentId: 'parent', pageLimit: 2, name: '旧資料' };
const request: CommandsRenameFolder = { operationId: 'operation', expectedFolderRevision: 17, name: '新資料', reason: '合成理由' };
const result: MutationResult = { operationId: request.operationId, resourceId: targetFolderId, changed: true, resultingRevision: 18, occurredAt: '2026-10-06T00:00:00Z' };
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
function harness() {
  const owner = {}; const store = folderRenameOperations(owner); const send = jest.fn().mockResolvedValue(result); const invalidate = jest.fn().mockResolvedValue(undefined);
  return { owner, store, send, invalidate, run: (body = request, currentName = context.name) => sendFolderRenameOperation({ store, targetFolderId, request: body, context, currentName, send, invalidate }) };
}
// Any relaxed rename numeric boundary must fail these without narrowing the create boundary.
test.each([0, 1, Number.MAX_SAFE_INTEGER - 1])('実変更revision %pは正確に+1を照合する', async revision => {
  const h = harness(); h.send.mockResolvedValue({ ...result, resultingRevision: revision + 1 }); await h.run({ ...request, expectedFolderRevision: revision });
  expect(h.store.get()).toMatchObject({ status: 'succeeded', expectedChanged: true, refresh: 'complete' });
});
test.each([0, Number.MAX_SAFE_INTEGER])('正規化同名はAPIへ送りrevision %pを据置する', async revision => {
  const h = harness(); const name = folderName('\u2003e\u0301\u2003'); h.send.mockResolvedValue({ ...result, resultingRevision: revision, changed: false });
  await h.run({ ...request, name, expectedFolderRevision: revision }, 'é');
  expect(h.send).toHaveBeenCalledTimes(1); expect(h.store.get()).toMatchObject({ status: 'succeeded', expectedChanged: false });
});
test.each([Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1, -1, 0.5, NaN, Infinity])('実変更不正revision %pは送信を止める', async revision => {
  const h = harness(); await expect(h.run({ ...request, expectedFolderRevision: revision })).rejects.toThrow(/revision/);
  expect(h.send).not.toHaveBeenCalled(); expect(h.store.get()).toBeUndefined();
});
test('create親revisionのMAX safe据置資格は狭めない', () => {
  const folder = { folderId: targetFolderId, name: context.name, revision: Number.MAX_SAFE_INTEGER, capabilities: { renameFolder: { status: 'available' }, createFolder: { status: 'available' } } } as FolderDetail;
  expect(canRenameFolder(folder)).toBe(true); expect(renameRevisionError(folder, '別名')).toMatch(/revision/); expect(renameRevisionError(folder, context.name)).toBeNull();
});
test.each(['', '.', '..', 'x/y', 'x\\y', '\t名前', '名前\n', '\ud800', '😀'.repeat(256)])('既存名前規約のraw/Unicode境界を改名でも保持 %p', name => { expect(renameFolderValidation(name, '理由')).not.toBeNull(); });
test.each(['😀'.repeat(255), 'e\u0301'.repeat(255), '\ufeff', '\u2003名前\u2003'])('trim/NFC/255 scalarsを改名でも受理 %p', name => { expect(renameFolderValidation(name, '理由')).toBeNull(); });
test.each(['', 'a\u0085b', 'あ'.repeat(342), '\ud800'])('理由のUTF8/制御境界は変更理由として表示 %p', reason => {
  expect(renameFolderValidation('資料', reason)).toMatch(/^変更理由/); expect(rootFolderValidation('資料', reason)).toMatch(/^作成理由/);
});
test('理由のtrim後1024 bytesを受理しBOMを誤trimしない', () => {
  expect(renameFolderValidation('資料', `\u0085${'あ'.repeat(341)}a\u0085`)).toBeNull(); expect(folderName('\ufeff名前\ufeff')).toBe('\ufeff名前\ufeff');
});
test('owner/固定path/body/context/currentName/expectedChangedとreceiptのidentityを保持する', async () => {
  const h = harness(); expect(folderRenameOperations(h.owner)).toBe(h.store); expect(folderRenameOperations({}).get()).toBeUndefined();
  await h.run(); const saved = h.store.get()!; expect(saved).toMatchObject({ targetFolderId, context, currentName: context.name, expectedChanged: true, result });
  expect(Object.isFrozen(saved.request)).toBe(true); expect(Object.isFrozen(saved.context)).toBe(true);
  expect(Object.keys(saved.request).sort()).toEqual(['expectedFolderRevision', 'name', 'operationId', 'reason']);
  await h.run({ ...request, operationId: 'new' }); expect(h.send).toHaveBeenCalledTimes(1);
  expect(h.store.clearSettled({ ...saved })).toBe(false); expect(h.store.clearSettled(saved)).toBe(true);
});
test('pending連打/clearは拒否し再送には同一pathと同一body/contextを使う', async () => {
  const h = harness(); let finish!: (value: MutationResult) => void; h.send.mockReturnValueOnce(new Promise<MutationResult>(resolve => { finish = resolve; }));
  const first = h.run(); const pending = h.store.get()!; await h.run(); expect(h.send).toHaveBeenCalledTimes(1); expect(h.store.clearSettled(pending)).toBe(false);
  finish(result); await first; h.store.clearSettled(h.store.get()!);
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); const unknown = h.store.get()!; expect(h.store.clearSettled(unknown)).toBe(false);
  await sendFolderRenameOperation({ store: h.store, targetFolderId: 'other', context: { ...context, folderId: 'other' }, currentName: 'other', request: { ...request, name: 'other', operationId: 'other' }, send: h.send, invalidate: h.invalidate });
  expect(h.send.mock.calls[2]).toEqual([targetFolderId, unknown.request]); expect(h.send.mock.calls[2]![1]).toBe(unknown.request);
  expect(h.store.get()).toMatchObject({ status: 'succeeded', currentName: context.name, context });
});
test.each([['VALIDATION_FAILED', 422], ['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['ROOT_PROTECTED', 409], ['BUSINESS_RULE_REJECTED', 422]])('初回exact %s/%iだけを確定拒否としてProblemを保持する', async (code, status) => {
  const h = harness(); const error = problem(code as string, status as number); h.send.mockRejectedValue(error); await h.run();
  expect(h.store.get()).toMatchObject({ status: 'rejected', error }); expect(h.invalidate).not.toHaveBeenCalled(); h.store.clearSettled(h.store.get()!);
});
test.each([['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['VALIDATION_FAILED', 422]])('UNKNOWN後の%sは同要求をUNKNOWNのまま保持する', async (code, status) => {
  const h = harness(); h.send.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); const saved = h.store.get()!; await h.run();
  expect(h.store.get()).toMatchObject({ status: 'unknown' }); expect(h.send.mock.calls[1]![0]).toBe(targetFolderId); expect(h.send.mock.calls[1]![1]).toBe(saved.request);
  expect(h.store.get()!.context).toBe(saved.context); expect(h.invalidate).not.toHaveBeenCalled(); await h.run();
});
test.each([null, undefined, { ...result, operationId: 'other' }, { ...result, resourceId: 'parent' }, { ...result, changed: false }, { ...result, changed: 1 }, { ...result, resultingRevision: 17 }, { ...result, resultingRevision: Number.MAX_SAFE_INTEGER + 1 }, ...['123', '2026-02-30T00:00:00Z', '2026-10-06', '2026-10-06T00:00:00', '2026-10-06T24:00:00Z', undefined].map(occurredAt => ({ ...result, occurredAt }))])('不正/不足receiptはUNKNOWNにしてinvalidateしない %p', async receipt => {
  const h = harness(); h.send.mockResolvedValueOnce(receipt); await h.run(); expect(h.store.get()!.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled(); await h.run();
});
test('no-opでchanged true receiptもUNKNOWN', async () => {
  const h = harness(); await h.run({ ...request, name: context.name }); expect(h.store.get()!.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled();
  h.send.mockResolvedValue({ ...result, changed: false, resultingRevision: 17 }); await h.run();
});
test.each(['2026-10-06T00:00:00.123456Z', '2026-10-06T09:00:00+09:00'])('厳密な精度/offsetの日時を受理 %p', async occurredAt => {
  const h = harness(); h.send.mockResolvedValue({ ...result, occurredAt }); await h.run(); expect(h.store.get()!.status).toBe('succeeded');
});
test('refresh失敗を保持して成功receiptをUNKNOWNへ戻さない', async () => {
  const h = harness(); h.invalidate.mockRejectedValue(new Error('refresh failed')); await h.run(); expect(h.store.get()).toMatchObject({ status: 'succeeded', result, refresh: 'failed' });
});
test('遅延refresh完了でack済みの別操作を復活/clearしない', async () => {
  const h = harness(); let finish!: () => void; h.invalidate.mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve; })); const pending = h.run();
  await Promise.resolve(); const succeeded = h.store.get()!; expect(succeeded.status).toBe('succeeded'); h.store.clearSettled(succeeded);
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run({ ...request, operationId: 'new' }); const unknown = h.store.get(); finish(); await pending; expect(h.store.get()).toBe(unknown); await h.run();
});
test('別storeのackはUNKNOWN beforeunloadを解除しない', async () => {
  const h = harness(); const create = rootFolderOperations(h.owner);
  create.put({ request: { operationId: 'create', folderId: 'child', parentFolderId: 'parent', expectedParentRevision: 0, name: 'child', reason: 'reason' }, status: 'unknown' });
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run(); await h.run(); h.store.clearSettled(h.store.get()!);
  const event = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(event); expect(event.defaultPrevented).toBe(true);
  const settled = { ...create.get()!, status: 'rejected' as const }; create.put(settled); create.clearSettled(settled);
});
