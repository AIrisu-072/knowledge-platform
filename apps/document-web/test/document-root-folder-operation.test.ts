import { rootFolderOperations, sendRootFolderOperation, folderName, rootFolderValidation } from '../src/application/document-root-folder';
import type { CommandsCreateFolder, MutationResult } from '@knowledge-platform/document-api-client';

const request: CommandsCreateFolder = { operationId: '019a0010-0000-7000-8000-000000000001', folderId: '019a0010-0000-7000-8000-000000000002', parentFolderId: '019a0010-0000-7000-8000-000000000003', expectedParentRevision: 17, name: '資料', reason: '合成理由' };
const result: MutationResult = { operationId: request.operationId, resourceId: request.folderId, changed: true, resultingRevision: 0, occurredAt: '2026-10-05T12:00:00Z' };
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: status >= 500 });
function harness() { const owner = {}; const store = rootFolderOperations(owner); const send = jest.fn().mockResolvedValue(result); const invalidate = jest.fn().mockResolvedValue(undefined); return { owner, store, send, invalidate, run: (body = request) => sendRootFolderOperation({ store, request: body, send, invalidate }) }; }

test.each(['', ' ', '.', '..', 'x/y', 'x\\y', '\t名前', '名前\n', '\u0085名前\u0085', '\u0000', '😀'.repeat(256), '\ud800'])('raw Cc・非scalar・NFC後の境界を拒否 %p', name => {
  expect(rootFolderValidation(name, '理由')).not.toBeNull();
});
test.each(['😀'.repeat(255), 'e\u0301'.repeat(255), '\ufeff', '\u2003名前\u2003'])('Rust trim→NFC後の255 scalarsまでを受理 %p', name => {
  expect(rootFolderValidation(name, '理由')).toBeNull();
});
test('Rust White_SpaceとNFCを使用しBOMを誤trimしない', () => {
  expect(folderName('\u2003e\u0301\u2003')).toBe('é'); expect(folderName('\ufeff名前\ufeff')).toBe('\ufeff名前\ufeff');
});
test.each(['', '\u0085', 'a\u0085b', 'あ'.repeat(342), '\ud800'])('理由の制御文字・空値・byte上限・非scalarを拒否 %p', reason => { expect(rootFolderValidation('資料', reason)).not.toBeNull(); });
test('理由はtrim後1024 UTF8 bytesを受理する', () => { expect(rootFolderValidation('資料', `\u0085${'あ'.repeat(341)}a\u0085`)).toBeNull(); });

test('QueryClient単位に固定要求と成功receiptを持ち、異なるownerとは共有しない', async () => {
  const h = harness(); expect(rootFolderOperations(h.owner)).toBe(h.store); expect(rootFolderOperations({}).get()).toBeUndefined();
  await h.run(); const operation = h.store.get()!;
  expect(operation).toMatchObject({ status: 'succeeded', result }); expect(Object.isFrozen(operation.request)).toBe(true);
  await h.run({ ...request, operationId: 'other' }); expect(h.send).toHaveBeenCalledTimes(1);
  expect(h.store.clearSettled({ ...operation })).toBe(false); expect(h.store.get()).toBe(operation);
  expect(h.store.clearSettled(operation)).toBe(true); expect(h.store.get()).toBeUndefined();
});

test('pendingは再送/別要求とclearを拒否し、同時送信を一つに限定する', async () => {
  const h = harness(); let finish!: (value: MutationResult) => void; h.send.mockReturnValue(new Promise<MutationResult>(resolve => { finish = resolve; }));
  const first = h.run(); const pending = h.store.get()!; expect(pending.status).toBe('pending');
  expect(h.store.clearSettled(pending)).toBe(false); await h.run(); expect(h.send).toHaveBeenCalledTimes(1);
  finish(result); await first; expect(h.store.get()!.status).toBe('succeeded');
});

test.each([['VALIDATION_FAILED', 422], ['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['ROOT_PROTECTED', 409], ['BUSINESS_RULE_REJECTED', 422]])('初回のexact %s/%iだけを確定拒否とする', async (code, status) => {
  const h = harness(); h.send.mockRejectedValue(problem(code as string, status as number)); await h.run(); expect(h.store.get()!.status).toBe('rejected'); expect(h.invalidate).not.toHaveBeenCalled();
});

test.each([new Error('lost'), { status: 403 }, problem('FORBIDDEN', 500), problem('UNKNOWN', 409), problem('INTERNAL', 500), problem('COMMIT_OUTCOME_UNKNOWN', 503)])('非Problem/曖昧errorはunknownのまま %p', async error => {
  const h = harness(); h.send.mockRejectedValueOnce(error); await h.run(); const unknown = h.store.get()!; expect(unknown.status).toBe('unknown'); expect(h.store.clearSettled(unknown)).toBe(false);
  await h.run({ ...request, operationId: 'new', expectedParentRevision: 99, name: '別内容' }); expect(h.send.mock.calls[1]![0]).toBe(unknown.request); expect(h.store.get()!.status).toBe('succeeded');
});

test.each([['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404], ['REVISION_CONFLICT', 409], ['OPERATION_CONFLICT', 409], ['VALIDATION_FAILED', 422]])('unknown後の%sは未作成の証明にならず、同一要求だけを維持', async (code, status) => {
  const h = harness(); h.send.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem(code as string, status as number)); await h.run(); const fixed = h.store.get()!.request; await h.run();
  expect(h.store.get()).toMatchObject({ status: 'unknown', request: fixed }); expect(h.send.mock.calls[1]![0]).toBe(fixed);
  await h.run(); expect(h.store.get()!.status).toBe('succeeded');
});

test.each([{ operationId: 'other' }, { resourceId: request.parentFolderId }, { changed: false }, { resultingRevision: 18 }, { resultingRevision: -1 }, { occurredAt: 'not a date' }, { occurredAt: undefined }])('応答の不一致はunknownにしてinvalidateしない %p', async patch => {
  const h = harness(); h.send.mockResolvedValueOnce({ ...result, ...patch }); await h.run(); expect(h.store.get()!.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled(); await h.run();
});

test('invalidate失敗は確定成功をunknownへ戻さない', async () => {
  const h = harness(); h.invalidate.mockRejectedValue(new Error('refresh')); await h.run(); expect(h.store.get()).toMatchObject({ status: 'succeeded', result });
});

test('古い確定拒否のrefresh完了では新しいpending/unknownをclearできない', async () => {
  const h = harness(); h.send.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); await h.run(); const rejected = h.store.get()!; expect(h.store.clearSettled(rejected)).toBe(true);
  h.send.mockRejectedValueOnce(new Error('lost')); await h.run({ ...request, operationId: 'next' }); const unknown = h.store.get();
  expect(h.store.clearSettled(rejected)).toBe(false); expect(h.store.get()).toBe(unknown); await h.run();
});

test.each(['123', '2026-02-30T12:00:00Z', '2026-10-05', '2026-10-05T12:00:00', '2026-10-05T24:00:00Z'])('occurredAtの非日時・暦日繰上り・timezone欠損は照合不成立 %s', async occurredAt => {
  const h = harness(); h.send.mockResolvedValueOnce({ ...result, occurredAt }); await h.run(); expect(h.store.get()!.status).toBe('unknown'); expect(h.invalidate).not.toHaveBeenCalled(); await h.run();
});
test.each(['2026-10-05T12:00:00.123456Z', '2026-10-05T21:00:00+09:00'])('backendの精度・UTC offset付きoccurredAtを受理 %s', async occurredAt => {
  const h = harness(); h.send.mockResolvedValue({ ...result, occurredAt }); await h.run(); expect(h.store.get()!.status).toBe('succeeded');
});

test('pending/unknownの離脱警告はcomponentに依存せず、確定結果で自分のlistenerを解除する', async () => {
  const add = jest.spyOn(window, 'addEventListener'); const remove = jest.spyOn(window, 'removeEventListener');
  const h = harness(); h.send.mockRejectedValueOnce(new Error('lost')); await h.run();
  const listener = add.mock.calls.find(([name]) => name === 'beforeunload')![1];
  expect(h.store.get()!.status).toBe('unknown'); expect(add).toHaveBeenLastCalledWith('beforeunload', listener);
  await h.run(); expect(h.store.get()!.status).toBe('succeeded'); expect(remove).toHaveBeenLastCalledWith('beforeunload', listener);
});
