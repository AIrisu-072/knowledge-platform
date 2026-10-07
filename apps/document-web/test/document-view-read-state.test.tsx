import { installDocumentViewNavigation } from '../src/application/document-view-navigation';
import { StrictMode } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentDetail } from '../src/application/document-workspace';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: Object.fromEntries([
  'getSession', 'getRootFolder', 'listFolderChildren', 'listDocuments', 'getDocument', 'listDocumentVersions', 'getDocumentVersion',
  'listDocumentRevisions', 'getDocumentHistory', 'listVersionFiles', 'getDocumentAccessPolicy', 'compareDocumentRevisions', 'downloadVersionFile',
  'getCurrentDocumentVersionReadState', 'recordDocumentVersionView', 'resetDocumentVersionReadState',
].map(name => [name, jest.fn()])) }));

const documentId = '019a0107-0000-7000-8000-000000000010';
const versionId = '019a0107-0000-7000-8000-000000000011';
const olderVersionId = '019a0107-0000-7000-8000-000000000012';
const instant = '2026-10-07T00:00:00Z';
const available = { status: 'available' as const };
const disabled = { status: 'disabled' as const, reason: 'permission' as const };
const title = '再確認する公開文書';
const api = documentApi as unknown as Record<string, jest.Mock>;
type Snapshot = { documentId: string; versionId: string; firstReadAt: string | null; needsRecheck: boolean; readStateRevision: number; isRead: boolean };
type Body = { operationId: string; expectedReadStateRevision: number };
let authoritative: Snapshot;
function detail(): DocumentDetail {
  return { documentId, documentVersionId: versionId, title, folderId: null, folderName: null, revision: 7, metadata: { owning_department: '総務' }, createdAt: instant,
    currentVersionId: versionId, unread: !authoritative.isRead, publishedAt: instant, readState: { isRead: authoritative.isRead, firstReadAt: authoritative.firstReadAt }, displayRevision: null,
    displayTimestamp: { kind: 'revisionCreatedAt', value: instant },
    displayVersion: { versionId, versionNo: 1, baseVersionId: null, lifecycleState: 'PUBLISHED', isCurrent: true, approvedAt: instant, scheduledPublishAt: null, publishedAt: instant, withdrawnAt: null, updatedAt: instant,
      fileSummary: { authoritativeItemCount: 1, totalSizeBytes: 6, primary: { displayName: '公開原本.txt', mediaType: 'text/plain', sizeBytes: 6 } } },
    capabilities: { createVersion: disabled, updateMetadata: disabled, moveDocument: disabled, endPublication: disabled, manageAccess: disabled, compareVersions: available } };
}
function mutation(kind: 'VIEW' | 'RESET', doc: string, version: string, body: Body) {
  if (body.expectedReadStateRevision !== authoritative.readStateRevision) throw { type: 'about:blank', title: 'Synthetic', status: 409, code: 'REVISION_CONFLICT', traceId: 'synthetic', retryable: false };
  authoritative = { ...authoritative, firstReadAt: authoritative.firstReadAt ?? instant, needsRecheck: kind === 'RESET', isRead: kind === 'VIEW', readStateRevision: authoritative.readStateRevision + 1 };
  return { operationId: body.operationId, documentId: doc, versionId: version, kind, expectedReadStateRevision: body.expectedReadStateRevision, changed: true, occurredAt: instant,
    resultingReadState: { firstReadAt: authoritative.firstReadAt, needsRecheck: authoritative.needsRecheck, readStateRevision: authoritative.readStateRevision, isRead: authoritative.isRead } };
}
function mockApi(read = false) {
  Object.values(api).forEach(mock => mock.mockReset());
  authoritative = { documentId, versionId, firstReadAt: read ? instant : null, needsRecheck: false, readStateRevision: read ? 1 : 0, isRead: read };
  api.getRootFolder!.mockResolvedValue({ folderId: 'root', name: 'ルート', parentFolderId: null, revision: 1, capabilities: {} });
  api.listFolderChildren!.mockResolvedValue({ items: [], nextCursor: null, capabilities: {} });
  api.listDocuments!.mockImplementation(() => Promise.resolve({ view: 'published', items: [detail()], nextCursor: null }));
  api.getDocument!.mockImplementation(() => Promise.resolve(detail()));
  api.getDocumentVersion!.mockResolvedValue({ ...detail().displayVersion, lifecycleState: 'published', createdAt: instant, title, metadata: {}, firstReadAt: null,
    capabilities: { edit: disabled, rebase: disabled, publish: disabled, withdraw: disabled, schedulePublication: disabled, cancelPublicationSchedule: disabled, download: available } });
  api.listVersionFiles!.mockResolvedValue({ items: [{ contentItemId: 'primary', representationId: 'original', displayName: '公開原本.txt', mediaType: 'text/plain', sizeBytes: 6 }] });
  api.listDocumentVersions!.mockResolvedValue({ items: [], nextCursor: null });
  api.listDocumentRevisions!.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory!.mockResolvedValue({ items: [], nextCursor: null });
  api.getCurrentDocumentVersionReadState!.mockImplementation(() => Promise.resolve({ ...authoritative }));
  api.recordDocumentVersionView!.mockImplementation(async (doc, version, body) => mutation('VIEW', doc, version, body));
  api.resetDocumentVersionReadState!.mockImplementation(async (doc, version, body) => mutation('RESET', doc, version, body));
  api.downloadVersionFile!.mockResolvedValue(new Blob(['source']));
}
const clients: QueryClient[] = [];
const navigationStops: Array<() => void> = [];
function renderWorkspace(entry = '/documents?view=published') {
  const root = createRootRoute({ component: Outlet });
  const home = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const detailRoute = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別の画面</h1> });
  const router = createRouter({ routeTree: root.addChildren([home, detailRoute, tasks]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: 15_000, retry: 1, refetchOnWindowFocus: false } } });
  clients.push(client);
  navigationStops.push(installDocumentViewNavigation(router, client));
  const rendered = render(<StrictMode><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></StrictMode>);
  return { ...rendered, router, client };
}
async function shownOverview() {
  expect(await screen.findByRole('heading', { level: 1, name: title })).toBeVisible();
  expect(await screen.findByRole('heading', { name: '基本情報' })).toBeVisible();
  expect(screen.getByRole('heading', { name: 'その他の属性' })).toBeVisible();
  expect(await screen.findByText('総務')).toBeVisible();
  expect(await within(screen.getByRole('tabpanel')).findByText('公開原本.txt')).toBeVisible();
}
async function openFromHome() {
  fireEvent.click(await screen.findByRole('button', { name: /再確認する公開文書/ }));
  fireEvent.click(await screen.findByRole('button', { name: /詳細を開く/ }));
  await shownOverview();
}
const overviewUrl = `/documents/${documentId}?view=published&tab=overview`;
afterEach(() => { navigationStops.splice(0).forEach(stop => stop()); clients.splice(0).forEach(client => client.clear()); });

test('normal_home_to_detail_display_records_one_view', async () => {
  mockApi(); renderWorkspace(); await openFromHome();
  await waitFor(() => expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1));
  expect(api.recordDocumentVersionView!.mock.calls[0]).toEqual([documentId, versionId, { operationId: expect.stringMatching(/^[0-9a-f-]{14}7/), expectedReadStateRevision: 0 }]);
});
test('normally_displayed_read_document_offers_reset', async () => {
  mockApi(true); renderWorkspace(); await openFromHome();
  expect(await screen.findByRole('button', { name: '未読に戻す' })).toBeEnabled();
});
test.each(['invalidate', 'leave', 'tab_exit'] as const)('same_tick_%s_prevents_old_reset_handler', async event => {
  mockApi(true); const h = renderWorkspace(overviewUrl); await shownOverview();
  const reset = await screen.findByRole('button', { name: '未読に戻す' }); expect(reset).toBeEnabled();
  act(() => {
    if (event === 'invalidate') void h.client.invalidateQueries({ queryKey: ['document-current-read-state', documentId], refetchType: 'none' });
    else h.router.history.push(event === 'leave' ? '/documents' : `/documents/${documentId}?view=published&tab=history`);
    fireEvent.click(reset);
  });
  expect(api.resetDocumentVersionReadState).not.toHaveBeenCalled(); expect(authoritative).toMatchObject({ isRead: true, readStateRevision: 1 });
});
test.each([true, false])('css_ancestor_hidden_%s_checks_actual_visibility_before_view', async hidden => {
  mockApi(); const frames: FrameRequestCallback[] = [];
  const raf = jest.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length; });
  renderWorkspace(overviewUrl); await shownOverview();
  const stateRegion = await screen.findByRole('region', { name: '本人の既読状態' });
  await waitFor(() => expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(1));
  const ancestor = stateRegion.parentElement!.parentElement!;
  if (hidden) { ancestor.style.display = 'none'; expect(stateRegion).not.toBeVisible(); } else expect(stateRegion).toBeVisible();
  act(() => { frames.splice(0).forEach(callback => callback(performance.now())); });
  expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(hidden ? 0 : 1); raf.mockRestore();
});
test('query_reset_keeps_canonical_comparison_and_cancels_old_blob', async () => {
  mockApi();
  const revision = (id: string, version: string, major: number) => ({ revisionId: id, documentVersionId: version, major, minor: 0, label: `${major}.0`, createdAt: instant, sourceKind: major === 1 ? 'initialPublication' : 'contentPublication', metadataSnapshotStatus: 'complete' });
  api.listDocumentRevisions!.mockResolvedValue({ items: [revision('r2', versionId, 2), revision('r1', olderVersionId, 1)], nextCursor: null });
  const evidence = { documentId, versionId: olderVersionId, contentItemId: 'item', representationId: 'original', fileId: 'file', rawSha256: 'hash', inspectionProfile: 'dsi-v0', locator: { kind: 'textSpan', line: 1, byteStart: 0, byteEnd: 6 }, granularity: 'exact', parserProvenance: 'adapter' };
  api.compareDocumentRevisions!.mockResolvedValue({ projection: 'display', baseRevision: { revisionId: 'r1', documentVersionId: olderVersionId, major: 1, minor: 0, createdAt: instant }, targetRevision: { revisionId: 'r2', documentVersionId: versionId, major: 2, minor: 0, createdAt: instant },
    contentComparisonStatus: 'differentAuthoritativeVersions', verdict: 'unknown', coverage: 'partial', resultDigest: 'digest', changes: [], rows: [], unverifiedRegions: [{ reason: 'unsupportedSemanticConstruct', base: evidence, target: { ...evidence, versionId }, navigationHint: '原本を確認' }], ancillaryChanges: [], metadataComparisonStatus: 'unavailableLegacy', metadataChanges: [], baseMetadataSnapshotDigest: null, targetMetadataSnapshotDigest: null, auditEventId: 'audit', displayItems: [], pageSize: 50, nextCursor: null });
  const h = renderWorkspace(`/documents/${documentId}?view=published&tab=compare`);
  let resolveBlob!: (blob: Blob) => void; api.downloadVersionFile!.mockReturnValue(new Promise<Blob>(resolve => { resolveBlob = resolve; }));
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn() }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() });
  const save = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(await screen.findByRole('button', { name: '基準原本を確認' }));
  const signal = api.downloadVersionFile!.mock.calls[0]![1].signal as AbortSignal;
  await act(async () => { await refreshFolderMoveReads(h.client); resolveBlob(new Blob(['late'])); });
  expect(signal.aborted).toBe(true); expect(URL.createObjectURL).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled();
  await screen.findByRole('button', { name: '基準原本を確認' }); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});

async function currentBadge(expected: string, scope: ReturnType<typeof within> = screen) {
  const region = await scope.findByRole('region', { name: '本人の既読状態' });
  await waitFor(() => expect(within(region).getByRole('status')).toHaveTextContent(new RegExp(`^${expected}$`)));
}
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(r => { resolve = r; }); return { promise, resolve }; }
const problem = (status: number, code: string) => ({ type: 'about:blank', title: 'Synthetic', status, code, traceId: 'synthetic', retryable: false });
async function changeTab(h: ReturnType<typeof renderWorkspace>, tab: 'overview' | 'versions' | 'history' | 'compare') {
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: validateDetailSearch({ view: 'published', tab }) }); });
}
test('strict_remount_same_route_render_and_query_gc_do_not_consume_another_opening', async () => {
  mockApi(); const h = renderWorkspace(overviewUrl); await currentBadge('既読'); const first = api.recordDocumentVersionView!.mock.calls[0];
  h.unmount(); render(<StrictMode><QueryClientProvider client={h.client}><RouterProvider router={h.router as never} /></QueryClientProvider></StrictMode>);
  await shownOverview(); await act(async () => { await h.router.load(); h.client.removeQueries({ queryKey: ['document-read-opening'] }); });
  expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1); expect(api.recordDocumentVersionView!.mock.calls[0]).toEqual(first);
});
test('reset_survives_normal_refetch_reset_query_tabs_and_explicit_state_read', async () => {
  mockApi(true); const h = renderWorkspace(overviewUrl); await currentBadge('既読'); fireEvent.click(screen.getByRole('button', { name: '未読に戻す' })); await currentBadge('未読');
  const fixed = api.resetDocumentVersionReadState!.mock.calls[0]![2];
  await act(async () => { await h.client.refetchQueries({ queryKey: ['document', documentId] }); await h.client.resetQueries({ queryKey: ['document-version-files', documentId] }); });
  await changeTab(h, 'history'); await changeTab(h, 'overview'); await shownOverview();
  fireEvent.click(within(screen.getByRole('region', { name: '本人の既読状態' })).getByRole('button', { name: '現在の既読状態を再取得' })); await currentBadge('未読');
  expect(api.recordDocumentVersionView).not.toHaveBeenCalled(); expect(api.resetDocumentVersionReadState).toHaveBeenCalledTimes(1); expect(fixed.expectedReadStateRevision).toBe(1);
});
test('leave_and_return_within_fifteen_seconds_fetches_new_token_and_keeps_body_fixed', async () => {
  mockApi(true); const h = renderWorkspace(overviewUrl); await currentBadge('既読'); fireEvent.click(screen.getByRole('button', { name: '未読に戻す' })); await currentBadge('未読');
  const reads = api.getCurrentDocumentVersionReadState!.mock.calls.length;
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ })); await openFromHome(); await currentBadge('既読');
  expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1); expect(api.recordDocumentVersionView!.mock.calls[0]![2].expectedReadStateRevision).toBe(2); expect(api.getCurrentDocumentVersionReadState!.mock.calls.length).toBe(reads + 2);
});
test('reload_with_new_query_client_creates_a_fresh_intentional_display', async () => {
  mockApi(true); const h = renderWorkspace(overviewUrl); await currentBadge('既読'); fireEvent.click(screen.getByRole('button', { name: '未読に戻す' })); await currentBadge('未読');
  h.unmount(); renderWorkspace(overviewUrl); await currentBadge('既読'); expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1); expect(api.recordDocumentVersionView!.mock.calls[0]![2].expectedReadStateRevision).toBe(2);
});
test.each(['history', 'versions', 'compare'] as const)('direct_%s_then_overview_is_not_a_new_display_entry', async tab => {
  mockApi(); const h = renderWorkspace(`/documents/${documentId}?view=published&tab=${tab}`); await screen.findByRole('heading', { level: 1, name: tab === 'compare' ? '新旧比較' : title });
  await changeTab(h, 'overview'); await shownOverview(); expect(api.getCurrentDocumentVersionReadState).not.toHaveBeenCalled(); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test('unrelated_route_and_list_prefetch_panel_do_not_record', async () => {
  mockApi(); const h = renderWorkspace('/tasks?documentId=doc&view=published&tab=overview'); await screen.findByRole('heading', { name: '別の画面' });
  expect(api.getCurrentDocumentVersionReadState).not.toHaveBeenCalled();
  await act(async () => { await h.router.navigate({ to: '/documents', search: validateListSearch({}) }); });
  fireEvent.click(await screen.findByRole('button', { name: /再確認する公開文書/ })); await screen.findByRole('complementary', { name: '選択中の文書' });
  await act(async () => { await h.client.prefetchQuery({ queryKey: ['document', documentId, 'published'], queryFn: () => documentApi.getDocument(documentId, 'published') }); });
  expect(api.getCurrentDocumentVersionReadState).not.toHaveBeenCalled(); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test('slow_initial_document_and_files_still_produce_one_view_after_actual_display', async () => {
  mockApi(); const documentRead = deferred<DocumentDetail>(); const files = deferred<{ items: Array<{ contentItemId: string; representationId: string; displayName: string; mediaType: string; sizeBytes: number }> }>();
  api.getDocument!.mockReturnValue(documentRead.promise); api.listVersionFiles!.mockReturnValue(files.promise); renderWorkspace(overviewUrl);
  await waitFor(() => expect(api.getDocument).toHaveBeenCalled()); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
  await act(async () => { documentRead.resolve(detail()); }); await waitFor(() => expect(api.listVersionFiles).toHaveBeenCalled());
  expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
  await act(async () => { files.resolve({ items: [{ contentItemId: 'primary', representationId: 'original', displayName: '公開原本.txt', mediaType: 'text/plain', sizeBytes: 6 }] }); });
  await shownOverview(); await currentBadge('既読'); expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1);
});
test('hidden_document_and_original_download_are_not_display_events', async () => {
  mockApi(); Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn().mockReturnValue('blob:source') }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  renderWorkspace(overviewUrl); fireEvent.click(await screen.findByRole('button', { name: '現行ファイルを取得' })); await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalledTimes(1)); expect(api.recordDocumentVersionView).not.toHaveBeenCalled(); Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
});
test.each(['invalidate', 'reset', 'remove'] as const)('late_token_after_%s_never_records', async action => {
  mockApi(); const token = deferred<Snapshot>(); api.getCurrentDocumentVersionReadState!.mockReturnValue(token.promise); const h = renderWorkspace(overviewUrl);
  await waitFor(() => expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(1)); const signal = api.getCurrentDocumentVersionReadState!.mock.calls[0]![2].signal as AbortSignal;
  await act(async () => { if (action === 'invalidate') await h.client.invalidateQueries({ queryKey: ['document', documentId] }); else if (action === 'reset') await h.client.resetQueries({ queryKey: ['document', documentId] }); else h.client.removeQueries({ queryKey: ['document', documentId] }); token.resolve({ ...authoritative }); });
  expect(signal.aborted).toBe(true); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test('same_tick_new_version_in_canonical_cache_cannot_retarget_or_send_old_view', async () => {
  mockApi(); const frames: FrameRequestCallback[] = []; const raf = jest.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length; });
  const h = renderWorkspace(overviewUrl); await shownOverview(); await currentBadge('未読');
  act(() => { const next = { ...detail(), currentVersionId: 'new-version', displayVersion: { ...detail().displayVersion, versionId: 'new-version', versionNo: 2 } }; h.client.setQueryData(['document', documentId, 'published'], next); frames.splice(0).forEach(callback => callback(performance.now())); });
  expect(api.recordDocumentVersionView).not.toHaveBeenCalled(); expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(1); raf.mockRestore();
});
test('state403_keeps_normal_overview_history_and_download_and_blocks_agent_mutation', async () => {
  mockApi(); api.getCurrentDocumentVersionReadState!.mockRejectedValue(problem(403, 'FORBIDDEN')); const h = renderWorkspace(overviewUrl);
  await shownOverview(); await screen.findByText('この操作を行う権限がありません。'); const normalReads = api.getDocument!.mock.calls.length;
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn().mockReturnValue('blob:source') }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(screen.getByRole('button', { name: '現行ファイルを取得' })); await waitFor(() => expect(URL.createObjectURL).toHaveBeenCalledTimes(1));
  await changeTab(h, 'history'); await waitFor(() => expect(api.getDocumentHistory).toHaveBeenCalledTimes(1));
  expect(api.getDocument).toHaveBeenCalledTimes(normalReads); expect(api.recordDocumentVersionView).not.toHaveBeenCalled(); expect(screen.queryByRole('button', { name: '未読に戻す' })).not.toBeInTheDocument();
});
test.each([401, 404, 409])('state%i_rechecks_normal_document_once_without_reopening', async status => {
  mockApi(); let before = 0;
  api.getCurrentDocumentVersionReadState!.mockImplementation(async () => { before = api.getDocument!.mock.calls.length; throw problem(status, status === 401 ? 'AUTHENTICATION_REQUIRED' : status === 404 ? 'DOCUMENT_NOT_FOUND' : 'STALE_VERSION'); }); renderWorkspace(overviewUrl);
  await screen.findByRole('heading', { name: '基本情報' }); await waitFor(() => expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(1)); await waitFor(() => expect(api.getDocument).toHaveBeenCalledTimes(before + 1));
  expect(screen.getByRole('heading', { name: '基本情報' })).toBeVisible(); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test.each([401, 403, 404])('normal_document_%i_recheck_uses_its_existing_denial_barrier', async status => {
  mockApi(); let before = 0;
  api.getCurrentDocumentVersionReadState!.mockImplementation(async () => { before = api.getDocument!.mock.calls.length; api.getDocument!.mockRejectedValue(problem(status, status === 401 ? 'AUTHENTICATION_REQUIRED' : status === 403 ? 'FORBIDDEN' : 'DOCUMENT_NOT_FOUND')); throw problem(404, 'DOCUMENT_NOT_FOUND'); });
  const h = renderWorkspace(overviewUrl);
  await waitFor(() => expect(h.client.getQueryData(['document-history-refusal', documentId])).toBeTruthy());
  await waitFor(() => expect(screen.queryByRole('heading', { name: '基本情報' })).not.toBeInTheDocument());
  expect(api.getDocument).toHaveBeenCalledTimes(before + 1);
  expect(screen.getByRole('heading', { name: status === 401 ? 'ログインが必要です' : status === 403 ? 'アクセスできません' : '見つかりません' })).toBeVisible(); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test.each(['transport', 'payload'] as const)('auxiliary_%s_failure_does_not_deny_normal_reads', async failure => {
  mockApi(); let before = 0;
  api.getCurrentDocumentVersionReadState!.mockImplementation(async () => { before = api.getDocument!.mock.calls.length; if (failure === 'transport') throw new Error('network'); return { ...authoritative, isRead: true }; }); renderWorkspace(overviewUrl);
  await shownOverview(); await screen.findByText('現在の既読状態を確認できません。'); expect(api.getDocument).toHaveBeenCalledTimes(before); expect(api.recordDocumentVersionView).not.toHaveBeenCalled();
});
test.each([true, false])('maximum_revision_prevents_new_mutation_for_read_%s', async read => {
  mockApi(true); authoritative = { ...authoritative, readStateRevision: Number.MAX_SAFE_INTEGER, isRead: read, needsRecheck: !read }; renderWorkspace(overviewUrl);
  await currentBadge(read ? '既読' : '未読'); await screen.findByText('既読状態を更新できません。管理者に確認してください');
  if (read) expect(screen.getByRole('button', { name: '未読に戻す' })).toBeDisabled(); expect(api.recordDocumentVersionView).not.toHaveBeenCalled(); expect(api.resetDocumentVersionReadState).not.toHaveBeenCalled();
});
test('unknown_preserves_exact_intent_on_home_and_after_new_version_or_read_denial', async () => {
  mockApi(); api.recordDocumentVersionView!.mockRejectedValue(new Error('response lost')); const h = renderWorkspace(overviewUrl);
  await screen.findByText('結果を確認できません。同じ操作を再試行できます'); const fixed = api.recordDocumentVersionView!.mock.calls[0]!;
  api.getDocument!.mockRejectedValue(problem(404, 'DOCUMENT_NOT_FOUND'));
  await act(async () => { await h.client.fetchQuery({ queryKey: ['document', documentId, 'published'], staleTime: 0, retry: false }).catch(() => undefined); });
  expect(screen.getByRole('button', { name: '同じ操作を再試行' })).toBeEnabled();
  api.getDocument!.mockResolvedValue({ ...detail(), currentVersionId: 'new-version', displayVersion: { ...detail().displayVersion, versionId: 'new-version', versionNo: 2 } });
  await act(async () => { await h.client.fetchQuery({ queryKey: ['document', documentId, 'published'], staleTime: 0, retry: false }); });
  await screen.findAllByText('Version 2');
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ })); await screen.findByRole('heading', { name: '文書一覧' }); fireEvent.click(screen.getByRole('button', { name: '同じ操作を再試行' }));
  await waitFor(() => expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(2)); expect(api.recordDocumentVersionView!.mock.calls[1]).toEqual(fixed); expect(api.resetDocumentVersionReadState).not.toHaveBeenCalled();
});
test('old_successful_view_receipt_after_external_reset_never_becomes_current_badge', async () => {
  mockApi(); let saved!: ReturnType<typeof mutation>;
  api.recordDocumentVersionView!.mockImplementationOnce(async (doc, version, body) => { saved = mutation('VIEW', doc, version, body); throw new Error('lost after commit'); }).mockImplementationOnce(async () => saved);
  renderWorkspace(overviewUrl); await screen.findByText('結果を確認できません。同じ操作を再試行できます'); authoritative = { ...authoritative, needsRecheck: true, isRead: false, readStateRevision: 2 };
  fireEvent.click(screen.getByRole('button', { name: '同じ操作を再試行' })); await currentBadge('未読'); expect(authoritative.readStateRevision).toBe(2); expect(api.recordDocumentVersionView!.mock.calls[1]).toEqual(api.recordDocumentVersionView!.mock.calls[0]);
});
test('unknown_then_forbidden_retains_unknown_and_does_not_offer_a_new_reset', async () => {
  mockApi(); api.recordDocumentVersionView!.mockRejectedValueOnce(new Error('lost')).mockRejectedValue(problem(403, 'FORBIDDEN')); renderWorkspace(overviewUrl);
  await screen.findByText('結果を確認できません。同じ操作を再試行できます'); fireEvent.click(screen.getByRole('button', { name: '同じ操作を再試行' })); await waitFor(() => expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(2));
  await screen.findByRole('button', { name: '同じ操作を再試行' }); expect(api.resetDocumentVersionReadState).not.toHaveBeenCalled();
});
test('confirmed_success_with_failed_current_get_never_retries_the_mutation', async () => {
  mockApi(); api.getCurrentDocumentVersionReadState!.mockResolvedValueOnce({ ...authoritative }).mockRejectedValue(new Error('current read failed')); renderWorkspace(overviewUrl);
  await screen.findByText('処理結果は確認済みですが、現在の既読状態を取得できません'); await currentBadge('既読状態は未確認です');
  expect(screen.queryByRole('button', { name: '同じ操作を再試行' })).not.toBeInTheDocument(); const result = screen.getByRole('region', { name: '既読状態の操作結果' }); fireEvent.click(within(result).getByRole('button', { name: '現在の既読状態を再取得' }));
  await waitFor(() => expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(3)); expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1);
});
test('other_tab_old_unread_token_cannot_undo_reset', async () => {
  mockApi(); const files = deferred<{ items: Array<{ contentItemId: string; representationId: string; displayName: string; mediaType: string; sizeBytes: number }> }>(); api.listVersionFiles!.mockReturnValueOnce(files.promise);
  const oldTab = renderWorkspace(overviewUrl); await waitFor(() => expect(api.getCurrentDocumentVersionReadState).toHaveBeenCalledTimes(1));
  const newTab = renderWorkspace(overviewUrl); const newScope = within(newTab.container); await currentBadge('既読', newScope);
  fireEvent.click(newScope.getByRole('button', { name: '未読に戻す' })); await currentBadge('未読', newScope);
  await act(async () => { files.resolve({ items: [{ contentItemId: 'primary', representationId: 'original', displayName: '公開原本.txt', mediaType: 'text/plain', sizeBytes: 6 }] }); });
  await waitFor(() => expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(2)); expect(api.recordDocumentVersionView!.mock.calls[1]![2].expectedReadStateRevision).toBe(0); expect(authoritative).toMatchObject({ isRead: false, readStateRevision: 2 });
  expect(within(oldTab.container).queryByRole('button', { name: '同じ操作を再試行' })).not.toBeInTheDocument(); await currentBadge('未読', newScope);
});


test('recovery_current_refresh_before_queued_display_revokes_the_opening', async () => {
  mockApi();
  api.getCurrentDocumentVersionReadState!.mockResolvedValueOnce({ ...authoritative }).mockRejectedValueOnce(new Error('read failed')).mockImplementation(() => Promise.resolve({ ...authoritative }));
  const h = renderWorkspace(overviewUrl);
  await screen.findByText('処理結果は確認済みですが、現在の既読状態を取得できません');
  expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1);
  authoritative = { ...authoritative, needsRecheck: true, isRead: false, readStateRevision: 2 };
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  await screen.findByRole('heading', { name: '文書一覧' });
  const frames: FrameRequestCallback[] = [];
  const raf = jest.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length; });
  await openFromHome(); await currentBadge('未読');
  const refresh = deferred<Snapshot>();
  api.getCurrentDocumentVersionReadState!.mockReturnValue(refresh.promise);
  const recovery = screen.getByRole('region', { name: '既読状態の操作結果' });
  fireEvent.click(within(recovery).getByRole('button', { name: '現在の既読状態を再取得' }));
  await waitFor(() => expect(h.client.getQueryState(['document-current-read-state', documentId, versionId])?.fetchStatus).toBe('fetching'));
  const { documentViewNavigation } = require('../src/application/document-view-navigation');
  expect(documentViewNavigation(h.client).current(documentId, versionId)).toBeUndefined();
  act(() => { frames.splice(0).forEach(callback => callback(performance.now())); });
  const posts = api.recordDocumentVersionView!.mock.calls.length;
  await act(async () => { refresh.resolve({ ...authoritative }); });
  await currentBadge('未読');
  act(() => { frames.splice(0).forEach(callback => callback(performance.now())); });
  raf.mockRestore();
  expect(posts).toBe(1);
  expect(api.recordDocumentVersionView).toHaveBeenCalledTimes(1);
});
