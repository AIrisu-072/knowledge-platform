import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentRevisionPage, type DocumentRevisionSummary, type DocumentDetail, type RevisionComparisonResponse, type History } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';
import { workingOperationKey, type WorkingOperation, type WorkingWriteIntent } from '../src/application/document-working-version';
import { metadataOperations, type MetadataOperation } from '../src/application/document-metadata';
import { refreshLifecycleQueries } from '../src/application/document-lifecycle-operations';
import { refreshWorkingQueries } from '../src/application/document-working-version';
import { refreshScheduleQueries } from '../src/application/document-schedule-cancel';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getDocumentHistory: jest.fn(), getRootFolder: jest.fn(), getDocument: jest.fn(), listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(),
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
const more = () => screen.getByRole('button', { name: '変更履歴をさらに表示' });
const restart = () => screen.getByRole('button', { name: '変更履歴を最初から読み直す' });
const result = () => within(screen.getByRole('region', { name: '変更履歴' }));
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
function setup(search = 'tab=history', retry: boolean | number = false) {
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
const prefix = ['document-history', documentId];
function entry(index: number): History['items'][number] {
  return { sourceKind: 'operation', sourceKey: `key${index}`, actionCode: `操作${index}`, occurredAt: '2026-10-01T00:00:00Z', details: {}, provenanceQuality: 'operationLedger' };
}
function historyPage(indices = [0], nextCursor: string | null = 'next'): History { return { items: indices.map(entry), nextCursor }; }
async function loaded() { await screen.findByText('操作0'); }
function source(h: ReturnType<typeof setup>, firstPage = historyPage(), nextPage = historyPage([1], null)) {
  h.api.getDocumentHistory.mockImplementation((_id, cursor) => Promise.resolve(cursor === undefined ? firstPage : nextPage));
}
async function leave(h: ReturnType<typeof setup>, target: string) {
  if (target === 'route') { await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面'); }
  else { await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: target === 'document' ? otherId : documentId }, search: { view: 'published', tab: 'versions' } }); }); await screen.findByRole('heading', { name: '正式改訂' }); }
}
async function back(h: ReturnType<typeof setup>) { await act(async () => h.history.back()); await screen.findByRole('heading', { name: '変更履歴' }); }

// Event history keeps the detail page's JST contract even in a UTC browser.
test('通常詳細の履歴日時は端末のtimezoneによらずJSTの日付境界を保つ', async () => {
  const h = setup(); source(h, { items: [{ ...entry(0), occurredAt: '2026-10-01T23:30:00Z' }], nextCursor: null });
  await loaded();
  expect(result().getByRole('listitem').querySelector('time')).toHaveTextContent('2026/10/02 8:30 (Asia/Tokyo, UTC+09:00)');
});

// Removing continuation or ignoring nextCursor loses the 101st real route row.
test('100+1をopaque cursorで追加しnullだけで終端とする', async () => {
  const h = setup(); const cursor = 'opaque+/=?日本語'; source(h, historyPage(Array.from({ length: 100 }, (_, i) => i), cursor), historyPage([100], null));
  await loaded(); expect(result().getAllByRole('listitem')).toHaveLength(100); fireEvent.click(more()); await screen.findByText('操作100');
  expect(result().getAllByRole('listitem')).toHaveLength(101); expect(h.api.getDocumentHistory.mock.calls).toEqual([[documentId, undefined], [documentId, cursor]]);
  expect(screen.queryByRole('button', { name: '変更履歴をさらに表示' })).not.toBeInTheDocument(); expect(restart()).toBeEnabled();
});

test('短いページと空ページにもcursorがあれば続ける', async () => {
  const h = setup(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => Promise.resolve(cursor === undefined ? historyPage([], 'short') : cursor === 'short' ? historyPage([0], 'empty') : cursor === 'empty' ? historyPage([], 'last') : historyPage([1], null)));
  await screen.findByRole('heading', { name: '変更履歴' }); await waitFor(() => expect(more()).toBeEnabled());
  expect(screen.queryByText('表示できる履歴はありません。')).not.toBeInTheDocument(); fireEvent.click(more()); await loaded(); fireEvent.click(more());
  await waitFor(() => expect(h.api.getDocumentHistory).toHaveBeenCalledWith(documentId, 'empty')); await waitFor(() => expect(more()).toBeEnabled()); fireEvent.click(more()); await screen.findByText('操作1');
  expect(h.api.getDocumentHistory.mock.calls.map(([, cursor]) => cursor)).toEqual([undefined, 'short', 'empty', 'last']);
});

test('重複はsourceKind/sourceKeyの組で最初の記録を保持し、同keyの別種は残す', async () => {
  const h = setup(); source(h, historyPage(), { items: [{ ...entry(0), actionCode: '重複は破棄' }, { ...entry(1), sourceKey: 'key0', sourceKind: 'version' }, entry(2)], nextCursor: null });
  await loaded(); fireEvent.click(more()); await screen.findByText('操作2'); expect(result().getAllByRole('listitem').map(li => li.querySelector('strong')?.textContent)).toEqual(['操作0', '操作1', '操作2']);
  expect(screen.queryByText('重複は破棄')).not.toBeInTheDocument();
});

test('未知の日時/実行者と3由来を保ち、表示名欠如を人物や日時で補完しない', async () => {
  const h = setup(); source(h, { items: [{ ...entry(0), occurredAt: null }, { ...entry(1), provenanceQuality: 'versionFallback', actor: { identityProvider: 'synthetic', principalId: '不明主体', presentation: { ref: { provider: 'synthetic', kind: 'principal', subjectId: 'synthetic' }, secondaryText: null, displayName: null, resolution: 'notFound' } } }, { ...entry(2), provenanceQuality: 'legacyUnknown', actor: { identityProvider: 'synthetic', principalId: '表示不能主体', presentation: { ref: { provider: 'synthetic', kind: 'principal', subjectId: 'synthetic' }, secondaryText: null, displayName: null, resolution: 'unavailable' } } }], nextCursor: null });
  await loaded(); for (const label of ['操作記録', '版からの履歴', '由来不明の履歴', '日時不明', '実行者不明', '不明主体 · ディレクトリに存在しません', '表示不能主体 · 表示情報を取得できません']) expect(result().getByText(label)).toBeVisible();
});

test('同batchの連打は一要求で、追加中は旧行を保持してbuttonを無効化する', async () => {
  const h = setup(); const pending = deferred<History>(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? pending.promise : Promise.resolve(historyPage()));
  await loaded(); const button = more(); act(() => { fireEvent.click(button); fireEvent.click(button); }); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(2);
  await screen.findByText('変更履歴の続きを取得中…'); expect(more()).toBeDisabled(); expect(screen.getByText('操作0')).toBeVisible(); await act(async () => pending.resolve(historyPage([1], null))); await screen.findByText('操作1');
});

test.each([new TypeError('offline'), { ...problem('DEPENDENCY_UNAVAILABLE', 503), retryable: true }])('追加通信/再試行可能失敗は同じcursorのみ明示再試行する', async error => {
  const h = setup(undefined, 1); let attempts = 0; h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? (++attempts === 1 ? Promise.reject(error) : Promise.resolve(historyPage([1], null))) : Promise.resolve(historyPage()));
  await loaded(); fireEvent.click(more()); await screen.findByRole('alert'); expect(screen.getByText('操作0')).toBeVisible(); expect(attempts).toBe(1);
  fireEvent.click(screen.getByRole('button', { name: '変更履歴の続きを再試行' })); await screen.findByText('操作1'); expect(h.api.getDocumentHistory.mock.calls.map(([, cursor]) => cursor)).toEqual([undefined, 'next', 'next']);
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_NOT_FOUND', 404], ['INTERNAL', 500]])('追加%sは旧行を隠しinvalidate/reset/再訪でも明示restartまで停止する', async (code, status) => {
  const h = setup(undefined, 1); source(h); await loaded(); h.api.getDocumentHistory.mockRejectedValue(problem(code, Number(status))); fireEvent.click(more()); await screen.findByRole('alert');
  expect(screen.queryByText('操作0')).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: '変更履歴をさらに表示' })).not.toBeInTheDocument();
  source(h, historyPage([9], null)); await act(async () => { await h.client.invalidateQueries({ queryKey: prefix }); await refreshFolderMoveReads(h.client); });
  expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(2); expect(screen.queryByText('操作9')).not.toBeInTheDocument();
  await leave(h, 'route'); await back(h); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(2); await screen.findByRole('alert');
  fireEvent.click(restart()); await screen.findByText('操作9'); expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, undefined);
});

test.each([new TypeError('offline'), problem('FORBIDDEN', 403)])('初回失敗を空履歴にせず明示先頭readで回復する', async error => {
  const h = setup(undefined, 1); h.api.getDocumentHistory.mockRejectedValue(error); await screen.findByRole('alert');
  expect(screen.queryByText('表示できる履歴はありません。')).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(1);
  source(h, historyPage([], null)); fireEvent.click(restart()); await screen.findByText('表示できる履歴はありません。'); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('履歴のみ403は普通の文書と正式改訂を拒否扱いにしない', async () => {
  const h = setup(); h.api.getDocumentHistory.mockRejectedValue(problem('FORBIDDEN', 403)); await screen.findByRole('alert');
  expect(screen.getByRole('heading', { name: '合成文書' })).toBeVisible(); await leave(h, 'tab'); expect(screen.getByRole('heading', { name: '正式改訂' })).toBeVisible();
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).not.toHaveProperty('denial'); await back(h); await screen.findByRole('alert'); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(1);
});

test('文書detailの現在403を自動retry200が上書きしても旧履歴は明示restartまで復活しない', async () => {
  const h = setup(undefined, 1); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('操作1');
  h.api.getDocument.mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockResolvedValue(detail()); source(h, historyPage([9], null));
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); }); await screen.findByRole('alert');
  expect(screen.getByRole('heading', { name: '合成文書' })).toBeVisible(); for (const n of [0, 1, 9]) expect(screen.queryByText(`操作${n}`)).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(2);
  fireEvent.click(restart()); await screen.findByText('操作9'); expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, undefined);
});

test.each(['tab', 'route', 'document'])('成功ページとcursorを%s往復後に再利用しない', async target => {
  const h = setup(); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('操作1'); await leave(h, target);
  source(h, historyPage([9], null)); await back(h); await screen.findByText('操作9'); expect(screen.queryByText('操作1')).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, undefined);
});

test.each(['success', 'denial'])('取消後の遅延追加%sは別文書と再訪した新readへ干渉しない', async outcome => {
  const h = setup(); const pending = deferred<History>(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? pending.promise : Promise.resolve(historyPage()));
  await loaded(); fireEvent.click(more()); await leave(h, 'document'); source(h, historyPage([9], null)); await back(h); await screen.findByText('操作9');
  await act(async () => { if (outcome === 'success') pending.resolve(historyPage([1], null)); else pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('操作9')).toBeVisible(); expect(screen.queryByText('操作1')).not.toBeInTheDocument(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test.each(['success', 'denial'])('move resetで取消した遅延追加%sは新先頭へ干渉しない', async outcome => {
  const h = setup(); const pending = deferred<History>(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? pending.promise : Promise.resolve(historyPage()));
  await loaded(); fireEvent.click(more()); source(h, historyPage([9], null)); await act(async () => { await refreshFolderMoveReads(h.client); }); await screen.findByText('操作9');
  await act(async () => { if (outcome === 'success') pending.resolve(historyPage([1], null)); else pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('操作9')).toBeVisible(); expect(screen.queryByText('操作1')).not.toBeInTheDocument(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('旧単ページcacheを混ぜずinvalidateはfresh cursorで原子的に組み直す', async () => {
  const h = setup(); source(h); act(() => { h.client.setQueryData(prefix, historyPage([99], null)); }); await loaded(); expect(screen.queryByText('操作99')).not.toBeInTheDocument(); fireEvent.click(more()); await screen.findByText('操作1');
  const pending = deferred<History>(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor === 'fresh' ? pending.promise : Promise.resolve(historyPage([9], 'fresh')));
  let refreshing!: Promise<void>; act(() => { refreshing = h.client.invalidateQueries({ queryKey: prefix }); }); await screen.findByText('変更履歴を読み直し中…'); await waitFor(() => expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, 'fresh'));
  for (const n of [0, 1, 9]) expect(screen.queryByText(`操作${n}`)).not.toBeInTheDocument(); await act(async () => { pending.resolve(historyPage([10], null)); await refreshing; });
  await screen.findByText('操作10'); expect(screen.getByText('操作9')).toBeVisible(); expect(screen.queryByText('操作1')).not.toBeInTheDocument();
});

test('再試行可能な追加失敗後のinvalidateも旧cursorを捨てfresh先頭を読む', async () => {
  const h = setup(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? Promise.reject(new TypeError('offline')) : Promise.resolve(historyPage()));
  await loaded(); fireEvent.click(more()); await screen.findByRole('alert'); source(h, historyPage([9], null)); await act(async () => { await h.client.invalidateQueries({ queryKey: prefix }); }); await screen.findByText('操作9');
  expect(screen.queryByText('操作0')).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, undefined);
});

test.each(['metadata', 'lifecycle', 'working', 'cancel', 'move', 'restart'])('既存%s更新はcursorを捨て未確定操作/blob/OrganizationとURLを保持する', async kind => {
  const returnTo = '/documents?title=保持'; const h = setup(`tab=history&versionId=${versionId}&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}&returnTo=${encodeURIComponent(returnTo)}`); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('操作1');
  const blob = new Blob(['immutable']); const fixed = fixedWorking(blob); const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed', expectedDocumentRevision: 7, set: {}, unset: [], reason: '保持理由' } };
  act(() => { h.client.setQueryData(workingOperationKey(documentId), fixed); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'keep' }); });
  source(h, historyPage([9], null)); await act(async () => { if (kind === 'metadata') await h.client.invalidateQueries({ queryKey: prefix }); else if (kind === 'lifecycle') await refreshLifecycleQueries(h.client, documentId); else if (kind === 'working') await refreshWorkingQueries(h.client, documentId); else if (kind === 'cancel') await refreshScheduleQueries(h.client, documentId); else if (kind === 'move') await refreshFolderMoveReads(h.client); else fireEvent.click(restart()); });
  await screen.findByText('操作9'); expect(screen.queryByText('操作1')).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenLastCalledWith(documentId, undefined);
  expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixed); expect((h.client.getQueryData(workingOperationKey(documentId)) as { intent: WorkingWriteIntent }).intent.prepared.body).toBe(blob);
  expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'keep' });
  expect(h.router.state.location.search).toMatchObject({ versionId, baseRevisionId: second.revisionId, targetRevisionId: first.revisionId, returnTo });
});

test('先頭restart連打は一要求で全行を消しloadingへ戻る', async () => {
  const h = setup(); source(h); await loaded(); const pending = deferred<History>(); h.api.getDocumentHistory.mockReturnValue(pending.promise); const button = restart(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  await screen.findByText('履歴を読み込み中…'); expect(screen.queryByText('操作0')).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(2); expect(restart()).toBeDisabled();
  await act(async () => pending.resolve(historyPage([9], null))); await screen.findByText('操作9');
});

test.each(['success', 'denial'])('文書detail拒否後に取消した追加%sが届いても明示restartした履歴を変えない', async outcome => {
  const h = setup(undefined, 1); const pending = deferred<History>(); h.api.getDocumentHistory.mockImplementation((_id, cursor) => cursor ? pending.promise : Promise.resolve(historyPage()));
  await loaded(); fireEvent.click(more()); h.api.getDocument.mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockResolvedValue(detail());
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); }); await screen.findByRole('alert'); expect(screen.queryByText('操作0')).not.toBeInTheDocument();
  source(h, historyPage([9], null)); fireEvent.click(restart()); await screen.findByText('操作9');
  await act(async () => { if (outcome === 'success') pending.resolve(historyPage([1], null)); else pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('操作9')).toBeVisible(); expect(screen.queryByText('操作1')).not.toBeInTheDocument(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('別文書移動で取消した旧detail403を再訪時の履歴拒否にしない', async () => {
  const h = setup(); source(h); await loaded(); const pending = deferred<DocumentDetail>(); h.api.getDocument.mockImplementation(id => id === documentId ? pending.promise : Promise.resolve(detail(id)));
  let refreshing!: Promise<void>; act(() => { refreshing = h.client.invalidateQueries({ queryKey: ['document', documentId] }); }); await waitFor(() => expect(h.api.getDocument).toHaveBeenCalledTimes(2));
  await leave(h, 'document'); await act(async () => { pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); await refreshing; });
  h.api.getDocument.mockImplementation(id => Promise.resolve(detail(id))); source(h, historyPage([9], null)); await back(h); await screen.findByText('操作9'); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('取消した先頭readの遅延403は同文書の新しい先頭を拒否しない', async () => {
  const h = setup(); const pending = deferred<History>(); h.api.getDocumentHistory.mockReturnValue(pending.promise);
  await screen.findByText('履歴を読み込み中…'); await leave(h, 'tab'); source(h, historyPage([9], null)); await back(h); await screen.findByText('操作9');
  await act(async () => { pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('操作9')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('source組の区切り文字が同じ文字列になる別tupleも別の行として表示する', async () => {
  const h = setup(); source(h, { items: [{ ...entry(0), sourceKind: 'operation:version', sourceKey: 'key' }], nextCursor: 'next' }, { items: [{ ...entry(1), sourceKind: 'operation', sourceKey: 'version:key' }], nextCursor: null });
  await loaded(); fireEvent.click(more()); await screen.findByText('操作1'); expect(result().getAllByRole('listitem')).toHaveLength(2);
});
