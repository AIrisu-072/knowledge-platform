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
const more = () => screen.getByRole('button', { name: '比較結果をさらに表示' });
const restart = () => screen.getByRole('button', { name: '比較結果を最初から読み直す' });
const result = () => within(screen.getByRole('region', { name: '新旧比較' }));
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
function setup(search = `tab=compare&baseRevisionId=${second.revisionId}&targetRevisionId=${first.revisionId}`, retry: boolean | number = false) {
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
const fixedPair = { baseRevisionId: second.revisionId, targetRevisionId: first.revisionId, projection: 'display', pageSize: 50 };
const prefix = ['revision-comparison', documentId, second.revisionId, first.revisionId];
function item(index: number): RevisionComparisonResponse['displayItems'][number] {
  return { changeIndex: index, facet: 'body', operation: 'modified', relocation: null, baseLocator: null, targetLocator: null,
    base: { kind: 'text', text: `旧本文${index}`, truncated: false, locator: { kind: 'textSpan', line: index + 1, byteStart: 0, byteEnd: 10 } }, target: { kind: 'text', text: `新本文${index}`, truncated: false, locator: { kind: 'textSpan', line: index + 1, byteStart: 0, byteEnd: 10 } } };
}
function comparison(indices: number[] = [0], nextCursor: string | null = 'next'): RevisionComparisonResponse {
  return { projection: 'display', baseRevision: second, targetRevision: first,
    contentComparisonStatus: 'differentAuthoritativeVersions', verdict: 'unknown', coverage: 'partial', resultDigest: 'digest',
    metadataComparisonStatus: 'different', metadataChanges: [{ path: 'title', baseValue: '旧題名', targetValue: '新題名' }],
    displayItems: indices.map(item), unverifiedRegions: [], nextCursor, changes: [], rows: [], ancillaryChanges: [],
    baseMetadataSnapshotDigest: 'base', targetMetadataSnapshotDigest: 'target', auditEventId: 'audit-first', pageSize: 50 };
}
async function loaded() { await screen.findByText('新本文0'); }
function source(h: ReturnType<typeof setup>, firstPage = comparison(), nextPage = comparison([1], null)) {
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => Promise.resolve(body.cursor ? nextPage : firstPage));
}

// Without the real continuation UI the fifty-first fragment cannot be disclosed.
test.each([null, undefined])('50+1を固定display/50/pairで追加し、監査IDの変化と終端%sを受け入れる', async terminal => {
  const h = setup(); const cursor = 'opaque+/=?日本語'; const last = comparison([50], null);
  if (terminal === undefined) delete (last as Partial<RevisionComparisonResponse>).nextCursor;
  source(h, comparison(Array.from({ length: 50 }, (_, i) => i), cursor), { ...last, auditEventId: 'audit-next', contentAuditEventId: 'content-next', displayAuditEventId: 'display-next', displayResultAuditEventId: 'result-next' });
  await loaded(); expect(result().getAllByText(/^新本文\d+$/)).toHaveLength(50); fireEvent.click(more());
  await screen.findByText('新本文50'); expect(result().getAllByText(/^新本文\d+$/)).toHaveLength(51);
  expect(h.api.compareDocumentRevisions.mock.calls).toEqual([[documentId, fixedPair], [documentId, { ...fixedPair, cursor }]]);
  expect(screen.queryByRole('button', { name: '比較結果をさらに表示' })).not.toBeInTheDocument();
  expect(result().getAllByRole('heading', { name: 'メタデータの変更' })).toHaveLength(1);
  expect(result().getAllByText('旧題名 → 新題名')).toHaveLength(1);
  expect(result().getAllByText('比較範囲', { selector: 'dt' })).toHaveLength(1);
  expect(h.router.state.location.search).toMatchObject({ baseRevisionId: second.revisionId, targetRevisionId: first.revisionId });
});

test('空displayと50未満でもcursorがあれば続け、未比較範囲だけの各ページも順序通り追加する', async () => {
  const h = setup(); const region = (hint: string) => ({ reason: 'unsupportedSemanticConstruct' as const, base: null, target: null, navigationHint: hint });
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => Promise.resolve({ ...comparison([], body.cursor === 'last' ? null : body.cursor ? 'last' : 'next'), unverifiedRegions: body.cursor === 'last' ? [region('末尾範囲')] : body.cursor ? [region('中間範囲')] : [] }));
  await screen.findByRole('heading', { name: '本文の変更' });
  expect(screen.getByText('このページには表示できる差分がありません。続きの比較結果を確認してください。')).toBeVisible();
  fireEvent.click(more()); await screen.findByText('中間範囲'); fireEvent.click(more()); await screen.findByText('末尾範囲');
  expect(screen.getByRole('heading', { name: '未比較範囲 · 取得済み2件' })).toBeVisible();
  expect(within(screen.getByRole('region', { name: '未比較範囲 · 取得済み2件' })).getAllByRole('listitem').map(li => li.textContent)).toEqual([expect.stringContaining('中間範囲'), expect.stringContaining('末尾範囲')]);
  expect(h.api.compareDocumentRevisions.mock.calls.map(([, body]) => body.cursor)).toEqual([undefined, 'next', 'last']);
  expect(screen.queryByRole('button', { name: '比較結果をさらに表示' })).not.toBeInTheDocument();
});

test('同batchの連打は一要求で追加中も既取得内容を保持する', async () => {
  const h = setup(); const pending = deferred<RevisionComparisonResponse>();
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve(comparison()));
  await loaded(); const button = more(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2); await screen.findByText('比較結果の続きを取得中…');
  expect(more()).toBeDisabled(); expect(screen.getByText('新本文0')).toBeVisible(); await act(async () => pending.resolve(comparison([1], null))); await screen.findByText('新本文1');
});

test.each([new TypeError('offline'), { ...problem('DEPENDENCY_UNAVAILABLE', 503), retryable: true }])('追加の通信/再試行可能失敗は内容を保持し同じcursorを明示再試行する', async failure => {
  const h = setup(undefined, 1); let attempts = 0;
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? (++attempts === 1 ? Promise.reject(failure) : Promise.resolve(comparison([1], null))) : Promise.resolve(comparison()));
  await loaded(); fireEvent.click(more()); await screen.findByRole('alert'); expect(screen.getByText('新本文0')).toBeVisible(); expect(attempts).toBe(1);
  fireEvent.click(screen.getByRole('button', { name: '比較結果の続きを再試行' })); await screen.findByText('新本文1');
  expect(h.api.compareDocumentRevisions.mock.calls.map(([, body]) => body.cursor)).toEqual([undefined, 'next', 'next']);
});

test.each([['CURSOR_STALE', 409], ['STALE_COMPARISON_INPUT', 409], ['VALIDATION_FAILED', 422], ['INTERNAL', 500]])('比較%sは旧内容とcursorを隠し明示restartまで自動再送しない', async (code, status) => {
  const h = setup(undefined, 1); source(h); await loaded(); h.api.compareDocumentRevisions.mockRejectedValue(problem(code, Number(status)));
  fireEvent.click(more()); await screen.findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '比較結果をさらに表示' })).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2);
  await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面');
  await act(async () => h.history.back()); await screen.findByRole('heading', { name: '正式改訂の比較' }); expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2);
  source(h, comparison([9], null)); fireEvent.click(restart()); await screen.findByText('新本文9'); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
});

const mismatches: Array<[string, (page: RevisionComparisonResponse) => void]> = [
  ['base revision', p => { p.baseRevision = { ...p.baseRevision, revisionId: tail.revisionId }; }],
  ['target version', p => { p.targetRevision = { ...p.targetRevision, documentVersionId: 'changed' }; }],
  ['revision timestamp', p => { p.baseRevision = { ...p.baseRevision, createdAt: '2026-10-02T00:00:00Z' }; }],
  ['projection', p => { p.projection = 'diff'; }], ['page size', p => { p.pageSize = 100; }],
  ['content status', p => { p.contentComparisonStatus = 'sameAuthoritativeVersion'; }], ['verdict', p => { p.verdict = 'same'; }],
  ['coverage', p => { p.coverage = 'full'; }], ['content digest', p => { p.resultDigest = 'changed'; }],
  ['metadata status', p => { p.metadataComparisonStatus = 'same'; }], ['base metadata digest', p => { p.baseMetadataSnapshotDigest = 'changed'; }],
  ['target metadata digest', p => { p.targetMetadataSnapshotDigest = 'changed'; }], ['metadata changes', p => { p.metadataChanges = []; }],
];
test.each(mismatches)('追加の%s不一致は旧header/本文と新ページを結合せずrestartへ止める', async (_name, change) => {
  const h = setup(); const next = comparison([1], null); change(next); source(h, comparison(), next); await loaded(); fireEvent.click(more());
  await screen.findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
  expect(screen.queryByText('旧題名 → 新題名')).not.toBeInTheDocument(); expect(restart()).toBeEnabled();
  expect(screen.queryByRole('button', { name: '比較結果の続きを再試行' })).not.toBeInTheDocument();
});

test('初回応答のpair不一致も表示しない', async () => {
  const h = setup(); source(h, { ...comparison(), targetRevision: tail }); await screen.findByRole('alert');
  expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(restart()).toBeEnabled();
});

test('同pairのrestartは全ページを捨て初回loadingに戻し、同batch連打をまとめる', async () => {
  const h = setup(); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('新本文1');
  const pending = deferred<RevisionComparisonResponse>(); h.api.compareDocumentRevisions.mockReturnValue(pending.promise);
  const button = restart(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  await screen.findByText('比較結果を取得中…'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
  expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(3); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
  await act(async () => pending.resolve(comparison([9], null))); await screen.findByText('新本文9');
  expect(h.router.state.location.search).toMatchObject({ baseRevisionId: second.revisionId, targetRevisionId: first.revisionId });
});

test('旧単ページcacheを混在させずprefix invalidate中の新header/旧tailを隠しfresh cursorだけで再構成する', async () => {
  const h = setup(); source(h); act(() => { h.client.setQueryData(prefix, comparison([99], null)); });
  await loaded(); expect(screen.queryByText('新本文99')).not.toBeInTheDocument(); fireEvent.click(more()); await screen.findByText('新本文1');
  const pending = deferred<RevisionComparisonResponse>(); const fresh = { ...comparison([9], 'fresh'), resultDigest: 'fresh-digest' };
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor === 'fresh' ? pending.promise : Promise.resolve(fresh));
  const refreshing = h.client.invalidateQueries({ queryKey: prefix }); await screen.findByText('比較結果を読み直し中…');
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, { ...fixedPair, cursor: 'fresh' }));
  for (const n of [0, 1, 9]) expect(screen.queryByText(`新本文${n}`)).not.toBeInTheDocument();
  await act(async () => { pending.resolve({ ...comparison([10], null), resultDigest: 'fresh-digest' }); await refreshing; });
  await screen.findByText('新本文10'); expect(screen.getByText('新本文9')).toBeVisible(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
});

test.each(['pair', 'document'])('%s変更後の遅延成功と戻りは旧ページ/cursorを復活させない', async target => {
  const h = setup(); const pending = deferred<RevisionComparisonResponse>(); h.api.listDocumentRevisions.mockResolvedValue(page([first, second, tail]));
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve({ ...comparison(), baseRevision: body.baseRevisionId === tail.revisionId ? tail : second }));
  await loaded(); fireEvent.click(more());
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: target === 'document' ? otherId : documentId }, search: { tab: 'compare', view: 'published', baseRevisionId: target === 'pair' ? tail.revisionId : second.revisionId, targetRevisionId: first.revisionId } }); });
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(3));
  await act(async () => pending.resolve(comparison([1], null))); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
  source(h, comparison([9], null)); await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'published', tab: 'compare', baseRevisionId: second.revisionId, targetRevisionId: first.revisionId } }); });
  await screen.findByText('新本文9'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
});

test.each([['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['REVISION_NOT_FOUND', 404]])('追加%sは文書全pairを失効し正式改訂の明示readまで復活させない', async (code, status) => {
  const h = setup(undefined, 1); source(h); await loaded(); h.client.setQueryData(['revision-comparison', documentId, tail.revisionId, first.revisionId], comparison([99], null));
  h.api.compareDocumentRevisions.mockRejectedValue(problem(code, Number(status))); fireEvent.click(more()); await screen.findByRole('alert');
  expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(h.client.getQueryData(['revision-comparison', documentId, tail.revisionId, first.revisionId])).toBeUndefined();
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).toHaveProperty('denial.code', code);
  expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(2); expect(screen.queryByRole('combobox', { name: '基準改訂' })).not.toBeInTheDocument();
  source(h, comparison([9], null)); fireEvent.click(screen.getByRole('button', { name: '正式改訂を最初から読み直す' }));
  await screen.findByText('新本文9'); expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(2); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
});

test.each(['success', 'denial'])('正式改訂restart後の中断済み追加%sは新結果へ干渉しない', async outcome => {
  const h = setup(); const pending = deferred<RevisionComparisonResponse>(); h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve(comparison()));
  await loaded(); fireEvent.click(more()); source(h, comparison([9], null)); fireEvent.click(screen.getByRole('button', { name: '正式改訂を最初から読み直す' }));
  await screen.findByText('新本文9'); await act(async () => { if (outcome === 'success') pending.resolve(comparison([1], null)); else pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('新本文9')).toBeVisible(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).not.toHaveProperty('denial');
});

test.each(['lifecycle', 'move'])('既存%s read更新は旧ページを捨て未確定操作/blob/providerを保持する', async kind => {
  const h = setup(); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('新本文1');
  const blob = new Blob(['immutable']); const fixed = fixedWorking(blob); const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed', expectedDocumentRevision: 7, set: {}, unset: [], reason: '保持理由' } };
  act(() => { h.client.setQueryData(workingOperationKey(documentId), fixed); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'keep' }); });
  source(h, comparison([9], null)); await act(async () => { if (kind === 'lifecycle') await refreshLifecycleQueries(h.client, documentId); else await refreshFolderMoveReads(h.client); });
  await screen.findByText('新本文9'); expect(screen.queryByText('新本文1')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
  expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixed); expect((h.client.getQueryData(workingOperationKey(documentId)) as { intent: WorkingWriteIntent }).intent.prepared.body).toBe(blob);
  expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'keep' });
});

test('再試行可能な追加失敗の後も既存invalidateは旧cursorを捨てfresh先頭を読む', async () => {
  const h = setup(); h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? Promise.reject(new TypeError('offline')) : Promise.resolve(comparison()));
  await loaded(); fireEvent.click(more()); await screen.findByRole('alert'); expect(screen.getByText('新本文0')).toBeVisible();
  source(h, comparison([9], null)); await act(async () => { await h.client.invalidateQueries({ queryKey: ['revision-comparison', documentId] }); });
  await screen.findByText('新本文9'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
});

test.each(['pair', 'document'])('%s変更で取消済みの追加403は新pairの比較/改訂readを拒否しない', async target => {
  const h = setup(); const pending = deferred<RevisionComparisonResponse>(); h.api.listDocumentRevisions.mockResolvedValue(page([first, second, tail]));
  h.api.compareDocumentRevisions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve({ ...comparison([0], 'next'), baseRevision: body.baseRevisionId === tail.revisionId ? tail : second }));
  await loaded(); fireEvent.click(more());
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: target === 'document' ? otherId : documentId }, search: { tab: 'compare', view: 'published', baseRevisionId: target === 'pair' ? tail.revisionId : second.revisionId, targetRevisionId: first.revisionId } }); });
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(3)); await loaded();
  await act(async () => { pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(screen.getByText('新本文0')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(h.client.getQueryData(['document-revisions', documentId, 'pages'])).not.toHaveProperty('denial');
  expect(screen.getByRole('combobox', { name: '基準改訂' })).toHaveValue(target === 'pair' ? tail.revisionId : second.revisionId);
});

test.each(['base', 'target', 'projection', 'pageSize'])('初回%sの固定request不一致はrestartまで表示しない', async field => {
  const h = setup(); const page = comparison();
  if (field === 'base') page.baseRevision = tail;
  if (field === 'target') page.targetRevision = tail;
  if (field === 'projection') page.projection = 'comparisonTable';
  if (field === 'pageSize') page.pageSize = null;
  source(h, page); await screen.findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(restart()).toBeEnabled();
});

test('別画面を挟む文書往復でも旧ページ/cursorを再利用しない', async () => {
  const h = setup(); source(h); await loaded(); fireEvent.click(more()); await screen.findByText('新本文1');
  await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面');
  source(h, comparison([8], null)); await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { tab: 'compare', view: 'published', baseRevisionId: second.revisionId, targetRevisionId: first.revisionId } }); }); await screen.findByText('新本文8');
  await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await screen.findByText('別画面');
  source(h, comparison([9], null)); await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { tab: 'compare', view: 'published', baseRevisionId: second.revisionId, targetRevisionId: first.revisionId } }); });
  await screen.findByText('新本文9'); expect(screen.queryByText('新本文1')).not.toBeInTheDocument(); expect(h.api.compareDocumentRevisions).toHaveBeenLastCalledWith(documentId, fixedPair);
});
