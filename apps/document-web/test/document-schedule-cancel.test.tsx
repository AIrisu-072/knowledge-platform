import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), getDocument: jest.fn(), getDocumentVersion: jest.fn(), listVersionFiles: jest.fn(),
  listDocumentVersions: jest.fn(), listDocumentRevisions: jest.fn(), getDocumentHistory: jest.fn(),
  cancelPublicationSchedule: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const versionId = '00000000-0000-4000-8000-000000000011';
const otherVersionId = '00000000-0000-4000-8000-000000000012';
const scheduleId = '0199387a-0000-7000-8000-000000000001';
const nextScheduleId = '0199387a-0000-7000-8000-000000000002';
const available = { status: 'available' };
const denied = { status: 'disabled', reason: 'permission' };
const scheduledAt = '2026-10-07T01:00:00Z';
function version(id = versionId) {
  return { versionId: id, versionNo: id === versionId ? 2 : 1, lifecycleState: 'working', isCurrent: false, baseVersionId: null,
    createdAt: '2026-10-01T00:00:00Z', updatedAt: '2026-10-01T00:00:00Z', approvedAt: null, scheduledPublishAt: scheduledAt,
    publishedAt: null, withdrawnAt: null, firstReadAt: null, fileSummary: { authoritativeItemCount: 0, totalSizeBytes: 0, primary: null },
    currentPublicationScheduleId: scheduleId,
    capabilities: { cancelPublicationSchedule: available, publish: denied, schedulePublication: denied, download: denied, edit: denied, rebase: denied, withdraw: denied } };
}
function detail(id = documentId, revision = 7) {
  return { documentId: id, title: id === documentId ? '合成文書' : '別文書', lifecycleState: 'working', folderId: null, revision,
    currentVersionId: null, metadata: {}, readState: { isRead: false, firstReadAt: null },
    displayVersion: { ...version(), lifecycleState: 'WORKING' }, displayRevision: null,
    displayTimestamp: { kind: 'workingUpdatedAt', value: '2026-10-01T00:00:00Z' },
    capabilities: { createVersion: denied, manageAccess: denied, compareVersions: denied, updateMetadata: denied, endPublication: denied, moveDocument: denied } };
}
function setup() {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation((id: string) => Promise.resolve(detail(id)));
  api.getDocumentVersion.mockImplementation((_id: string, id: string) => Promise.resolve(version(id)));
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.listDocumentVersions.mockResolvedValue({ items: [version(), version(otherVersionId)], nextCursor: null });
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.cancelPublicationSchedule.mockImplementation((id: string, targetVersionId: string, body: { operationId: string; publishOperationId: string; expectedRevision: number }) => Promise.resolve({ ...body, documentId: id, targetVersionId, resultingRevision: body.expectedRevision + 1 }));
  const root = createRootRoute({ component: Outlet });
  const page = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const router = createRouter({ routeTree: root.addChildren([page]), history: createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=authoring&tab=versions&versionId=${versionId}`] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  const view = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, invalidate, ...view };
}
async function open() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: '公開予約を取り消す' }));
  return { user, dialog: await screen.findByRole('dialog', { name: '公開予約の取消' }) };
}
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: /予約を取り消す|同じ内容で再試行|処理中/ }).closest('form')!); }
function problem(code: string, status: number) { return { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false }; }
async function go(router: ReturnType<typeof setup>['router'], id: string, selected = versionId) {
  await act(async () => { await router.navigate({ to: '/documents/$documentId', params: { documentId: id }, search: validateDetailSearch({ view: 'authoring', tab: 'versions', versionId: selected }) }); });
}
function deferred<T = unknown>() { let resolve!: (value: T) => void; let reject!: (value: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }

 test('正規予約IDと現在capabilityを使い確認後だけ既存取消契約を送る', async () => {
  const { api, invalidate } = setup(); const { dialog, user } = await open();
  expect(within(dialog).getByText('合成文書 · 版 2')).toBeVisible();
  expect(within(dialog).getByText('2026/10/07 10:00 (Asia/Tokyo, UTC+09:00)')).toBeVisible();
  expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole('button', { name: '戻る' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  const reopened = await open(); submit(reopened.dialog);
  await screen.findByText('公開予約を取り消しました');
  expect(api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
  expect(api.cancelPublicationSchedule).toHaveBeenCalledWith(documentId, versionId, { operationId: expect.stringMatching(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/), publishOperationId: scheduleId, expectedRevision: 7 });
  for (const key of ['document', 'document-version', 'document-versions', 'document-history', 'document-revisions']) expect(invalidate).toHaveBeenCalledWith({ queryKey: [key, documentId] }, expect.anything());
});

test.each([null, undefined])('正規予約identity不在では履歴から推測して取消しない %p', async id => {
  const { api } = setup(); api.getDocumentVersion.mockResolvedValue({ ...version(), currentPublicationScheduleId: id });
  api.getDocumentHistory.mockResolvedValue({ items: [{ sourceKey: `schedule:${scheduleId}` }], nextCursor: null });
  await screen.findByRole('heading', { name: 'コンテンツ版' });
  expect(screen.queryByRole('button', { name: '公開予約を取り消す' })).not.toBeInTheDocument();
  expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
});

test('現在capabilityが無効では取消操作を表示しない', async () => {
  const { api } = setup(); api.getDocumentVersion.mockResolvedValue({ ...version(), capabilities: { ...version().capabilities, cancelPublicationSchedule: denied } });
  await screen.findByRole('heading', { name: 'コンテンツ版' });
  expect(screen.queryByRole('button', { name: '公開予約を取り消す' })).not.toBeInTheDocument();
});

test('二重送信と送信中のEscape/戻るを抑止する', async () => {
  const { api } = setup(); api.cancelPublicationSchedule.mockReturnValue(new Promise(() => {}));
  const { dialog, user } = await open(); act(() => { submit(dialog); submit(dialog); });
  expect(api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '戻る' })).toBeDisabled());
  await user.keyboard('{Escape}'); expect(dialog).toBeVisible();
});

test('通信結果不明は同じ予約ID operationId OCC payloadを再送し新しい予約に置換しない', async () => {
  const { api, client } = setup(); api.cancelPublicationSchedule.mockRejectedValueOnce(new Error('通信断'));
  const { dialog } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const first = api.cancelPublicationSchedule.mock.calls[0];
  act(() => { client.setQueryData(['document', documentId, 'authoring'], detail(documentId, 20)); client.setQueryData(['document-version', documentId, versionId, 'authoring'], { ...version(), currentPublicationScheduleId: nextScheduleId }); });
  submit(dialog); await screen.findByText('公開予約を取り消しました');
  expect(api.cancelPublicationSchedule.mock.calls[1]).toEqual(first);
});

test.each([['REVISION_CONFLICT', 409], ['FORBIDDEN', 403], ['BUSINESS_RULE_REJECTED', 422]])('確定拒否 %s は再送せず最新状態確認を求める', async (code, status) => {
  const { api } = setup(); api.cancelPublicationSchedule.mockRejectedValue(problem(code as string, status as number));
  const { dialog } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '最新状態を確認' });
  expect(within(dialog).queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
  expect(api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
});

test('確認中の予約差替えまたは権限失効で未送信確認を無効にする', async () => {
  const { api, client } = setup(); const { dialog } = await open();
  act(() => { client.setQueryData(['document-version', documentId, versionId, 'authoring'], { ...version(), currentPublicationScheduleId: nextScheduleId }); });
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '予約を取り消す' })).toBeDisabled()); submit(dialog);
  expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
});

test('確認中の予約実行でcurrent IDがnullになれば取消を送らない', async () => {
  const { api, client } = setup(); const { dialog } = await open();
  act(() => { client.setQueryData(['document-version', documentId, versionId, 'authoring'], { ...version(), lifecycleState: 'published', currentPublicationScheduleId: null, scheduledPublishAt: null }); });
  submit(dialog); expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
});

test('別版への移動は確認を閉じ、古い応答を選択版へ表示しない', async () => {
  const { api, router } = setup(); const response = deferred(); api.cancelPublicationSchedule.mockReturnValue(response.promise);
  const { dialog } = await open(); submit(dialog); const body = api.cancelPublicationSchedule.mock.calls[0][2];
  await go(router, documentId, otherVersionId); await screen.findByRole('heading', { name: '選択中: WORKING · 版 1' });
  await act(async () => response.resolve({ ...body, documentId, targetVersionId: versionId, resultingRevision: 8 }));
  expect(screen.queryByText('公開予約を取り消しました')).not.toBeInTheDocument();
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

test('Back/Forwardを跨いでも未確認intentを保持し別Documentへ結果を適用しない', async () => {
  const { api, router } = setup(); api.cancelPublicationSchedule.mockRejectedValueOnce(new Error('通信断'));
  const { dialog } = await open(); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const first = api.cancelPublicationSchedule.mock.calls[0]; await go(router, otherId);
  expect(screen.queryByRole('button', { name: '未確認の取消を開く' })).not.toBeInTheDocument();
  await act(async () => router.history.back());
  await userEvent.click(await screen.findByRole('button', { name: '未確認の取消を開く' }));
  const reopened = await screen.findByRole('dialog', { name: '公開予約の取消' }); submit(reopened);
  await screen.findByText('公開予約を取り消しました'); expect(api.cancelPublicationSchedule.mock.calls[1]).toEqual(first);
});

test('不一致の成功応答は結果不明として元intentを保持する', async () => {
  const { api } = setup(); api.cancelPublicationSchedule.mockResolvedValue({ operationId: 'other', publishOperationId: scheduleId, documentId, targetVersionId: versionId, resultingRevision: 8 });
  const { dialog } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  expect(screen.queryByText('公開予約を取り消しました')).not.toBeInTheDocument();
});

test('確認中のcapability失効を現在cacheでも検査する', async () => {
  const { api, client } = setup(); const { dialog } = await open();
  act(() => { client.setQueryData(['document-version', documentId, versionId, 'authoring'], { ...version(), capabilities: { ...version().capabilities, cancelPublicationSchedule: denied } }); });
  submit(dialog); expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '予約を取り消す' })).toBeDisabled());
});

test('確認中のDocument revision変更を現在cacheでも検査する', async () => {
  const { api, client } = setup(); const { dialog } = await open();
  act(() => { client.setQueryData(['document', documentId, 'authoring'], detail(documentId, 8)); });
  submit(dialog); expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
});

test('通信結果不明後の権限失効応答は過去の成功を否定せず同intentを保持する', async () => {
  const { api } = setup(); api.cancelPublicationSchedule.mockRejectedValueOnce(new Error('通信断')).mockRejectedValueOnce(problem('FORBIDDEN', 403));
  const { dialog } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' }); submit(dialog);
  await waitFor(() => expect(api.cancelPublicationSchedule).toHaveBeenCalledTimes(2));
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  expect(within(dialog).queryByRole('button', { name: '最新状態を確認' })).not.toBeInTheDocument();
  expect(api.cancelPublicationSchedule.mock.calls[1]).toEqual(api.cancelPublicationSchedule.mock.calls[0]);
});

test('結果不明の確認を閉じても再表示では同じintentを再開する', async () => {
  const { api } = setup(); api.cancelPublicationSchedule.mockRejectedValueOnce(new Error('通信断'));
  const { dialog, user } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  await user.click(within(dialog).getByRole('button', { name: '戻る' }));
  expect(screen.getByRole('button', { name: '公開予約を取り消す' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: '未確認の取消を開く' }));
  const resumed = await screen.findByRole('dialog', { name: '公開予約の取消' }); submit(resumed);
  await screen.findByText('公開予約を取り消しました');
  expect(api.cancelPublicationSchedule.mock.calls[1]).toEqual(api.cancelPublicationSchedule.mock.calls[0]);
});

test('取消成功後の再読取失敗は成功を保持して取消を再送しない', async () => {
  const { api } = setup(); const { dialog } = await open();
  api.getDocument.mockRejectedValue(new Error('読取失敗')); submit(dialog);
  await screen.findByText('公開予約を取り消しました');
  await waitFor(() => expect(api.getDocument).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
  expect(api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
});

test.each(['success', 'forbidden'])('取消成功と遅い正式改訂refreshを分け、read結果 %s でも取消は1回だけ送る', async outcome => {
  const h = setup();
  const revisionPage = { items: [{ revisionId: '00000000-0000-4000-8000-000000000101', documentVersionId: otherVersionId,
    major: 1, minor: 0, label: '1.0', createdAt: '2026-10-01T00:00:00Z', sourceKind: 'initialPublication', metadataSnapshotStatus: 'complete' }], nextCursor: null };
  h.api.listDocumentRevisions.mockResolvedValue(revisionPage);
  const { dialog } = await open();
  const refreshed = deferred<typeof revisionPage>();
  h.api.listDocumentRevisions.mockReturnValue(refreshed.promise);
  h.api.getDocument.mockResolvedValue(detail(documentId, 8));
  h.api.getDocumentVersion.mockResolvedValue({ ...version(), currentPublicationScheduleId: null, scheduledPublishAt: null,
    capabilities: { ...version().capabilities, cancelPublicationSchedule: denied, schedulePublication: available } });
  try {
    submit(dialog);
    await screen.findByText('公開予約を取り消しました');
    await waitFor(() => expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getAllByRole('status').map(node => node.textContent)).toEqual(['正式改訂を読み直し中…', '公開予約を取り消しました']);
    expect(screen.getByText('正式改訂を読み直し中…')).toHaveAttribute('aria-live', 'polite');
    // runtimeと同じ成功確認を、実readがまだpendingの間にも厳密に行う。
    expect(within(screen.getByRole('region', { name: '公開予約の取消操作' })).getByRole('status')).toHaveTextContent(/^公開予約を取り消しました$/);
    expect(screen.queryByRole('button', { name: '公開予約を取り消す' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '予約公開する' })).toBeEnabled();
    const restart = screen.getByRole('button', { name: '正式改訂を最初から読み直す' });
    expect(restart).toBeDisabled();
    const revisions = screen.getByRole('list', { name: '正式改訂一覧' });
    expect(within(revisions).getByText('1.0')).toBeVisible();
    expect(h.api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
    if (outcome === 'success') {
      await act(async () => refreshed.resolve(revisionPage));
      await waitFor(() => expect(screen.queryByText('正式改訂を読み直し中…')).not.toBeInTheDocument());
      expect(restart).toBeEnabled();
      expect(within(revisions).getByText('1.0')).toBeVisible();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    } else {
      await act(async () => refreshed.reject(problem('FORBIDDEN', 403)));
      await screen.findByRole('alert');
      expect(within(revisions).queryByText('1.0')).not.toBeInTheDocument();
      expect(screen.queryByText('正式改訂はありません。WORKING版は上の版一覧に表示されます。')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
    }
    expect(within(screen.getByRole('region', { name: '公開予約の取消操作' })).getByRole('status')).toHaveTextContent(/^公開予約を取り消しました$/);
    expect(h.api.cancelPublicationSchedule).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => refreshed.resolve(revisionPage));
    h.unmount(); h.client.clear();
  }
});

test('取消の未知結果だけはページ離脱前の警告を保持し成功後に解除する', async () => {
  const { api } = setup(); api.cancelPublicationSchedule.mockRejectedValueOnce(new Error('通信断'));
  const { dialog } = await open(); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const first = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(first); expect(first.defaultPrevented).toBe(true);
  submit(dialog); await screen.findByText('公開予約を取り消しました');
});

test('選択版の再読取エラーではcacheに古いcapabilityと予約IDがあっても開始できない', async () => {
  const { api, client } = setup(); await screen.findByRole('button', { name: '公開予約を取り消す' });
  api.getDocumentVersion.mockRejectedValue(new Error('読取失敗'));
  await act(async () => { await client.invalidateQueries({ queryKey: ['document-version', documentId] }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: '公開予約を取り消す' })).not.toBeInTheDocument());
});

test('確認中に再読取が開始された直後も新規取消を送らない', async () => {
  const { api, client } = setup(); const { dialog } = await open();
  api.getDocumentVersion.mockReturnValue(new Promise(() => {}));
  act(() => { void client.invalidateQueries({ queryKey: ['document-version', documentId] }); });
  submit(dialog); expect(api.cancelPublicationSchedule).not.toHaveBeenCalled();
});

test('成功後に取消triggerが消えても選択tabへkeyboard focusを戻す', async () => {
  const { api } = setup(); const { dialog } = await open();
  api.getDocumentVersion.mockResolvedValue({ ...version(), currentPublicationScheduleId: null, scheduledPublishAt: null,
    capabilities: { ...version().capabilities, cancelPublicationSchedule: denied } });
  submit(dialog); await screen.findByText('公開予約を取り消しました');
  await waitFor(() => expect(screen.getByRole('tab', { name: '版・改訂' })).toHaveFocus());
});
