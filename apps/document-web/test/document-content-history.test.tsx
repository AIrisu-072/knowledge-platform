import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentDetail, type Version, type VersionDetail, type VersionList, type FileList } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';
import { workingOperationKey, type WorkingOperation, type WorkingWriteIntent } from '../src/application/document-working-version';
import { metadataOperations, type MetadataOperation } from '../src/application/document-metadata';
import { refreshLifecycleQueries } from '../src/application/document-lifecycle-operations';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getDocumentHistory: jest.fn(), getRootFolder: jest.fn(), getDocument: jest.fn(), listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(),
  listDocumentRevisions: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(),
  publishVersion: jest.fn(), schedulePublication: jest.fn(), withdrawVersion: jest.fn(), updateWorkingVersion: jest.fn(), markDocumentVersionRead: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const available = { status: 'available' as const }; const denied = { status: 'disabled' as const, reason: 'permission' as const };
const version = (index: number): VersionDetail => ({ versionId: `00000000-0000-4000-8000-${String(index).padStart(12, '0')}`, versionNo: index, baseVersionId: null,
  lifecycleState: 'published', isCurrent: index === 102, createdAt: '2026-10-01T00:00:00Z', approvedAt: null, scheduledPublishAt: null, publishedAt: '2026-10-02T00:00:00Z', withdrawnAt: null, updatedAt: '2026-10-02T00:00:00Z',
  fileSummary: { authoritativeItemCount: 2, totalSizeBytes: 12, primary: null }, firstReadAt: '2026-10-03T00:00:00Z', title: `履歴タイトル${index}`, metadata: { category: `版属性${index}` }, currentPublicationScheduleId: null,
  capabilities: { edit: available, rebase: available, publish: available, withdraw: available, schedulePublication: available, cancelPublicationSchedule: available, download: available } });
const current = version(102), previous = version(101), tail = version(100);
const page = (items: Version[], nextCursor: string | null = null): VersionList => ({ items, nextCursor });
const files: FileList = { items: [
  { contentItemId: 'item-preview', representationId: 'rep-preview', logicalPath: 'preview', ordinal: 0, role: 'PREVIEW', displayName: 'プレビュー.pdf', mediaType: 'application/pdf', sizeBytes: 1 },
  { contentItemId: 'item-two', representationId: 'rep-b', logicalPath: 'b', ordinal: 2, role: 'AUTHORITATIVE', displayName: '原本B.pdf', mediaType: 'application/pdf', sizeBytes: 6 },
  { contentItemId: 'item-one', representationId: 'rep-a', logicalPath: 'a', ordinal: 1, role: 'AUTHORITATIVE', displayName: '原本A.pdf', mediaType: 'application/pdf', sizeBytes: 6 },
  { contentItemId: 'item-unknown', representationId: 'rep-unknown', logicalPath: 'unknown', ordinal: 3, role: 'UNKNOWN', displayName: '未知.pdf', mediaType: 'application/pdf', sizeBytes: 1 },
] };
const problem = (code: string, status: number, retryable = false) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function detail(id = documentId): DocumentDetail {
  return { documentId: id, documentVersionId: current.versionId, title: id === documentId ? '合成文書' : '別文書', folderId: null, folderName: null, revision: 7, metadata: { category: '現在の文書属性' }, createdAt: current.createdAt, currentVersionId: current.versionId, unread: false, publishedAt: current.publishedAt!,
    displayVersion: { ...current, lifecycleState: 'PUBLISHED' }, displayRevision: null, readState: { isRead: true, firstReadAt: current.firstReadAt }, displayTimestamp: { kind: 'revisionCreatedAt', value: current.publishedAt! },
    capabilities: { createVersion: denied, updateMetadata: denied, moveDocument: denied, endPublication: denied, manageAccess: denied, compareVersions: denied } };
}
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => { metadataOperations(client).clear(documentId); client.clear(); }));
function setup(search = `tab=versions&versionId=${current.versionId}`, retry: boolean | number = false) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock } & { markDocumentVersionRead: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation(id => Promise.resolve(detail(id)));
  api.listDocumentVersions.mockImplementation((_id, purpose) => Promise.resolve(page(purpose === 'history' ? [current, previous] : [current])));
  api.getDocumentVersion.mockImplementation((_id, id, purpose) => Promise.resolve(purpose === 'history' ? version(Number(id.slice(-12))) : { ...current, capabilities: { ...current.capabilities, edit: denied, withdraw: denied, cancelPublicationSchedule: denied, download: denied } }));
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.listVersionFiles.mockResolvedValue(files); api.downloadVersionFile.mockResolvedValue(new Blob(['synthetic']));
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn().mockReturnValue('blob:synthetic') });
  Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() });
  jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=published&${search}`] });
  const router = createRouter({ routeTree: root.addChildren([route, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry, retryDelay: 0, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, history };
}
const region = () => within(screen.getByRole('region', { name: 'コンテンツ版の履歴（閲覧専用）' }));
const selected = () => within(screen.getByRole('region', { name: '選択したコンテンツ版の詳細' }));
const chooser = () => region().getByRole('combobox', { name: '履歴のコンテンツ版を選択' });
const more = () => region().getByRole('button', { name: 'コンテンツ版の履歴をさらに表示' });
const restart = () => region().getByRole('button', { name: 'コンテンツ版の履歴を最初から読み直す' });
const close = () => region().getByRole('button', { name: 'コンテンツ版の履歴を閉じる' });
const historyCalls = (mock: jest.Mock) => mock.mock.calls.filter(call => call[1] === 'history');
async function open() { fireEvent.click(await screen.findByRole('button', { name: 'コンテンツ版の履歴を開く' })); await waitFor(() => expect(chooser()).toBeInTheDocument()); }
async function select(id = previous.versionId) { fireEvent.change(chooser(), { target: { value: id } }); await selected().findByText(`履歴タイトル${Number(id.slice(-12))}`); }
async function downloadReady() { return region().findByRole('button', { name: '履歴の原本を取得: 原本B.pdf' }); }

// 入口を自動readへ変える、または通常selectedVersionへ代入するとこの分離が破れる。
test('明示入口だけでhistoryを読み、空の選択から旧版詳細へ進んでも通常URLと公開対象を保持する', async () => {
  const h = setup(); await screen.findByRole('heading', { name: '選択中: 版 102' });
  expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(0); await open(); expect(chooser()).toHaveValue('');
  expect(h.api.getDocumentVersion.mock.calls.filter(call => call[2] === 'history')).toHaveLength(0); await select();
  expect(h.api.getDocumentVersion).toHaveBeenCalledWith(documentId, previous.versionId, 'history'); expect(h.api.listVersionFiles).toHaveBeenCalledWith(documentId, previous.versionId, 'history');
  expect(h.router.state.location.search.versionId).toBe(current.versionId); expect(h.router.state.location.search.workflow).toBeUndefined();
  expect(screen.getByRole('heading', { name: '選択中: 版 102' })).toBeVisible(); expect(selected().getByText('版属性101')).toBeVisible();
  expect(selected().queryByText('現在の文書属性')).not.toBeInTheDocument(); expect(selected().getByText('本人の初回既読日時')).toBeVisible();
  expect(region().queryByRole('button', { name: '公開する' })).not.toBeInTheDocument();
  expect(h.api.publishVersion).not.toHaveBeenCalled(); expect(h.api.withdrawVersion).not.toHaveBeenCalled(); expect(h.api.markDocumentVersionRead).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '公開する' })); await screen.findByRole('region', { name: '公開・予約公開' });
  expect(h.router.state.location.search.versionId).toBe(current.versionId); expect(screen.queryByRole('region', { name: 'コンテンツ版の履歴（閲覧専用）' })).not.toBeInTheDocument();
});

test('historyは現行・過去公開・WORKING・WITHDRAWNを返却状態で区別する', async () => {
  const h = setup(); const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose) => purpose === 'history' ? Promise.resolve(page([current, previous, { ...tail, lifecycleState: 'working' }, { ...version(99), lifecycleState: 'withdrawn' }])) : original(id, purpose));
  await open(); const options = within(chooser()).getAllByRole('option').map(option => option.textContent);
  expect(options).toEqual(['コンテンツ版を選択してください', 'Version 102 · 現行版', 'Version 101 · 過去版', 'Version 100 · 下書き', 'Version 99 · 公開終了']);
});

test('全AUTHORITATIVEだけをserver順で示し、返却された4IDとhistoryで原本を取得する', async () => {
  const h = setup(); await open(); await select(); await downloadReady();
  const buttons = region().getAllByRole('button', { name: /^履歴の原本を取得:/ }); expect(buttons.map(button => button.textContent)).toEqual(['履歴の原本を取得: 原本B.pdf', '履歴の原本を取得: 原本A.pdf']);
  for (let index = 0; index < buttons.length; index++) { fireEvent.click(buttons[index]!); await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalledTimes(index + 1)); }
  expect(h.api.downloadVersionFile).toHaveBeenNthCalledWith(1, { documentId, versionId: previous.versionId, contentItemId: 'item-two', representationId: 'rep-b', purpose: 'history' }, { signal: expect.any(AbortSignal) });
  expect(h.api.downloadVersionFile).toHaveBeenNthCalledWith(2, { documentId, versionId: previous.versionId, contentItemId: 'item-one', representationId: 'rep-a', purpose: 'history' }, { signal: expect.any(AbortSignal) });
  expect(h.api.markDocumentVersionRead).not.toHaveBeenCalled();
});

test.each(['disabled', 'hidden', 'missing'])('download capabilityが%sなら原本downloadを提示しない', async status => {
  const h = setup(); const original = h.api.getDocumentVersion.getMockImplementation()!;
  h.api.getDocumentVersion.mockImplementation((id, vid, purpose) => purpose === 'history' ? Promise.resolve({ ...previous, capabilities: { ...previous.capabilities, download: status === 'missing' ? undefined : { status, reason: 'permission' } } }) : original(id, vid, purpose));
  await open(); await select(); await waitFor(() => expect(h.api.listVersionFiles).toHaveBeenCalledWith(documentId, previous.versionId, 'history'));
  expect(region().queryByRole('button', { name: /^履歴の原本を取得:/ })).not.toBeInTheDocument();
});

test('100+1件のopaque cursorと重複除去を守り、短い空pageでも続きがあれば取得する', async () => {
  const h = setup(); const hundred = Array.from({ length: 100 }, (_, i) => version(200 - i)); const cursor = 'opaque+/=?日本語'; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : Promise.resolve(token === 'tail' ? page([hundred[99]!, tail]) : token === cursor ? page([], 'tail') : page(hundred, cursor)));
  await open(); expect(within(chooser()).getAllByRole('option')).toHaveLength(101); fireEvent.click(more()); await waitFor(() => expect(h.api.listDocumentVersions).toHaveBeenCalledWith(documentId, 'history', cursor));
  await waitFor(() => expect(more()).not.toBeDisabled()); fireEvent.click(more()); await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(102));
  expect(h.api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'history', 'tail'); expect(region().queryByRole('button', { name: 'コンテンツ版の履歴をさらに表示' })).not.toBeInTheDocument();
  expect(region().getByText(/取得中の変更により/)).toBeVisible();
});

test.each([new TypeError('network'), problem('DEPENDENCY_UNAVAILABLE', 503, true)])('追加の一時失敗だけは行・選択と同cursorを保ち、連打をまとめる: %p', async failure => {
  const h = setup(); const delayed = deferred<VersionList>(); let count = 0; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : !token ? Promise.resolve(page([current, previous], 'same')) : ++count === 1 ? Promise.reject(failure) : delayed.promise);
  await open(); await select(); fireEvent.click(more()); await region().findByRole('alert'); expect(chooser()).toHaveValue(previous.versionId); expect(selected().getByText('履歴タイトル101')).toBeVisible();
  act(() => { const retry = region().getByRole('button', { name: 'コンテンツ版の履歴の続きを再試行' }); retry.click(); retry.click(); }); expect(count).toBe(2);
  await act(async () => delayed.resolve(page([tail]))); expect(h.api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'history', 'same');
});

test.each([['FORBIDDEN', 403], ['AUTHENTICATION_REQUIRED', 401], ['DOCUMENT_NOT_FOUND', 404], ['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['INTERNAL', 500]])('続きの%sは全履歴を隠し、画面往復・reset後も明示再読取まで停止する', async (code, status) => {
  const h = setup(undefined, 1); let fresh = false; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : token ? Promise.reject(problem(code, Number(status))) : Promise.resolve(page([current, previous], fresh ? null : 'old')));
  await open(); await select(); await downloadReady(); fireEvent.click(more()); await region().findByRole('alert');
  expect(region().queryByRole('combobox')).not.toBeInTheDocument(); expect(screen.queryByText('履歴タイトル101')).not.toBeInTheDocument(); expect(region().queryByText('原本ファイルはありません')).not.toBeInTheDocument();
  const count = historyCalls(h.api.listDocumentVersions).length; fresh = true; fireEvent.click(close()); await openBlocked();
  await act(async () => refreshFolderMoveReads(h.client)); expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(count);
  fireEvent.click(restart()); await waitFor(() => expect(chooser()).toHaveValue('')); expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(count + 1);
});
async function openBlocked() { fireEvent.click(await screen.findByRole('button', { name: 'コンテンツ版の履歴を開く' })); await region().findByRole('alert'); }

test.each(['detail', 'files', 'download'])('%sの現在拒否は通常文書を保持して履歴の成功cacheを捨てる', async kind => {
  const h = setup(undefined, 1); await open();
  if (kind === 'detail') { const original = h.api.getDocumentVersion.getMockImplementation()!; h.api.getDocumentVersion.mockImplementation((id, vid, purpose) => purpose === 'history' ? Promise.reject(problem('FORBIDDEN', 403)) : original(id, vid, purpose)); fireEvent.change(chooser(), { target: { value: previous.versionId } }); }
  else if (kind === 'files') { h.api.listVersionFiles.mockRejectedValue(problem('DOCUMENT_VERSION_NOT_FOUND', 404)); fireEvent.change(chooser(), { target: { value: previous.versionId } }); }
  else { await select(); h.api.downloadVersionFile.mockRejectedValueOnce(problem('AUTHENTICATION_REQUIRED', 401)).mockResolvedValue(new Blob(['late'])); fireEvent.click(await downloadReady()); }
  await region().findByRole('alert'); expect(region().queryByRole('combobox')).not.toBeInTheDocument(); expect(screen.getByRole('heading', { name: '合成文書' })).toBeVisible();
  expect(screen.queryByText('履歴タイトル101')).not.toBeInTheDocument(); expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test('通常詳細の401を自動再試行200が上書きしても履歴の拒否を維持する', async () => {
  const h = setup(undefined, 1); await open(); await select(); await downloadReady(); const count = historyCalls(h.api.listDocumentVersions).length;
  h.api.getDocument.mockRejectedValueOnce(problem('AUTHENTICATION_REQUIRED', 401)).mockResolvedValue(detail());
  await act(async () => h.client.invalidateQueries({ queryKey: ['document', documentId] })); await screen.findByRole('heading', { name: '合成文書' });
  if (screen.queryByRole('button', { name: 'コンテンツ版の履歴を開く' })) await openBlocked(); else await region().findByRole('alert');
  expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(count); expect(screen.queryByText('履歴タイトル101')).not.toBeInTheDocument();
  fireEvent.click(restart()); await waitFor(() => expect(chooser()).toBeInTheDocument());
});

// 片方の拒否接続を落とす、または片方の再読取で両markerを消すと、他方の履歴が自動復活する。
test.each(['event', 'content'])('通常詳細の拒否は両履歴を止め、%sから明示再読取しても他方を解除しない', async first => {
  const h = setup(undefined, 1);
  h.api.getDocumentHistory.mockResolvedValue({ items: [{ sourceKind: 'operation', sourceKey: 'same-document-event', actionCode: '合成イベント', occurredAt: current.createdAt, details: {}, provenanceQuality: 'operationLedger' }], nextCursor: null });
  await open(); await select(); await downloadReady();
  const contentReadCount = historyCalls(h.api.listDocumentVersions).length;
  fireEvent.click(screen.getByRole('tab', { name: '履歴' })); await screen.findByText('合成イベント');
  const eventReadCount = h.api.getDocumentHistory.mock.calls.length;
  h.api.getDocument.mockRejectedValueOnce(problem('AUTHENTICATION_REQUIRED', 401)).mockResolvedValue(detail());
  await act(async () => h.client.invalidateQueries({ queryKey: ['document', documentId] }));
  await screen.findByRole('heading', { name: '合成文書' });

  const eventRegion = () => within(screen.getByRole('region', { name: '変更履歴' }));
  async function eventDenied() {
    fireEvent.click(screen.getByRole('tab', { name: '履歴' }));
    await waitFor(() => expect(eventRegion().getByRole('alert')).toBeVisible());
    expect(eventRegion().queryByText('合成イベント')).not.toBeInTheDocument();
    expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(eventReadCount);
  }
  async function contentDenied() {
    fireEvent.click(screen.getByRole('tab', { name: '版・改訂' })); await openBlocked();
    expect(region().queryByRole('combobox')).not.toBeInTheDocument();
    expect(screen.queryByText('履歴タイトル101')).not.toBeInTheDocument();
    expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(contentReadCount);
  }
  await eventDenied(); await contentDenied();
  if (first === 'event') {
    await eventDenied();
    fireEvent.click(eventRegion().getByRole('button', { name: '変更履歴を最初から読み直す' }));
    await screen.findByText('合成イベント'); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(eventReadCount + 1);
    await contentDenied(); fireEvent.click(restart()); await waitFor(() => expect(chooser()).toBeInTheDocument());
    expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(contentReadCount + 1);
    await select(); await downloadReady();
  } else {
    fireEvent.click(restart()); await waitFor(() => expect(chooser()).toBeInTheDocument());
    expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(contentReadCount + 1);
    await select(); await downloadReady(); await eventDenied();
    fireEvent.click(eventRegion().getByRole('button', { name: '変更履歴を最初から読み直す' }));
    await screen.findByText('合成イベント'); expect(h.api.getDocumentHistory).toHaveBeenCalledTimes(eventReadCount + 1);
  }
  expect(h.router.state.location.search.versionId).toBe(current.versionId);
  expect(h.api.markDocumentVersionRead).not.toHaveBeenCalled();
});

test('明示再読取は旧cursorを捨て、見つからない明示IDを他版へ置換しない', async () => {
  const h = setup(); let fresh = false; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : Promise.resolve(token ? page([previous]) : fresh ? page([current], 'new') : page([current, previous], 'old')));
  await open(); await select(); const count = h.api.getDocumentVersion.mock.calls.filter(call => call[2] === 'history').length; fresh = true; fireEvent.click(restart());
  await region().findByText(/未取得の選択/); expect(chooser()).toHaveValue(previous.versionId); expect(screen.queryByText('履歴タイトル101')).not.toBeInTheDocument();
  expect(h.api.getDocumentVersion.mock.calls.filter(call => call[2] === 'history')).toHaveLength(count); fireEvent.click(more()); await waitFor(() => expect(selected().getByText('履歴タイトル101')).toBeVisible());
  expect(h.api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'history', 'new');
});

test.each(['close', 'tab', 'route', 'document'])('%s後に戻ると成功cache・選択・cursorを捨てて先頭から開く', async kind => {
  const h = setup(); const original = h.api.listDocumentVersions.getMockImplementation()!; h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : Promise.resolve(token ? page([tail]) : page([current, previous], 'old')));
  await open(); await select(); fireEvent.click(more()); await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(4));
  if (kind === 'close') fireEvent.click(close());
  else if (kind === 'tab') { fireEvent.click(screen.getByRole('tab', { name: '概要' })); await screen.findByRole('heading', { name: '基本情報' }); fireEvent.click(screen.getByRole('tab', { name: '版・改訂' })); }
  else { await act(async () => h.router.navigate(kind === 'route' ? { to: '/tasks' } as never : { to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'published', tab: 'versions' } })); await act(async () => h.history.back()); }
  await open(); expect(chooser()).toHaveValue(''); expect(within(chooser()).getAllByRole('option')).toHaveLength(3); expect(h.api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'history', undefined);
});

test.each(['lifecycle', 'move'])('%sの既存read失効は履歴へ届き、UNKNOWN/blob/Organizationを保持する', async kind => {
  const h = setup(); let fresh = false; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : Promise.resolve(token ? page([tail]) : page([current, previous], fresh ? null : 'old')));
  await open(); fireEvent.click(more()); await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(4)); await select();
  const blob = new Blob(['fixed']); const fixed: WorkingOperation = { status: 'unknown', intent: { kind: 'create', documentId, sourceVersionId: current.versionId, body: { operationId: 'fixed', targetVersionId: 'target', expectedRevision: 7, title: '合成作業版', items: [] }, files: new Map([['fixed', blob]]), prepared: { body: blob, contentType: 'multipart/form-data; boundary=fixed' } } };
  const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed', expectedDocumentRevision: 7, set: { category: 'keep' }, unset: [], reason: '合成理由' } };
  h.client.setQueryData(workingOperationKey(documentId), fixed); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'same' });
  fresh = true; await act(async () => kind === 'move' ? refreshFolderMoveReads(h.client) : refreshLifecycleQueries(h.client, documentId));
  await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(3)); expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixed); expect((fixed.intent as WorkingWriteIntent).prepared.body).toBe(blob); expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'same' });
});

test.each(['list', 'detail', 'files'].flatMap(kind => ['success', 'refusal'].map(outcome => [kind, outcome])))('取消した%sの遅い%sを再度開いた対象へ注入しない', async (kind, outcome) => {
  const h = setup(); const pending = deferred<unknown>(); const method = kind === 'list' ? h.api.listDocumentVersions : kind === 'detail' ? h.api.getDocumentVersion : h.api.listVersionFiles; const original = method.getMockImplementation()!; let delay = true;
  method.mockImplementation((...args) => delay && (kind === 'list' ? args[1] === 'history' : args[2] === 'history') ? pending.promise : original(...args));
  fireEvent.click(await screen.findByRole('button', { name: 'コンテンツ版の履歴を開く' }));
  if (kind !== 'list') { await waitFor(() => expect(chooser()).toBeInTheDocument()); fireEvent.change(chooser(), { target: { value: previous.versionId } }); }
  await waitFor(() => expect(method.mock.calls.some(args => kind === 'list' ? args[1] === 'history' : args[2] === 'history')).toBe(true));
  fireEvent.click(close()); delay = false; await open(); await select(current.versionId); await downloadReady();
  await act(async () => outcome === 'refusal' ? pending.reject(problem('FORBIDDEN', 403)) : pending.resolve(kind === 'list' ? page([tail], 'obsolete') : kind === 'detail' ? previous : { items: [] })); expect(selected().getByText('履歴タイトル102')).toBeVisible(); expect(region().queryByRole('alert')).not.toBeInTheDocument();
});

test.each(['selection', 'close', 'tab', 'route', 'document', 'restart', 'invalidate', 'denial'])('%sでdownloadを中断し、遅いBlobを保存しない', async kind => {
  const h = setup(); const pending = deferred<Blob>(); h.api.downloadVersionFile.mockReturnValue(pending.promise); await open(); await select(); fireEvent.click(await downloadReady());
  const signal = h.api.downloadVersionFile.mock.calls[0]![1].signal as AbortSignal;
  if (kind === 'selection') fireEvent.change(chooser(), { target: { value: current.versionId } });
  else if (kind === 'close') fireEvent.click(close());
  else if (kind === 'tab') fireEvent.click(screen.getByRole('tab', { name: '概要' }));
  else if (kind === 'route') await act(async () => h.router.navigate({ to: '/tasks' } as never));
  else if (kind === 'document') await act(async () => h.router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'published', tab: 'versions' } }));
  else if (kind === 'restart') fireEvent.click(restart());
  else if (kind === 'invalidate') await act(async () => h.client.invalidateQueries({ queryKey: ['document-version-files', documentId] }));
  else { h.api.getDocument.mockRejectedValue(problem('FORBIDDEN', 403)); await act(async () => h.client.invalidateQueries({ queryKey: ['document', documentId] })); }
  await waitFor(() => expect(signal.aborted).toBe(true)); await act(async () => pending.resolve(new Blob(['late']))); expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled();
});


test('prefix失効はfresh headの新cursorでtailを再構成し、単ページcacheと混ざらない', async () => {
  const h = setup(); let fresh = false; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.client.setQueryData(['document-versions', documentId, 'history'], page([version(77)], 'single'));
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : Promise.resolve(token ? page([fresh ? version(99) : tail]) : page([current, previous], fresh ? 'new' : 'old')));
  await open(); fireEvent.click(more()); await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(4)); fresh = true;
  await act(async () => h.client.invalidateQueries({ queryKey: ['document-versions', documentId] }));
  await waitFor(() => expect(within(chooser()).getByRole('option', { name: 'Version 99 · 過去版' })).toBeInTheDocument());
  expect(within(chooser()).queryByRole('option', { name: 'Version 100 · 過去版' })).not.toBeInTheDocument();
  expect(h.api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'history', 'new');
  expect(h.client.getQueryData(['document-versions', documentId, 'history'])).toEqual(page([version(77)], 'single'));
});

test.each(['selection', 'close', 'invalidate'])('%sと同tickの遅いBlobでも保存しない', async kind => {
  const h = setup(); const pending = deferred<Blob>(); h.api.downloadVersionFile.mockReturnValue(pending.promise); await open(); await select(); fireEvent.click(await downloadReady());
  await act(async () => {
    if (kind === 'selection') fireEvent.change(chooser(), { target: { value: current.versionId } });
    else if (kind === 'close') fireEvent.click(close());
    else void h.client.invalidateQueries({ queryKey: ['document-version-files', documentId] });
    pending.resolve(new Blob(['late']));
  });
  expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled();
});

test('閉じた後の遅いfocus復帰はユーザーの別操作へ割り込まない', async () => {
  setup(); await open(); const callbacks: FrameRequestCallback[] = [];
  jest.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { callbacks.push(callback); return callbacks.length; });
  fireEvent.click(close()); const normal = screen.getByRole('tab', { name: '概要' }); normal.focus();
  act(() => callbacks.forEach(callback => callback(0))); expect(normal).toHaveFocus();
});

test('背景再読取のtail一時失敗は追加pageの再試行と区別し、明示先頭readまで停止する', async () => {
  const h = setup(); let phase = 'old'; const original = h.api.listDocumentVersions.getMockImplementation()!;
  h.api.listDocumentVersions.mockImplementation((id, purpose, token) => purpose !== 'history' ? original(id, purpose) : token === 'new' && phase === 'failed' ? Promise.reject(new TypeError('network')) : Promise.resolve(token ? page([tail]) : page([current, previous], phase === 'old' ? 'old' : 'new')));
  await open(); fireEvent.click(more()); await waitFor(() => expect(within(chooser()).getAllByRole('option')).toHaveLength(4)); await select(); phase = 'failed';
  await act(async () => h.client.invalidateQueries({ queryKey: ['document-versions', documentId] })); await region().findByRole('alert');
  expect(region().queryByRole('combobox')).not.toBeInTheDocument(); expect(region().queryByRole('button', { name: 'コンテンツ版の履歴の続きを再試行' })).not.toBeInTheDocument();
  phase = 'recovered'; const count = historyCalls(h.api.listDocumentVersions).length;
  await act(async () => h.client.invalidateQueries({ queryKey: ['document-versions', documentId] }));
  expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(count); expect(region().queryByRole('combobox')).not.toBeInTheDocument();
  fireEvent.click(restart()); await waitFor(() => expect(chooser()).toHaveValue(previous.versionId)); expect(historyCalls(h.api.listDocumentVersions)).toHaveLength(count + 1);
});
