import { onlineManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentList, type VersionDetail, type FileList } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';
import { workingOperationKey, type WorkingOperation, type WorkingWriteIntent } from '../src/application/document-working-version';
import { metadataOperations, type MetadataOperation } from '../src/application/document-metadata';
import { OrganizationProvider, useOrganizationContext, useTaskTransient } from '../src/application/organization-context';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), getDocument: jest.fn(),
  listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(), listDocumentRevisions: jest.fn(), getDocumentHistory: jest.fn(),
  listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(), publishVersion: jest.fn(), withdrawVersion: jest.fn(),
  endDocumentPublication: jest.fn(), patchDocumentMetadata: jest.fn(), moveDocument: jest.fn(), markDocumentVersionRead: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const versionId = '00000000-0000-4000-8000-000000000011';
const available = { status: 'available' as const }, denied = { status: 'disabled' as const, reason: 'permission' as const };
const version: VersionDetail = { versionId, versionNo: 1, baseVersionId: null, lifecycleState: 'published', isCurrent: false,
  createdAt: '2026-10-01T00:00:00Z', approvedAt: null, scheduledPublishAt: null, publishedAt: '2026-10-02T00:00:00Z', withdrawnAt: null, updatedAt: '2026-10-02T00:00:00Z',
  fileSummary: { authoritativeItemCount: 2, totalSizeBytes: 12, primary: null }, firstReadAt: null, title: '旧版の題名', metadata: { category: '旧版の属性' }, currentPublicationScheduleId: null,
  capabilities: { edit: available, rebase: available, publish: available, withdraw: available, schedulePublication: available, cancelPublicationSchedule: available, download: available } };
type HistoryDocument = Extract<DocumentList, { view: 'history' }>['items'][number];
const item = (id = documentId): HistoryDocument => ({ documentId: id, documentVersionId: versionId, title: id === documentId ? '終了文書の代表名' : '全版取下げの代表名',
  lifecycleState: id === documentId ? 'published' : 'withdrawn', ended: id === documentId, folderId: null, folderName: null, revision: 7,
  metadata: { category: '現在値のcategory' }, createdAt: version.createdAt, displayVersion: { ...version, versionNo: 2, lifecycleState: id === documentId ? 'PUBLISHED' : 'WITHDRAWN' },
  displayRevision: null, readState: { isRead: true, firstReadAt: null }, displayTimestamp: { kind: 'revisionCreatedAt', value: version.publishedAt! } });
const list = (ids = [documentId, otherId], nextCursor: string | null = null): DocumentList => ({ view: 'history', items: ids.map(item), nextCursor });
const files: FileList = { items: [
  { contentItemId: 'item-b', representationId: 'rep-b', logicalPath: 'b', ordinal: 2, role: 'AUTHORITATIVE', displayName: '旧原本B.pdf', mediaType: 'application/pdf', sizeBytes: 6 },
  { contentItemId: 'item-a', representationId: 'rep-a', logicalPath: 'a', ordinal: 1, role: 'AUTHORITATIVE', displayName: '旧原本A.pdf', mediaType: 'application/pdf', sizeBytes: 6 },
  { contentItemId: 'preview', representationId: 'preview', logicalPath: 'preview', ordinal: 0, role: 'PREVIEW', displayName: '非原本.pdf', mediaType: 'application/pdf', sizeBytes: 1 },
] };
const problem = (status: number) => ({ type: 'about:blank', title: 'Synthetic refusal', status, code: status === 404 ? 'DOCUMENT_NOT_FOUND' : status === 401 ? 'AUTHENTICATION_REQUIRED' : 'FORBIDDEN', traceId: 'synthetic', retryable: false });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const clients: QueryClient[] = [];
afterEach(() => { clients.splice(0).forEach(client => { metadataOperations(client).clear(documentId); client.clear(); }); jest.restoreAllMocks(); });
function setup(entry = '/documents?view=published&panel=closed', retry: boolean | number = false, seed?: (client: QueryClient) => void) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock } & { markDocumentVersionRead: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getRootFolder.mockResolvedValue({ folderId: 'root', name: '見えているルート', revision: 1, parentFolderId: null, capabilities: {} });
  api.listFolderChildren.mockResolvedValue({ items: [], nextCursor: null, capabilities: {} });
  api.listDocuments.mockImplementation(query => Promise.resolve(query.view === 'history' ? list() : { view: query.view, items: [], nextCursor: null }));
  api.getDocument.mockRejectedValue(problem(404));
  api.listDocumentVersions.mockResolvedValue({ items: [version], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue(version); api.listVersionFiles.mockResolvedValue(files);
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockImplementation(id => Promise.resolve({ items: [{ sourceKind: 'operation', sourceKey: id, actionCode: id === documentId ? 'T10終了記録' : '取下げ記録', occurredAt: null, details: {}, provenanceQuality: 'operationLedger' }], nextCursor: null }));
  api.downloadVersionFile.mockResolvedValue(new Blob(['synthetic']));
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn().mockReturnValue('blob:synthetic') });
  Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() });
  jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  const root = createRootRoute({ component: () => <><ContextProbe /><Outlet /></> });
  const home = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const detail = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const history = createMemoryHistory({ initialEntries: [entry] });
  const router = createRouter({ routeTree: root.addChildren([home, detail]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry, retryDelay: 0, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  seed?.(client);
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { api, router, client, history };
}
function ContextProbe() {
  const organization = useOrganizationContext();
  const [transient, update] = useTaskTransient('history-workspace-test');
  return <section aria-label="合成Organization保持">
    <button onClick={() => { organization.setContext({ principalId: 'synthetic', displayName: '合成主体', actingAssignmentId: 'synthetic-assignment', capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: false } }, '/tasks?taskId=synthetic'); update(previous => ({ ...previous, draft: '保持する未保存文案', unknown: true })); }}>合成文脈を保存</button>
    <span>{organization.session?.displayName}</span><span>{transient.draft}</span><span>{transient.unknown ? '未確定作業' : ''}</span>
  </section>;
}
const panel = () => within(screen.getByRole('region', { name: '選択した文書の履歴' }));
const chooser = () => panel().getByRole('combobox', { name: '履歴のコンテンツ版を選択' });
async function selectDocument(id = documentId) { fireEvent.click(await screen.findByRole('button', { name: item(id).title })); await screen.findByRole('region', { name: '選択した文書の履歴' }); }
async function openContent() { fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を開く' })); await waitFor(() => expect(chooser()).toHaveValue('')); }
async function selectVersion() { fireEvent.change(chooser(), { target: { value: versionId } }); await panel().findByRole('button', { name: '履歴の原本を取得: 旧原本B.pdf' }); }

test('履歴一覧のイベント日時も既存詳細と同じJSTで表示する', async () => {
  const h = setup('/documents?view=history&panel=closed');
  h.api.getDocumentHistory.mockResolvedValue({ items: [{ sourceKind: 'operation', sourceKey: 'dated-event', actionCode: '日時付き終了記録', occurredAt: '2026-10-01T23:30:00Z', details: {}, provenanceQuality: 'operationLedger' }], nextCursor: null });
  await selectDocument(); await panel().findByText('日時付き終了記録');
  const events = within(panel().getByRole('region', { name: '変更履歴' }));
  expect(events.getByRole('listitem').querySelector('time')).toHaveTextContent('2026/10/02 8:30 (Asia/Tokyo, UTC+09:00)');
});

// Removing the normal navigation entry or routing a history row through normal detail breaks this real-route journey.
test('通常navから履歴一覧で終了/全版取下げ文書を明示選択し旧版の全原本とイベントを読む', async () => {
  const h = setup(); fireEvent.click(await screen.findByRole('link', { name: '文書履歴' }));
  await screen.findByRole('heading', { name: '文書履歴', level: 1 });
  await screen.findByRole('button', { name: item().title });
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument();
  expect(h.api.getDocument).not.toHaveBeenCalled(); expect(h.api.getDocumentHistory).not.toHaveBeenCalled();
  await selectDocument(); await panel().findByText('T10終了記録'); await openContent(); await selectVersion();
  expect(h.api.getDocumentVersion).toHaveBeenCalledWith(documentId, versionId, 'history');
  expect(panel().getAllByRole('button', { name: /^履歴の原本を取得:/ }).map(button => button.textContent)).toEqual(['履歴の原本を取得: 旧原本B.pdf', '履歴の原本を取得: 旧原本A.pdf']);
  fireEvent.click(panel().getByRole('button', { name: '履歴の原本を取得: 旧原本B.pdf' }));
  await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalledTimes(1));
  expect(h.api.downloadVersionFile).toHaveBeenCalledWith({ documentId, versionId, contentItemId: 'item-b', representationId: 'rep-b', purpose: 'history' }, { signal: expect.any(AbortSignal) });
  expect(panel().getByText('現在値のcategory')).toBeVisible(); expect(panel().getByText('旧版の属性')).toBeVisible();
  expect(panel().getByText('公開終了済み')).toBeVisible(); expect(panel().getByText('PUBLISHED')).toBeVisible();
  expect(panel().queryByText('見えているルート')).not.toBeInTheDocument(); expect(panel().queryByRole('button', { name: '公開する' })).not.toBeInTheDocument();
  await selectDocument(otherId); await panel().findByText('取下げ記録');
  expect(panel().getByText('公開終了していません')).toBeVisible(); expect(panel().getByText('WITHDRAWN')).toBeVisible();
  expect(h.api.getDocument).not.toHaveBeenCalled();
  for (const method of [h.api.publishVersion, h.api.withdrawVersion, h.api.endDocumentPublication, h.api.patchDocumentMetadata, h.api.moveDocument, h.api.markDocumentVersionRead]) expect(method).not.toHaveBeenCalled();
  expect(h.router.state.location.pathname).toBe('/documents');
});

// A missing explicit ID must never select the first authorized row.
test.each(['', '&selectedDocumentId=00000000-0000-4000-8000-000000000099'])('未選択または現pageにない明示IDは先頭行へfallbackしない: %s', async suffix => {
  const h = setup(`/documents?view=history&panel=open${suffix}`); await screen.findByRole('button', { name: item().title });
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument();
  expect(h.api.getDocument).not.toHaveBeenCalled(); expect(h.api.getDocumentHistory).not.toHaveBeenCalled(); expect(h.api.listDocumentVersions).not.toHaveBeenCalled();
});

test('履歴入口は公開一覧の未読条件を持ち込まず通常APIを使わない', async () => {
  const h = setup('/documents?view=published&unreadOnly=true&panel=closed');
  fireEvent.click(await screen.findByRole('link', { name: '文書履歴' })); await screen.findByRole('button', { name: item().title });
  expect(h.router.state.location.search.unreadOnly).toBeUndefined(); expect(screen.queryByRole('checkbox', { name: '未読のみ' })).not.toBeInTheDocument();
  expect(h.api.listDocuments).toHaveBeenLastCalledWith({ view: 'history', sort: 'created_at_desc', pageSize: 50 });
});

test.each([401, 403, 404])('一覧%sをAPI境界で保持し自動retry200/reset/往復でも旧panelを復活させない', async status => {
  const h = setup('/documents?view=history', 1); await selectDocument(); await panel().findByText('T10終了記録'); await openContent(); await selectVersion();
  const calls = h.api.listDocuments.mock.calls.length; h.api.listDocuments.mockRejectedValueOnce(problem(status)).mockResolvedValue(list());
  await act(async () => h.client.invalidateQueries({ queryKey: ['documents'] })); await screen.findByRole('alert');
  expect(h.api.listDocuments).toHaveBeenCalledTimes(calls + 1); expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument();
  await act(async () => refreshFolderMoveReads(h.client)); expect(h.api.listDocuments).toHaveBeenCalledTimes(calls + 1);
  fireEvent.click(screen.getByRole('link', { name: '編集作業' })); await waitFor(() => expect(h.router.state.location.search.view).toBe('authoring'));
  fireEvent.click(screen.getByRole('link', { name: '文書履歴' })); await screen.findByRole('alert');
  const before = h.api.listDocuments.mock.calls.length; fireEvent.click(screen.getByRole('button', { name: '文書履歴一覧を読み直す' }));
  await screen.findByRole('button', { name: item().title }); expect(h.api.listDocuments).toHaveBeenCalledTimes(before + 1);
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); await selectDocument(); await panel().findByText('T10終了記録');
});

test('一覧の再取得中と対象消失はsummary/旧版/原本を隠し別行へfallbackしない', async () => {
  const h = setup('/documents?view=history'); await selectDocument(); await openContent(); await selectVersion();
  const pending = deferred<DocumentList>(); h.api.listDocuments.mockReturnValue(pending.promise);
  act(() => { void h.client.invalidateQueries({ queryKey: ['documents'] }); });
  await waitFor(() => expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument());
  expect(screen.queryByText('旧版の題名')).not.toBeInTheDocument();
  await act(async () => pending.resolve(list([otherId]))); await screen.findByRole('button', { name: item(otherId).title });
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); expect(h.router.state.location.search.selectedDocumentId).toBe(documentId);
});

test('historyと異なるviewの応答から履歴行を選ばない', async () => {
  const h = setup(`/documents?view=history&selectedDocumentId=${documentId}`); h.api.listDocuments.mockResolvedValue({ ...list(), view: 'published' });
  await screen.findByRole('alert');
  expect(screen.queryByText('文書がありません')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).not.toHaveBeenCalled();
});

test.each(['filter', 'page', 'sort', 'size'])('%s変更では同じIDが次応答にあっても明示選択をやり直す', async kind => {
  const h = setup('/documents?view=history'); h.api.listDocuments.mockResolvedValue(list(undefined, 'opaque-next'));
  await selectDocument(); await panel().findByText('T10終了記録');
  if (kind === 'filter') { fireEvent.change(screen.getByRole('searchbox', { name: '文書名で絞り込み' }), { target: { value: '終了' } }); fireEvent.click(screen.getByRole('button', { name: '絞り込む' })); }
  else if (kind === 'page') fireEvent.click(screen.getByRole('button', { name: '次のページ' }));
  else if (kind === 'sort') fireEvent.change(screen.getByRole('combobox', { name: '並び順' }), { target: { value: 'title_asc' } });
  else fireEvent.change(screen.getByRole('combobox', { name: '1ページあたりの件数' }), { target: { value: '25' } });
  await waitFor(() => expect(h.api.listDocuments).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); await selectDocument(); await panel().findByText('T10終了記録');
});

test('通常T10後404はhistoryの恒久拒否にせず各既存controlからfresh readで再開する', async () => {
  const h = setup(`/documents/${documentId}?view=published&tab=history`); await screen.findByRole('alert');
  expect(h.api.getDocument).toHaveBeenCalledWith(documentId, 'published'); const normalCalls = h.api.getDocument.mock.calls.length;
  fireEvent.click(screen.getByRole('link', { name: '文書履歴' })); await selectDocument();
  expect(h.api.getDocumentHistory).not.toHaveBeenCalled();
  fireEvent.click(panel().getByRole('button', { name: '変更履歴を最初から読み直す' })); await panel().findByText('T10終了記録');
  fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を開く' }));
  expect(h.api.listDocumentVersions).not.toHaveBeenCalled();
  fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を最初から読み直す' })); await waitFor(() => expect(chooser()).toHaveValue('')); await selectVersion();
  expect(h.api.getDocument).toHaveBeenCalledTimes(normalCalls); expect(h.api.getDocumentVersion).toHaveBeenCalledWith(documentId, versionId, 'history');
});

test.each(['event', 'content'])('history自身の%s拒否はview往復と一覧再読取から無言で解除しない', async kind => {
  const h = setup('/documents?view=history', 1); const method = kind === 'event' ? h.api.getDocumentHistory : h.api.listDocumentVersions;
  const original = method.getMockImplementation()!; method.mockRejectedValueOnce(problem(403)).mockImplementationOnce(original);
  await selectDocument(); if (kind === 'content') fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を開く' })); await panel().findByRole('alert');
  expect(method).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('link', { name: '文書' })); await waitFor(() => expect(h.router.state.location.search.view).toBe('published'));
  fireEvent.click(screen.getByRole('link', { name: '文書履歴' })); await selectDocument(); if (kind === 'content') fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を開く' }));
  await panel().findByRole('alert'); expect(method).toHaveBeenCalledTimes(1);
  fireEvent.click(panel().getByRole('button', { name: kind === 'event' ? '変更履歴を最初から読み直す' : 'コンテンツ版の履歴を最初から読み直す' }));
  if (kind === 'event') await panel().findByText('T10終了記録'); else await waitFor(() => expect(chooser()).toHaveValue(''));
  expect(method).toHaveBeenCalledTimes(2);
});

test.each(['invalidate', 'denial', 'close', 'selection', 'filter', 'page', 'navigation'])('%sと同tickのdownload Blobを保存せず中断する', async kind => {
  const h = setup('/documents?view=history'); h.api.listDocuments.mockResolvedValue(list(undefined, 'next'));
  const pending = deferred<Blob>(); h.api.downloadVersionFile.mockReturnValue(pending.promise); await selectDocument(); await openContent(); await selectVersion();
  fireEvent.click(panel().getByRole('button', { name: '履歴の原本を取得: 旧原本B.pdf' })); const signal = h.api.downloadVersionFile.mock.calls[0]![1].signal as AbortSignal;
  await act(async () => {
    if (kind === 'invalidate' || kind === 'denial') { h.api.listDocuments.mockImplementation(() => kind === 'denial' ? Promise.reject(problem(403)) : new Promise(() => {})); void h.client.invalidateQueries({ queryKey: ['documents'] }); }
    else if (kind === 'close') fireEvent.click(panel().getByRole('button', { name: '履歴パネルを閉じる' }));
    else if (kind === 'selection') fireEvent.click(screen.getByRole('button', { name: item(otherId).title }));
    else if (kind === 'filter') { fireEvent.change(screen.getByRole('searchbox', { name: '文書名で絞り込み' }), { target: { value: 'changed' } }); fireEvent.click(screen.getByRole('button', { name: '絞り込む' })); }
    else if (kind === 'page') fireEvent.click(screen.getByRole('button', { name: '次のページ' }));
    else fireEvent.click(screen.getByRole('link', { name: '文書' }));
    pending.resolve(new Blob(['late']));
  });
  expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled(); await waitFor(() => expect(signal.aborted).toBe(true));
});

test.each(['event', 'content', 'detail', 'files'].flatMap(kind => ['success', 'refusal'].map(outcome => [kind, outcome])))('一覧panelを閉じた%sの遅い%sが再選択へ入らない', async (kind, outcome) => {
  const h = setup('/documents?view=history'); const pending = deferred<unknown>();
  const method = kind === 'event' ? h.api.getDocumentHistory : kind === 'content' ? h.api.listDocumentVersions : kind === 'detail' ? h.api.getDocumentVersion : h.api.listVersionFiles;
  const original = method.getMockImplementation()!; let delay = true; method.mockImplementation((...args) => delay ? pending.promise : original(...args));
  await selectDocument();
  if (kind !== 'event') { fireEvent.click(panel().getByRole('button', { name: 'コンテンツ版の履歴を開く' })); if (kind !== 'content') { await waitFor(() => expect(chooser()).toHaveValue('')); fireEvent.change(chooser(), { target: { value: versionId } }); } }
  await waitFor(() => expect(method).toHaveBeenCalled()); fireEvent.click(panel().getByRole('button', { name: '履歴パネルを閉じる' }));
  await waitFor(() => expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument()); delay = false;
  await selectDocument(); await openContent(); await selectVersion(); await panel().findByText('T10終了記録');
  await act(async () => outcome === 'refusal' ? pending.reject(problem(403)) : pending.resolve(kind === 'detail' ? { ...version, title: '遅い旧題名' } : { items: [], nextCursor: null }));
  expect(panel().getByText('旧版の題名')).toBeVisible(); expect(panel().queryByRole('alert')).not.toBeInTheDocument(); expect(panel().getByText('T10終了記録')).toBeVisible();
});

test('事前cacheがあっても初回fresh一覧応答前に明示IDのpanelを開かない', async () => {
  const pending = deferred<DocumentList>();
  const h = setup(`/documents?view=history&selectedDocumentId=${documentId}`, false, client => {
    client.setQueryData(['documents', { view: 'history', includeDescendants: false, sort: 'created_at_desc', pageSize: 50 }], list());
  });
  h.api.listDocuments.mockReturnValue(pending.promise); await waitFor(() => expect(h.api.listDocuments).toHaveBeenCalled());
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); expect(h.api.getDocumentHistory).not.toHaveBeenCalled();
  await act(async () => pending.resolve(list())); await panel().findByText('T10終了記録');
});

test.each(['success', 'refusal'])('取消した旧条件一覧の遅い%sは現在条件と戻りを汚染しない', async outcome => {
  const pending = deferred<DocumentList>(); const h = setup('/documents?view=history'); h.api.listDocuments.mockImplementation(query => query.titleContains ? Promise.resolve(list([otherId])) : pending.promise);
  await waitFor(() => expect(h.api.listDocuments).toHaveBeenCalled());
  fireEvent.change(screen.getByRole('searchbox', { name: '文書名で絞り込み' }), { target: { value: '新条件' } }); fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await selectDocument(otherId); await panel().findByText('取下げ記録');
  await act(async () => outcome === 'refusal' ? pending.reject(problem(403)) : pending.resolve(list()));
  expect(panel().getByText('取下げ記録')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: item().title })).not.toBeInTheDocument();
  h.api.listDocuments.mockResolvedValue(list()); await act(async () => h.history.back()); await screen.findByRole('button', { name: item(otherId).title });
  await act(async () => h.history.back()); await screen.findByRole('button', { name: item().title }); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('通常navigationと戻る/一覧reset後もUNKNOWN固定要求/upload Blob/Organization文脈を保持する', async () => {
  const h = setup('/documents?view=authoring&panel=closed'); await screen.findByRole('heading', { name: '編集作業' });
  const blob = new Blob(['fixed-body']);
  const working: WorkingOperation = { status: 'unknown', intent: { kind: 'create', documentId, sourceVersionId: versionId,
    body: { operationId: 'fixed-working', targetVersionId: 'fixed-target', expectedRevision: 7, title: '合成作業版', items: [] },
    files: new Map([['fixed-part', blob]]), prepared: { body: blob, contentType: 'multipart/form-data; boundary=fixed' } } };
  const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'fixed-metadata', expectedDocumentRevision: 7, set: { category: 'keep' }, unset: [], reason: '合成理由' } };
  h.client.setQueryData(workingOperationKey(documentId), working); metadataOperations(h.client).put(documentId, metadata);
  // First prove the non-Organization links keep the same app and fixed operations.
  fireEvent.click(screen.getByRole('link', { name: '文書履歴' })); await selectDocument(); await openContent(); await selectVersion();
  fireEvent.click(screen.getByRole('link', { name: '文書管理ホーム' })); await waitFor(() => expect(h.router.state.location.search.view).toBe('published'));
  await act(async () => h.history.back()); await panel().findByText('T10終了記録');
  fireEvent.click(screen.getByRole('button', { name: '合成文脈を保存' }));
  fireEvent.click(screen.getByRole('link', { name: '編集作業' })); await screen.findByRole('heading', { name: '編集作業' });
  fireEvent.click(screen.getByRole('link', { name: '文書履歴' })); await selectDocument(); await panel().findByText('T10終了記録'); await openContent(); await selectVersion();
  await act(async () => refreshFolderMoveReads(h.client));
  expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(working); expect((working.intent as WorkingWriteIntent).prepared.body).toBe(blob); expect((working.intent as WorkingWriteIntent).files.get('fixed-part')).toBe(blob);
  expect(metadataOperations(h.client).get(documentId)).toBe(metadata);
  const context = within(screen.getByRole('region', { name: '合成Organization保持' })); for (const value of ['合成主体', '保持する未保存文案', '未確定作業']) expect(context.getByText(value)).toBeVisible();
});

test('history行はendedと代表版の状態を別々に表示し表示日時を公開日時と呼ばない', async () => {
  setup('/documents?view=history'); await screen.findByRole('button', { name: item().title });
  const table = within(screen.getByRole('table', { name: '文書一覧' }));
  expect(table.getByRole('columnheader', { name: '表示日時' })).toBeVisible(); expect(table.queryByRole('columnheader', { name: '公開日時' })).not.toBeInTheDocument();
  const ended = within(screen.getByRole('button', { name: item().title }).closest('[role="row"]')! as HTMLElement);
  expect(ended.getByText('公開終了済み')).toBeVisible(); expect(ended.getByText('代表版: PUBLISHED')).toBeVisible();
  const withdrawn = within(screen.getByRole('button', { name: item(otherId).title }).closest('[role="row"]')! as HTMLElement);
  expect(withdrawn.getByText('公開終了していません')).toBeVisible(); expect(withdrawn.getByText('代表版: WITHDRAWN')).toBeVisible(); expect(withdrawn.queryByText('公開終了')).not.toBeInTheDocument();
});

test('refetchなしの一覧失効でも旧panelを隠して原本を止める', async () => {
  const h = setup('/documents?view=history'); await selectDocument(); await openContent(); await selectVersion();
  await act(async () => h.client.invalidateQueries({ queryKey: ['documents'], refetchType: 'none' }));
  expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument();
});

test('offlineでpausedの一覧再取得中も旧summary/旧版/原本を表示しない', async () => {
  const h = setup('/documents?view=history'); await selectDocument(); await openContent(); await selectVersion();
  try {
    act(() => { onlineManager.setOnline(false); void h.client.refetchQueries({ queryKey: ['documents'] }); });
    await waitFor(() => expect(h.client.getQueryCache().find({ queryKey: ['documents'], exact: false })?.state.fetchStatus).toBe('paused'));
    expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument(); expect(screen.queryByText('旧版の題名')).not.toBeInTheDocument();
  } finally { await act(async () => onlineManager.setOnline(true)); }
});

test('履歴panelを閉じたら元の明示選択行へfocusを戻す', async () => {
  setup('/documents?view=history'); await selectDocument(); await panel().findByText('T10終了記録');
  const close = panel().getByRole('button', { name: '履歴パネルを閉じる' }); close.focus(); fireEvent.click(close);
  await waitFor(() => expect(screen.queryByRole('region', { name: '選択した文書の履歴' })).not.toBeInTheDocument());
  expect(screen.getByRole('button', { name: item().title })).toHaveFocus();
});
