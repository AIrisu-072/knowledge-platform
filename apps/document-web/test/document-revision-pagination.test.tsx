import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentRevisionPage, type DocumentRevisionSummary, type DocumentDetail, type RevisionComparisonResponse } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';
import { workingOperationKey, type WorkingOperation, type WorkingWriteIntent } from '../src/application/document-working-version';
import { metadataOperations, type MetadataOperation } from '../src/application/document-metadata';
import { refreshLifecycleQueries } from '../src/application/document-lifecycle-operations';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), getDocument: jest.fn(), listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(),
  listDocumentRevisions: jest.fn(), compareDocumentRevisions: jest.fn(), listVersionFiles: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const versionId = '00000000-0000-4000-8000-000000000011';
const available = { status: 'available' as const }; const denied = { status: 'disabled' as const, reason: 'permission' as const };
const revision = (index: number): DocumentRevisionSummary => ({ revisionId: `00000000-0000-4000-8000-${String(index).padStart(12, '0')}`, documentVersionId: versionId, major: index, minor: 0, label: `${index}.0`, createdAt: '2026-10-01T00:00:00Z', sourceKind: 'metadataRevision', metadataSnapshotStatus: 'complete' });
const first = revision(102); const second = revision(101); const tail = revision(100);
const page = (items: DocumentRevisionSummary[], nextCursor: string | null = null): DocumentRevisionPage => ({ items, nextCursor });
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const more = () => screen.getByRole('button', { name: '正式改訂をさらに表示' });
const restart = () => screen.getByRole('button', { name: '正式改訂を最初から読み直す' });
const timeline = () => within(screen.getByRole('heading', { name: '正式改訂' }).closest('section')!).getByRole('list');
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => { metadataOperations(client).clear(documentId); client.clear(); }));
function detail(id = documentId): DocumentDetail {
  return { documentId: id, documentVersionId: versionId, title: id === documentId ? '合成文書' : '別文書', folderId: null, folderName: null, revision: 7, metadata: {}, createdAt: '2026-10-01T00:00:00Z', currentVersionId: versionId, unread: false, publishedAt: '2026-10-01T00:00:00Z',
    displayVersion: { versionId, versionNo: 1, baseVersionId: null, lifecycleState: 'PUBLISHED', isCurrent: true, approvedAt: null, scheduledPublishAt: null, publishedAt: null, withdrawnAt: null, updatedAt: '2026-10-01T00:00:00Z', fileSummary: { authoritativeItemCount: 0, totalSizeBytes: 0, primary: null } },
    displayRevision: first, readState: { isRead: true, firstReadAt: null }, displayTimestamp: { kind: 'revisionCreatedAt', value: '2026-10-01T00:00:00Z' },
    capabilities: { createVersion: denied, updateMetadata: denied, moveDocument: denied, endPublication: denied, manageAccess: denied, compareVersions: available } };
}
function fixedWorking(blob: Blob): WorkingOperation {
  return { status: 'unknown', intent: { kind: 'create', documentId, sourceVersionId: versionId,
    body: { operationId: 'fixed-working', targetVersionId: 'fixed-target', expectedRevision: 7, title: '合成作業版', items: [] },
    files: new Map([['fixed-part', blob]]), prepared: { body: blob, contentType: 'multipart/form-data; boundary=fixed' } } };
}
function setup(search = 'tab=versions', retry: boolean | number = false) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation(id => Promise.resolve(detail(id)));
  api.listDocumentVersions.mockResolvedValue({ items: [], nextCursor: null });
  api.listDocumentRevisions.mockResolvedValue(page([first, second]));
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.compareDocumentRevisions.mockImplementation((_id, pair) => {
    const result: RevisionComparisonResponse = {
    projection: 'display', baseRevision: { ...revision(Number(pair.baseRevisionId.slice(-12))) }, targetRevision: { ...revision(Number(pair.targetRevisionId.slice(-12))) },
    contentComparisonStatus: 'sameAuthoritativeVersion', metadataComparisonStatus: 'same', metadataChanges: [], displayItems: [], unverifiedRegions: [], nextCursor: null,
    changes: [], rows: [], ancillaryChanges: [], baseMetadataSnapshotDigest: 'base', targetMetadataSnapshotDigest: 'target', auditEventId: 'synthetic', pageSize: 50,
  }; return Promise.resolve(result); });
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=published&${search}`] });
  const router = createRouter({ routeTree: root.addChildren([route, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry, retryDelay: 0, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, history };
}
async function compare() { fireEvent.click(screen.getByRole('tab', { name: '新旧比較' })); await screen.findByRole('heading', { name: '正式改訂の比較' }); }
async function ready() { await screen.findByRole('heading', { name: '正式改訂' }); await waitFor(() => expect(within(timeline()).getAllByRole('listitem')).toHaveLength(2)); }

test('比較の基準と対象は新旧比較region内で厳密に確認し、現行版の対象要約を保持する', async () => {
  const base = { ...first, major: 1, minor: 1, label: '1.1' }, target = { ...second, major: 1, minor: 0, label: '1.0' };
  const h = setup(`tab=compare&baseRevisionId=${base.revisionId}&targetRevisionId=${target.revisionId}`);
  h.api.listDocumentRevisions.mockResolvedValue(page([base, target]));
  h.api.compareDocumentRevisions.mockResolvedValue({ projection: 'display', baseRevision: base, targetRevision: target,
    contentComparisonStatus: 'sameAuthoritativeVersion', metadataComparisonStatus: 'different', metadataChanges: [],
    displayItems: [], unverifiedRegions: [], nextCursor: null, changes: [], rows: [], ancillaryChanges: [],
    baseMetadataSnapshotDigest: 'base', targetMetadataSnapshotDigest: 'target', auditEventId: 'synthetic', pageSize: 50 });
  await screen.findByText('同じコンテンツ版のため本文比較なし');
  expect(screen.getAllByRole('region', { name: '新旧比較' })).toHaveLength(1);
  expect(screen.getByText('現行版 · Version 1')).toBeInTheDocument();
  const comparison = screen.getByRole('region', { name: '新旧比較' });
  expect(within(comparison).getByRole('combobox', { name: '基準改訂' })).toHaveValue(base.revisionId);
  expect(within(comparison).getByRole('combobox', { name: '比較対象' })).toHaveValue(target.revisionId);
  expect(screen.getAllByText('対象', { selector: 'dt', exact: true })).toHaveLength(2);
  const baseTerms = within(comparison).getAllByText('基準', { selector: 'dt', exact: true });
  const targetTerms = within(comparison).getAllByText('対象', { selector: 'dt', exact: true });
  expect(baseTerms).toHaveLength(1);
  expect(targetTerms).toHaveLength(1);
  expect(baseTerms[0]!.nextElementSibling).toHaveTextContent(/^1\.1$/);
  expect(targetTerms[0]!.nextElementSibling).toHaveTextContent(/^1\.0$/);
  fireEvent.click(screen.getByRole('button', { name: '← 版・改訂へ戻る' }));
  await ready();
  expect(screen.getByText('現行版 · Version 1')).toBeVisible();
});

// Without the continuation control, the 101st revision cannot be reached from the real route.
test('先頭100件からopaque cursorだけで101件目を追加し、終端とタブ共有を保つ', async () => {
  const h = setup(); const hundred = Array.from({ length: 100 }, (_, i) => revision(200 - i)); const last = revision(100); const cursor = 'opaque+/=?日本語';
  h.api.listDocumentRevisions.mockImplementation((_id, token) => Promise.resolve(token === cursor ? page([last]) : page(hundred, cursor)));
  await waitFor(() => expect(within(timeline()).getAllByRole('listitem')).toHaveLength(100));
  expect(within(timeline()).queryByText(last.label)).not.toBeInTheDocument(); fireEvent.click(more());
  await waitFor(() => expect(within(timeline()).getAllByRole('listitem')).toHaveLength(101));
  expect(h.api.listDocumentRevisions).toHaveBeenCalledWith(documentId, cursor); expect(screen.queryByRole('button', { name: '正式改訂をさらに表示' })).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole('combobox', { name: '基準' }), { target: { value: last.revisionId } });
  await compare(); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(last.revisionId);
  await screen.findByText('同じコンテンツ版のため本文比較なし');
  expect(screen.getByRole('combobox', { name: '比較対象' })).toHaveValue(hundred[0]!.revisionId);
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('button', { name: '← 版・改訂へ戻る' }));
  expect(await screen.findByRole('combobox', { name: '基準' })).toHaveValue(last.revisionId);
});

test('重複Revision IDはサーバー順の位置で一度だけ表示し次cursorを保つ', async () => {
  const h = setup(); h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(cursor === 'third' ? page([revision(99)]) : cursor ? page([second, tail], 'third') : page([first, second], 'next')));
  await ready(); fireEvent.click(more()); await waitFor(() => expect(within(timeline()).getAllByRole('listitem')).toHaveLength(3));
  expect(within(timeline()).getAllByRole('listitem').map(li => li.querySelector('strong')?.textContent)).toEqual([first.label, second.label, tail.label]);
  fireEvent.click(more()); await within(timeline()).findByText('99.0'); expect(h.api.listDocumentRevisions).toHaveBeenLastCalledWith(documentId, 'third');
});

test('同batchの連打とタブ往復でも続きは一要求で既取得列を保持する', async () => {
  const h = setup(); const pending = deferred<DocumentRevisionPage>(); h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor ? pending.promise : Promise.resolve(page([first, second], 'next')));
  await ready(); const button = more(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  expect(h.api.listDocumentRevisions.mock.calls.filter(([, cursor]) => cursor === 'next')).toHaveLength(1);
  await waitFor(() => expect(more()).toBeDisabled()); expect(restart()).toBeDisabled(); expect(screen.getByRole('status')).toHaveTextContent('正式改訂の続きを取得中');
  await compare(); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(second.revisionId);
  await act(async () => pending.resolve(page([tail]))); expect(await within(screen.getByRole('combobox', { name: '基準改訂' })).findByRole('option', { name: tail.label })).toBeInTheDocument();
});

test('続きの一時失敗は既取得行を残して同じcursorを再試行する', async () => {
  const h = setup(); let attempts = 0; h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor ? (++attempts === 1 ? Promise.reject(new Error('offline')) : Promise.resolve(page([tail]))) : Promise.resolve(page([first, second], 'same')));
  await ready(); fireEvent.click(more()); await screen.findByRole('alert'); expect(within(timeline()).getAllByRole('listitem')).toHaveLength(2);
  expect(screen.queryByText(/正式改訂はありません/)).not.toBeInTheDocument(); fireEvent.click(screen.getByRole('button', { name: '正式改訂の続きを再試行' }));
  await within(timeline()).findByText(tail.label); expect(h.api.listDocumentRevisions.mock.calls.filter(([, cursor]) => cursor === 'same')).toHaveLength(2); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_NOT_FOUND', 404]])('続きの%sでは過去の行と比較を閉じ、明示先頭readだけで回復する', async (code, status) => {
  const h = setup('tab=compare'); let fresh = false; h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor === 'old' ? Promise.reject(problem(code, Number(status))) : Promise.resolve(fresh ? page([first, tail], 'fresh') : page([first, second], 'old')));
  await screen.findByText('同じコンテンツ版のため本文比較なし'); fireEvent.click(more()); await screen.findByRole('alert');
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(screen.queryByRole('combobox', { name: '基準改訂' })).not.toBeInTheDocument();
  expect(screen.queryByText(/比較には2件以上/)).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: '正式改訂の続きを再試行' })).not.toBeInTheDocument();
  fresh = true; fireEvent.click(restart()); await screen.findByText('同じコンテンツ版のため本文比較なし'); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(tail.revisionId);
  expect(h.api.listDocumentRevisions).toHaveBeenLastCalledWith(documentId, undefined);
});

test.each(['versions', 'compare'])('初回拒否を%sの空表示や無期限比較loadingへ変換しない', async tab => {
  const h = setup(`tab=${tab}`); h.api.listDocumentRevisions.mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockResolvedValue(page([first, second]));
  await screen.findByRole('alert'); expect(screen.queryByText(/正式改訂はありません|比較には2件以上/)).not.toBeInTheDocument(); expect(screen.queryByText(/比較結果を取得中/)).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled();
  fireEvent.click(restart()); if (tab === 'versions') await ready(); else await screen.findByText('同じコンテンツ版のため本文比較なし');
});

test('コンテンツ版と正式改訂の初回loading/errorを独立して表示する', async () => {
  const h = setup(); const pending = deferred<DocumentRevisionPage>(); h.api.listDocumentRevisions.mockReturnValue(pending.promise);
  await screen.findByRole('heading', { name: '正式改訂' }); expect(screen.getByText(/正式改訂を取得中/)).toBeInTheDocument(); expect(screen.getByText('表示できるコンテンツ版はありません。')).toBeInTheDocument();
  await act(async () => pending.reject(problem('FORBIDDEN', 403))); await screen.findByRole('alert'); expect(screen.queryByText(/正式改訂はありません/)).not.toBeInTheDocument();
});

// Removing explicit-ID guards would issue an unrelated first-two comparison while the URL stays old.
test('未取得の明示比較IDを保持し、追加readで解決するまで別比較GETを止める', async () => {
  const h = setup(`tab=compare&baseRevisionId=${tail.revisionId}&targetRevisionId=${first.revisionId}`); h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([tail]) : page([first, second], 'next')));
  await screen.findByText(/未取得の選択/); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(tail.revisionId); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled(); expect(screen.queryByText(/比較結果を取得中/)).not.toBeInTheDocument();
  expect(h.router.state.location.search.baseRevisionId).toBe(tail.revisionId); fireEvent.click(more()); await screen.findByText('同じコンテンツ版のため本文比較なし');
  expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, expect.objectContaining({ baseRevisionId: tail.revisionId, targetRevisionId: first.revisionId }));
});

test.each(['baseRevisionId', 'targetRevisionId'])('未取得の%sは明示再選択で回復し、既定候補へ無言で置換しない', async key => {
  const h = setup(`tab=compare&${key}=${tail.revisionId}`); await screen.findByText(/未取得の選択/); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled();
  const label = key === 'baseRevisionId' ? '基準改訂' : '比較対象'; fireEvent.change(screen.getByRole('combobox', { name: label }), { target: { value: key === 'baseRevisionId' ? second.revisionId : first.revisionId } });
  await screen.findByText('同じコンテンツ版のため本文比較なし'); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1);
});

test('明示先頭再読取は古い比較を閉じてIDを保持し、再追加後に同じ比較を取得する', async () => {
  const h = setup(`tab=compare&baseRevisionId=${tail.revisionId}&targetRevisionId=${first.revisionId}`); let initial = true;
  h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(initial || cursor ? page([first, tail]) : page([first, second], 'new')));
  await screen.findByText('同じコンテンツ版のため本文比較なし'); initial = false; fireEvent.click(restart()); await screen.findByText(/未取得の選択/);
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.router.state.location.search.baseRevisionId).toBe(tail.revisionId); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1);
  fireEvent.click(more()); await screen.findByText('同じコンテンツ版のため本文比較なし'); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2);
});

test('遅延した旧文書の追加応答を別文書やURLへ混ぜない', async () => {
  const h = setup(); const delayed = deferred<DocumentRevisionPage>(); h.api.listDocumentRevisions.mockImplementation((id, cursor) => id === otherId ? Promise.resolve(page([revision(90), revision(89)])) : cursor ? delayed.promise : Promise.resolve(page([first, second], 'old')));
  await ready(); fireEvent.click(more()); await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'published', tab: 'versions' } }); });
  await within(timeline()).findByText('90.0'); await act(async () => delayed.resolve(page([tail]))); expect(within(timeline()).queryByText(tail.label)).not.toBeInTheDocument(); expect(h.router.state.location.pathname).toBe(`/documents/${otherId}`);
});

test('単ページcacheとページ列を分離し、prefix invalidateは新cursorから再構成する', async () => {
  const h = setup(); let changed = false; h.client.setQueryData(['document-revisions', documentId], page([revision(77)], 'single'));
  h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([changed ? revision(98) : tail]) : page([first, second], changed ? 'new' : 'old')));
  await ready(); fireEvent.click(more()); await within(timeline()).findByText(tail.label); changed = true;
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document-revisions', documentId] }); });
  await within(timeline()).findByText('98.0'); expect(within(timeline()).queryByText(tail.label)).not.toBeInTheDocument(); expect(h.api.listDocumentRevisions).toHaveBeenLastCalledWith(documentId, 'new'); expect(h.client.getQueryData(['document-revisions', documentId])).toEqual(page([revision(77)], 'single'));
});

test.each(['lifecycle', 'move'])('既存%s read更新はページ列へ適用され、固定要求/blob/providerを保持する', async kind => {
  const h = setup(); let changed = false; h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([tail]) : page([first, second], changed ? null : 'old')));
  await ready(); fireEvent.click(more()); await within(timeline()).findByText(tail.label);
  const blob = new Blob(['immutable']); const operation = fixedWorking(blob);
  const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed', expectedDocumentRevision: 7, set: { category: 'keep' }, unset: [], reason: '合成理由' } };
  h.client.setQueryData(workingOperationKey(documentId), operation); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'same' });
  changed = true; await act(async () => { if (kind === 'move') await refreshFolderMoveReads(h.client); else await refreshLifecycleQueries(h.client, documentId); });
  await waitFor(() => expect(within(timeline()).getAllByRole('listitem')).toHaveLength(2)); expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(operation); expect((h.client.getQueryData(workingOperationKey(documentId)) as { intent: WorkingWriteIntent }).intent.prepared.body).toBe(blob);
  expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'same' });
});

test('明示再読取は無関係queryと未確定操作を消さず、画面往復でも選択を保持する', async () => {
  const h = setup(`tab=versions&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`); await ready(); const fixed = fixedWorking(new Blob(['fixed'])); h.client.setQueryData(workingOperationKey(documentId), fixed); h.client.setQueryData(['unrelated'], 'keep');
  fireEvent.click(restart()); await ready(); expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixed); expect(h.client.getQueryData(['unrelated'])).toBe('keep');
  await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面'); await act(async () => h.history.back());
  expect(await screen.findByRole('combobox', { name: '基準' })).toHaveValue(second.revisionId); expect(h.router.state.location.search.targetRevisionId).toBe(first.revisionId);
});

test('比較GETの失敗は以前の結果を残さず、明示した対象の再読取で回復する', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`);
  await screen.findByText('同じコンテンツ版のため本文比較なし');
  const original = h.api.compareDocumentRevisions.getMockImplementation()!;
  h.api.compareDocumentRevisions.mockRejectedValueOnce(problem('STALE_COMPARISON_INPUT', 409)).mockImplementation(original);
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['revision-comparison', documentId] }); });
  const alert = await screen.findByRole('alert'); expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument();
  expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(second.revisionId); expect(h.router.state.location.search.targetRevisionId).toBe(first.revisionId);
  fireEvent.click(screen.getByRole('button', { name: '比較結果を最初から読み直す' })); await screen.findByText('同じコンテンツ版のため本文比較なし');
});

test('先頭再読取より遅い旧比較応答は未取得の選択へ戻った画面を復活させない', async () => {
  const h = setup(`tab=compare&baseRevisionId=${tail.revisionId}&targetRevisionId=${first.revisionId}`);
  const delayed = deferred<unknown>(); let restarted = false;
  h.api.listDocumentRevisions.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([tail]) : restarted ? page([first, second], 'next') : page([first, tail])));
  const original = h.api.compareDocumentRevisions.getMockImplementation()!;
  h.api.compareDocumentRevisions.mockReturnValueOnce(delayed.promise).mockImplementation(original);
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1)); restarted = true; fireEvent.click(restart());
  await screen.findByText(/未取得の選択/); await act(async () => delayed.resolve(await original(documentId, { baseRevisionId: tail.revisionId, targetRevisionId: first.revisionId })));
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(tail.revisionId);
  fireEvent.click(more()); await screen.findByText('同じコンテンツ版のため本文比較なし'); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2);
});

test('既存read resetより遅い追加応答は古いcursorや行を戻さない', async () => {
  const h = setup(); const delayed = deferred<DocumentRevisionPage>(); let fresh = false;
  h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor === 'old' ? delayed.promise : Promise.resolve(cursor === 'new' ? page([revision(98)]) : page([first, second], fresh ? 'new' : 'old')));
  await ready(); fireEvent.click(more()); fresh = true;
  await act(async () => { await refreshFolderMoveReads(h.client); }); await ready(); await act(async () => delayed.resolve(page([tail])));
  expect(within(timeline()).queryByText(tail.label)).not.toBeInTheDocument(); fireEvent.click(more()); await within(timeline()).findByText('98.0'); expect(h.api.listDocumentRevisions).toHaveBeenLastCalledWith(documentId, 'new');
});

test('cursor stale後のタブ往復は旧cursorを再使用せず、明示IDを失わない', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`);
  h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor ? Promise.reject(problem('CURSOR_STALE', 409)) : Promise.resolve(page([first, second], 'old')));
  await screen.findByText('同じコンテンツ版のため本文比較なし'); fireEvent.click(more()); await screen.findByRole('alert'); const count = h.api.listDocumentRevisions.mock.calls.length;
  fireEvent.click(screen.getByRole('button', { name: '← 版・改訂へ戻る' })); await screen.findByRole('heading', { name: '正式改訂' });
  expect(within(timeline()).queryAllByRole('listitem')).toHaveLength(0); expect(screen.queryByText(/正式改訂はありません/)).not.toBeInTheDocument(); await compare();
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(count); expect(h.router.state.location.search.baseRevisionId).toBe(second.revisionId); expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument();
});

test.each([0, 1])('成功した%d件の終端だけが空/比較不足を表示し、disabled比較をloadingへしない', async count => {
  const h = setup('tab=compare'); h.api.listDocumentRevisions.mockResolvedValue(page(count ? [first] : []));
  await screen.findByText(/比較には2件以上/); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled(); expect(screen.queryByText(/比較結果を取得中/)).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: '正式改訂をさらに表示' })).not.toBeInTheDocument();
});

test('コンテンツ版read拒否は成功した正式改訂のページ列を隠さない', async () => {
  const h = setup(); h.api.listDocumentVersions.mockRejectedValue(problem('FORBIDDEN', 403)); h.api.listDocumentRevisions.mockResolvedValue(page([first, second], 'next'));
  await ready(); await screen.findByRole('alert'); expect(within(timeline()).getAllByRole('listitem')).toHaveLength(2); expect(more()).toBeEnabled(); expect(screen.queryByText('表示できるコンテンツ版はありません。')).not.toBeInTheDocument();
});

test('同batchの明示先頭再読取は一要求で、未取得IDを既定値へ変えない', async () => {
  const h = setup(`tab=compare&baseRevisionId=${tail.revisionId}&targetRevisionId=${first.revisionId}`); await screen.findByText(/未取得の選択/);
  const pending = deferred<DocumentRevisionPage>(); h.api.listDocumentRevisions.mockReturnValue(pending.promise); const button = restart(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  await waitFor(() => expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2)); await act(async () => pending.resolve(page([first, second])));
  expect(await screen.findByText(/未取得の選択/)).toBeVisible(); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled(); expect(h.router.state.location.search.baseRevisionId).toBe(tail.revisionId);
});

// The production QueryClient retries once; a refused/expired continuation needs a fresh head read.
test('既存clientのretry1でも拒否済みcursorは自動再送せず明示先頭readへ止める', async () => {
  const h = setup('tab=versions', 1); h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor ? Promise.reject(problem('CURSOR_STALE', 409)) : Promise.resolve(page([first, second], 'expired')));
  await ready(); fireEvent.click(more()); await screen.findByRole('alert');
  expect(h.api.listDocumentRevisions.mock.calls.filter(([, cursor]) => cursor === 'expired')).toHaveLength(1);
  expect(restart()).toBeEnabled(); expect(screen.queryByRole('button', { name: '正式改訂の続きを再試行' })).not.toBeInTheDocument();
});

test('文書read拒否から再表示する時はfreshな正式改訂readを要求し旧比較を現在認可済みとして戻さない', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`);
  await screen.findByText('同じコンテンツ版のため本文比較なし');
  h.api.getDocument.mockRejectedValueOnce(problem('FORBIDDEN', 403)); h.api.listDocumentRevisions.mockRejectedValue(problem('FORBIDDEN', 403));
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); });
  const deniedRead = await screen.findByRole('alert'); expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument();
  fireEvent.click(within(deniedRead).getByRole('button', { name: '再読み込み' })); await screen.findByRole('heading', { name: '正式改訂の比較' });
  // Document readability cannot unlock the refused history; explicitly read its head.
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(1); fireEvent.click(restart());
  await waitFor(() => expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2)); await screen.findByRole('alert');
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1); expect(h.router.state.location.search.baseRevisionId).toBe(second.revisionId);
});

// Review I1: the production retry may succeed without ever populating detailQuery.error.
test.each([['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['DOCUMENT_NOT_FOUND', 404]])('review I1: Document %sの自動retry成功でも履歴を失効し明示先頭readまで止める', async (code, status) => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`, 1);
  await screen.findByText('同じコンテンツ版のため本文比較なし');
  const originalCompare = h.api.compareDocumentRevisions.getMockImplementation()!;
  h.api.getDocument.mockRejectedValueOnce(problem(code, Number(status)));
  h.api.listDocumentRevisions.mockRejectedValue(problem('FORBIDDEN', 403));
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); });
  await waitFor(() => expect(h.api.getDocument).toHaveBeenCalledTimes(3));
  await screen.findByRole('alert'); expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument();
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(1); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1);
  expect(h.router.state.location.search.baseRevisionId).toBe(second.revisionId);
  fireEvent.click(restart()); await waitFor(() => expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2)); await screen.findByRole('alert');
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument();
  h.api.listDocumentRevisions.mockResolvedValue(page([first, second])); h.api.compareDocumentRevisions.mockImplementation(originalCompare);
  fireEvent.click(restart()); await screen.findByText('同じコンテンツ版のため本文比較なし'); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2);
});

// Review I2: a refusal applies to this document's history and every previously cached pair.
test.each([['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['REVISION_NOT_FOUND', 404]])('review I2: 比較%s後の旧pair選択/タブ/画面往復は旧結果を復活させない', async (code, status) => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`, 1);
  h.api.listDocumentRevisions.mockResolvedValue(page([first, second, tail])); await screen.findByText('同じコンテンツ版のため本文比較なし');
  const original = h.api.compareDocumentRevisions.getMockImplementation()!;
  h.api.compareDocumentRevisions.mockRejectedValue(problem(code, Number(status)));
  fireEvent.change(screen.getByRole('combobox', { name: '基準改訂' }), { target: { value: tail.revisionId } }); await screen.findByRole('alert');
  const compareCount = h.api.compareDocumentRevisions.mock.calls.length;
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'published', tab: 'compare', baseRevisionId: second.revisionId, targetRevisionId: first.revisionId } }); });
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(compareCount);
  fireEvent.click(screen.getByRole('button', { name: '← 版・改訂へ戻る' })); await screen.findByRole('heading', { name: '正式改訂' });
  expect(within(timeline()).queryAllByRole('listitem')).toHaveLength(0); expect(screen.queryByText(/正式改訂はありません/)).not.toBeInTheDocument();
  await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面'); await act(async () => h.history.back());
  await screen.findByRole('heading', { name: '正式改訂' }); expect(within(timeline()).queryAllByRole('listitem')).toHaveLength(0); await compare();
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(1);
  h.api.compareDocumentRevisions.mockImplementation(original); fireEvent.click(restart()); await screen.findByText('同じコンテンツ版のため本文比較なし');
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(compareCount + 1);
  expect(h.router.state.location.search.baseRevisionId).toBe(second.revisionId);
});

test('review I2: 拒否後は遅延した他pairを取消/破棄し固定要求とblobを保持する', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`); h.api.listDocumentRevisions.mockResolvedValue(page([first, second, tail]));
  await screen.findByText('同じコンテンツ版のため本文比較なし'); const delayed = deferred<RevisionComparisonResponse>(); const original = h.api.compareDocumentRevisions.getMockImplementation()!;
  const blob = new Blob(['immutable']); const fixed = fixedWorking(blob); const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed', expectedDocumentRevision: 7, set: {}, unset: [], reason: '保持理由' } };
  act(() => { h.client.setQueryData(workingOperationKey(documentId), fixed); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'keep' }); h.client.setQueryData(['document-revisions', otherId, 'pages'], { pages: [page([revision(90)])], pageParams: [undefined] }); });
  h.api.compareDocumentRevisions.mockImplementation((_id, pair) => pair.baseRevisionId === second.revisionId ? delayed.promise : Promise.reject(problem('FORBIDDEN', 403)));
  const refetch = h.client.invalidateQueries({ queryKey: ['revision-comparison', documentId, second.revisionId, first.revisionId] });
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2));
  fireEvent.change(screen.getByRole('combobox', { name: '基準改訂' }), { target: { value: tail.revisionId } });
  await waitFor(() => expect(screen.queryByRole('combobox', { name: '基準改訂' })).not.toBeInTheDocument());
  await act(async () => { delayed.resolve(await original(documentId, { baseRevisionId: second.revisionId, targetRevisionId: first.revisionId })); await refetch; });
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'published', tab: 'compare', baseRevisionId: second.revisionId, targetRevisionId: first.revisionId } }); });
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(3);
  expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixed); expect((h.client.getQueryData(workingOperationKey(documentId)) as { intent: WorkingWriteIntent }).intent.prepared.body).toBe(blob);
  expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'keep' });
  expect(h.client.getQueryData(['document-revisions', otherId, 'pages'])).toEqual({ pages: [page([revision(90)])], pageParams: [undefined] });
});

test('review: retryable503は既取得行を保ち同じcursorの明示再試行で回復する', async () => {
  const h = setup(); let attempts = 0; h.api.listDocumentRevisions.mockImplementation((_id, cursor) => cursor ? (++attempts === 1 ? Promise.reject({ ...problem('DEPENDENCY_UNAVAILABLE', 503), retryable: true }) : Promise.resolve(page([tail]))) : Promise.resolve(page([first, second], 'same')));
  await ready(); fireEvent.click(more()); await screen.findByRole('alert'); expect(within(timeline()).getAllByRole('listitem')).toHaveLength(2);
  fireEvent.click(screen.getByRole('button', { name: '正式改訂の続きを再試行' })); await within(timeline()).findByText(tail.label);
  expect(h.api.listDocumentRevisions.mock.calls.filter(([, cursor]) => cursor === 'same')).toHaveLength(2);
});

test('review: 同pair先頭再読取の比較403でも旧結果を戻さず先頭readへ止める', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`);
  await screen.findByText('同じコンテンツ版のため本文比較なし'); h.api.compareDocumentRevisions.mockRejectedValue(problem('FORBIDDEN', 403)); fireEvent.click(restart());
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2)); await screen.findByRole('alert');
  expect(screen.queryByText('同じコンテンツ版のため本文比較なし')).not.toBeInTheDocument(); expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2); expect(restart()).toBeEnabled();
});

test('review: 取消済み旧比較の遅延403は先頭再読取後の新成功を失効させない', async () => {
  const h = setup(`tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`); await screen.findByText('同じコンテンツ版のため本文比較なし');
  const delayed = deferred<RevisionComparisonResponse>(); h.api.compareDocumentRevisions.mockReturnValueOnce(delayed.promise);
  const oldRead = h.client.invalidateQueries({ queryKey: ['revision-comparison', documentId, second.revisionId, first.revisionId] });
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2)); fireEvent.click(restart());
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(3)); await screen.findByText('同じコンテンツ版のため本文比較なし');
  await act(async () => { delayed.reject(problem('FORBIDDEN', 403)); await delayed.promise.catch(() => undefined); await oldRead; });
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).not.toHaveProperty('denial');
  expect(screen.getByText('同じコンテンツ版のため本文比較なし')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(second.revisionId); expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2);
});

test('review: 取消済み旧Documentの遅延403は既存read reset後の新履歴/比較を失効させない', async () => {
  const h = setup('tab=compare'); await screen.findByText('同じコンテンツ版のため本文比較なし');
  const delayed = deferred<DocumentDetail>(); h.api.getDocument.mockReturnValueOnce(delayed.promise);
  const oldRead = h.client.invalidateQueries({ queryKey: ['document', documentId] }); await waitFor(() => expect(h.api.getDocument).toHaveBeenCalledTimes(2));
  h.api.listDocumentRevisions.mockResolvedValue(page([first, tail])); await act(async () => { await refreshFolderMoveReads(h.client); });
  await waitFor(() => expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(tail.revisionId)); await screen.findByText('同じコンテンツ版のため本文比較なし');
  await act(async () => { delayed.reject(problem('FORBIDDEN', 403)); await delayed.promise.catch(() => undefined); await oldRead; });
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).not.toHaveProperty('denial');
  expect(screen.getByText('同じコンテンツ版のため本文比較なし')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument(); expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(tail.revisionId);
});
