import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { documentApi, type DocumentDetail, type VersionDetail } from '../src/application/document-workspace';
import type { DiffDisplayProjection } from '@knowledge-platform/document-api-client';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';
import { refreshLifecycleQueries } from '../src/application/document-lifecycle-operations';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';
import { workingOperationKey, type WorkingOperation, type WorkingWriteIntent } from '../src/application/document-working-version';
import { metadataOperations, type MetadataOperation } from '../src/application/document-metadata';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getDocument: jest.fn(), getRootFolder: jest.fn(), listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(),
  listDocumentRevisions: jest.fn(), listVersionFiles: jest.fn(), compareDocumentVersions: jest.fn(), compareDocumentRevisions: jest.fn(),
  publishVersion: jest.fn(), updateWorkingVersion: jest.fn(), downloadVersionFile: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const available = { status: 'available' as const }, denied = { status: 'disabled' as const, reason: 'permission' as const };
function version(n: number): VersionDetail { return { versionId: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`, versionNo: n, baseVersionId: null, lifecycleState: n === 3 ? 'working' : 'published', isCurrent: n === 2,
  createdAt: '2026-10-01T00:00:00Z', updatedAt: '2026-10-02T00:00:00Z', approvedAt: null, scheduledPublishAt: null, publishedAt: n === 3 ? null : '2026-10-01T00:00:00Z', withdrawnAt: null,
  firstReadAt: null, title: n === 3 ? '作業版の題名' : '現行公開版の題名', metadata: {}, currentPublicationScheduleId: null, fileSummary: { authoritativeItemCount: 1, totalSizeBytes: 20, primary: null },
  capabilities: { edit: available, rebase: denied, publish: available, withdraw: denied, schedulePublication: denied, cancelPublicationSchedule: denied, download: available } }; }
const published = version(2), working = version(3);
function detail(id = documentId): DocumentDetail { return { documentId: id, documentVersionId: working.versionId, title: '現在の文書題名', folderId: null, folderName: null, revision: 7, metadata: {}, createdAt: working.createdAt, currentVersionId: published.versionId, unread: false, publishedAt: published.publishedAt!,
  displayVersion: { ...working, lifecycleState: 'WORKING' }, displayRevision: null, readState: { isRead: false, firstReadAt: null }, displayTimestamp: { kind: 'workingUpdatedAt', value: working.updatedAt },
  capabilities: { createVersion: available, updateMetadata: available, moveDocument: denied, endPublication: denied, manageAccess: denied, compareVersions: available } }; }
function item(n: number): DiffDisplayProjection['items'][number] { return { changeIndex: n, facet: 'body', operation: 'modified', relocation: null, baseLocator: null, targetLocator: null,
  base: { kind: 'text', text: `旧本文${n}`, truncated: false, locator: { kind: 'textSpan', line: n + 1, byteStart: 0, byteEnd: 10 } }, target: { kind: 'text', text: `新本文${n}`, truncated: false, locator: { kind: 'textSpan', line: n + 1, byteStart: 0, byteEnd: 10 } } }; }
function comparison(indices = [0], nextCursor: string | null = 'next'): DiffDisplayProjection { return { projection: 'display', verdict: 'unknown', coverage: 'partial', resultDigest: 'digest', items: indices.map(item), unverifiedRegions: [], pageSize: 50, nextCursor, auditEventId: 'audit', resultAuditEventId: 'result-audit' }; }
const problem = (code: string, status: number, retryable = false) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => { metadataOperations(client).clear(documentId); client.clear(); }));
function setup(retry: boolean | number = false, gcTime = Infinity) {
  const api = documentApi as unknown as { [K in keyof typeof documentApi]: jest.Mock }; Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation(id => Promise.resolve(detail(id))); api.listDocumentVersions.mockResolvedValue({ items: [working], nextCursor: null });
  api.getDocumentVersion.mockImplementation((_id, id) => Promise.resolve(id === published.versionId ? published : working)); api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null }); api.listVersionFiles.mockResolvedValue({ items: [] });
  api.compareDocumentVersions.mockImplementation((_id, body) => Promise.resolve(comparison(body.cursor ? [1] : [0], body.cursor ? null : 'next')));
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=authoring&tab=versions&versionId=${working.versionId}`] });
  const router = createRouter({ routeTree: root.addChildren([route, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry, retryDelay: 0, staleTime: 15_000, gcTime } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>); return { api, router, client, history };
}
const entry = () => screen.getByRole('button', { name: '現行公開版とこの作業版を比較' });
const result = () => within(screen.getByRole('region', { name: '公開前の内容比較' }));
const more = () => result().getByRole('button', { name: '内容比較をさらに表示' });
const restart = () => result().getByRole('button', { name: '内容比較を最初から読み直す' });
const close = () => result().getByRole('button', { name: '内容比較を閉じる' });
const fixed = { baseVersionId: published.versionId, targetVersionId: working.versionId, profile: 'document-diff-v0', projection: 'display', pageSize: 50 };
async function open() { fireEvent.click(await screen.findByRole('button', { name: '現行公開版とこの作業版を比較' })); }
async function loaded() { await open(); await screen.findByText('新本文0'); }

// Version displayにはRevision snapshotがない。入口/固定pair/型を間違えるとこの実routeが失敗する。
test('選択WORKINGから公開側exact ID/published詳細を読み、版番号・題名・固定IDと内容判定だけを表示する', async () => {
  const h = setup(); await waitFor(() => expect(entry()).toBeEnabled());
  expect(screen.getByRole('heading', { name: '選択中: WORKING · 版 3' })).toBeVisible();
  expect(h.api.compareDocumentVersions).not.toHaveBeenCalled(); await loaded();
  expect(h.api.getDocumentVersion).toHaveBeenCalledWith(documentId, published.versionId, 'published'); expect(h.api.getDocumentVersion).toHaveBeenCalledWith(documentId, working.versionId, 'authoring');
  expect(h.api.compareDocumentVersions).toHaveBeenCalledWith(documentId, fixed); expect(h.api.compareDocumentRevisions).not.toHaveBeenCalled();
  expect(result().getByText('Version 2 · 現行公開版の題名')).toBeVisible(); expect(result().getByText('WORKING · Version 3 · 作業版の題名')).toBeVisible();
  expect(result().getByText(published.versionId)).toBeVisible(); expect(result().getByText(working.versionId)).toBeVisible(); expect(result().getByText('不明')).toBeVisible(); expect(result().getByText('一部のみ')).toBeVisible();
  expect(result().queryByText('メタデータ', { selector: 'dt' })).not.toBeInTheDocument(); expect(result().queryByRole('button', { name: /原本/ })).not.toBeInTheDocument();
  expect(h.router.state.location.search).toMatchObject({ tab: 'versions', view: 'authoring', versionId: working.versionId }); expect(h.api.publishVersion).not.toHaveBeenCalled();
});

test.each([null, undefined])('50+1を同pair/digestで継続し監査IDと終端%sを受け入れる', async terminal => {
  const h = setup(); const last = comparison([50], null); if (terminal === undefined) delete (last as Partial<DiffDisplayProjection>).nextCursor;
  h.api.compareDocumentVersions.mockImplementation((_id, body) => Promise.resolve(body.cursor ? { ...last, auditEventId: 'next', resultAuditEventId: 'next-result' } : comparison(Array.from({ length: 50 }, (_, i) => i), 'opaque+/=?日本語')));
  await loaded(); expect(result().getAllByText(/^新本文\d+$/)).toHaveLength(50); fireEvent.click(more()); await screen.findByText('新本文50'); expect(result().getAllByText(/^新本文\d+$/)).toHaveLength(51);
  expect(h.api.compareDocumentVersions.mock.calls).toEqual([[documentId, fixed], [documentId, { ...fixed, cursor: 'opaque+/=?日本語' }]]); expect(result().queryByRole('button', { name: '内容比較をさらに表示' })).not.toBeInTheDocument();
});

test.each(['no current', 'not working', 'disabled', 'hidden', 'missing'])('%sではWrite/Publishから比較能力を補完しない', async mode => {
  const h = setup(); if (mode === 'not working') { h.api.listDocumentVersions.mockResolvedValue({ items: [published], nextCursor: null }); }
  else h.api.getDocument.mockResolvedValue({ ...detail(), ...(mode === 'no current' ? { currentVersionId: null } : { capabilities: { ...detail().capabilities, compareVersions: mode === 'missing' ? undefined : { status: mode, reason: 'permission' } } }) });
  await screen.findByRole('heading', { name: /選択中:/ }); expect(screen.queryByRole('button', { name: '現行公開版とこの作業版を比較' })).not.toBeInTheDocument(); expect(h.api.compareDocumentVersions).not.toHaveBeenCalled();
});

test('空のitemsでもunknown/partialと未比較範囲だけのページを同一と断定しない', async () => {
  const h = setup(); h.api.compareDocumentVersions.mockImplementation((_id, body) => Promise.resolve({ ...comparison([], body.cursor ? null : 'next'), unverifiedRegions: body.cursor ? [{ reason: 'resourceLimit', base: null, target: null, navigationHint: '未検証の末尾' }] : [] }));
  await open(); await result().findByText('不明'); expect(result().getByText('一部のみ')).toBeVisible(); expect(result().getByText(/続きの比較結果/)).toBeVisible(); expect(result().queryByText('同一')).not.toBeInTheDocument();
  fireEvent.click(more()); await result().findByText('未検証の末尾'); expect(result().getByText('比較上限に達しました')).toBeVisible(); expect(result().queryByRole('button', { name: /原本/ })).not.toBeInTheDocument();
});

test.each([new TypeError('offline'), problem('DEPENDENCY_UNAVAILABLE', 503, true)])('一時的な比較tail障害だけ既取得を保持し同cursorを一度ずつ再送する: %p', async failure => {
  const h = setup(1); const pending = deferred<DiffDisplayProjection>(); let count = 0;
  h.api.compareDocumentVersions.mockImplementation((_id, body) => !body.cursor ? Promise.resolve(comparison()) : ++count === 1 ? Promise.reject(failure) : pending.promise);
  await loaded(); fireEvent.click(more()); await result().findByRole('alert'); expect(result().getByText('新本文0')).toBeVisible(); expect(count).toBe(1);
  const retry = result().getByRole('button', { name: '内容比較の続きを再試行' }); act(() => { retry.click(); retry.click(); }); await waitFor(() => expect(count).toBe(2));
  await act(async () => pending.resolve(comparison([1], null))); await screen.findByText('新本文1'); expect(h.api.compareDocumentVersions.mock.calls.map(([, body]) => body.cursor)).toEqual([undefined, 'next', 'next']);
});

test.each([['AUTHENTICATION_REQUIRED', 401], ['FORBIDDEN', 403], ['DOCUMENT_VERSION_NOT_FOUND', 404], ['STALE_COMPARISON_INPUT', 409], ['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422], ['INTERNAL', 500]])('%sで旧結果を捨てclose/reopen/resetでも停止し明示再読取だけ復帰する', async (code, status) => {
  const h = setup(1); await loaded(); h.api.compareDocumentVersions.mockRejectedValueOnce(problem(code, Number(status))); fireEvent.click(more()); await result().findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(result().queryByRole('button', { name: '内容比較をさらに表示' })).not.toBeInTheDocument();
  const count = h.api.compareDocumentVersions.mock.calls.length; fireEvent.click(close()); await open(); await result().findByRole('alert'); await act(async () => refreshFolderMoveReads(h.client)); expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(count);
  h.api.compareDocumentVersions.mockResolvedValue(comparison([9], null)); fireEvent.click(restart()); await screen.findByText('新本文9'); expect(h.api.compareDocumentVersions).toHaveBeenLastCalledWith(documentId, fixed);
});

test.each(['projection', 'pageSize', 'verdict', 'coverage', 'resultDigest'])('tailの%s不一致は旧headerと新itemsを結合しない', async field => {
  const h = setup(); const bad = { ...comparison([1], null), [field]: ({ projection: 'diff', pageSize: 100, verdict: 'same', coverage: 'full', resultDigest: 'changed' } as Record<string, unknown>)[field] };
  h.api.compareDocumentVersions.mockImplementation((_id, body) => Promise.resolve(body.cursor ? bad : comparison())); await loaded(); fireEvent.click(more()); await result().findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
});

test.each(['projection', 'pageSize'])('初回%sの不一致をdisplayとして表示しない', async field => {
  const h = setup(); h.api.compareDocumentVersions.mockResolvedValue({ ...comparison(), [field]: field === 'projection' ? 'diff' : 100 }); await open(); await result().findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument();
});

test.each(['current', 'revision', 'working', 'capability', 'published', 'denied'])('比較応答後の正規readで%s変更なら内容を開示しない', async kind => {
  const h = setup(); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockReturnValue(pending.promise); await open(); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(1));
  if (kind === 'current') h.api.getDocument.mockResolvedValue({ ...detail(), currentVersionId: version(1).versionId });
  if (kind === 'revision') h.api.getDocument.mockResolvedValue({ ...detail(), revision: 8 });
  if (kind === 'capability') h.api.getDocument.mockResolvedValue({ ...detail(), capabilities: { ...detail().capabilities, compareVersions: denied } });
  if (kind === 'working' || kind === 'published') h.api.getDocumentVersion.mockImplementation((_id, id) => Promise.resolve(id === working.versionId ? { ...working, ...(kind === 'working' ? { updatedAt: '2026-10-03T00:00:00Z' } : {}) } : { ...published, ...(kind === 'published' ? { isCurrent: false } : {}) }));
  if (kind === 'denied') h.api.getDocument.mockRejectedValue(problem('FORBIDDEN', 403));
  await act(async () => pending.resolve(comparison())); await result().findByRole('alert'); expect(screen.queryByText('新本文0')).not.toBeInTheDocument();
});

test.each(['document', 'working', 'versions', 'published', 'policy'])('背景%s read開始で旧結果を隠し遅いtailも破棄する', async source => {
  const h = setup(); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve(comparison())); await loaded(); fireEvent.click(more()); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(2));
  const key = source === 'document' ? ['document', documentId, 'authoring'] : source === 'versions' ? ['document-versions', documentId, 'authoring'] : source === 'policy' ? ['document-access-policy', documentId] : ['document-version', documentId, source === 'working' ? working.versionId : published.versionId, source === 'working' ? 'authoring' : 'published'];
  act(() => { h.client.setQueryData(key, source === 'document' ? detail() : source === 'versions' ? { items: [working], nextCursor: null } : source === 'policy' ? { policyRevision: 1 } : source === 'working' ? working : published); });
  await waitFor(() => expect(screen.queryByText('新本文0')).not.toBeInTheDocument()); await act(async () => pending.resolve(comparison([1], null))); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
});

test.each(['fetching', 'paused', 'invalidated', 'error'])('現在文書readが%sなら表示を失効し再読取まで再開しない', async state => {
  const h = setup(); await loaded(); const q = h.client.getQueryCache().find<unknown, unknown>({ queryKey: ['document', documentId, 'authoring'], exact: true })!;
  act(() => q.setState(state === 'invalidated' ? { isInvalidated: true } : state === 'error' ? { status: 'error', error: problem('FORBIDDEN', 403) } : { fetchStatus: state as 'fetching' | 'paused' }));
  await waitFor(() => expect(screen.queryByText('新本文0')).not.toBeInTheDocument()); expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(1);
});

test.each(['close', 'tab', 'document', 'selection', 'route'])('%s後の遅い成功を再表示しない', async kind => {
  const h = setup(); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockReturnValue(pending.promise); await open(); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(1));
  if (kind === 'close') fireEvent.click(close());
  else if (kind === 'route') await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  else await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: kind === 'document' ? otherId : documentId }, search: { view: 'authoring', tab: kind === 'tab' ? 'overview' : 'versions', versionId: kind === 'selection' ? version(4).versionId : working.versionId } }); });
  await waitFor(() => expect(screen.queryByRole('region', { name: '公開前の内容比較' })).not.toBeInTheDocument());
  await act(async () => pending.resolve(comparison())); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(screen.queryByRole('region', { name: '公開前の内容比較' })).not.toBeInTheDocument();
});

test.each(['lifecycle', 'folder move', 'working', 'metadata', 'ACL'])('既存%s read resetは比較を失効しUNKNOWN/Blob/Organizationを保持する', async kind => {
  const h = setup(); await loaded(); const blob = new Blob(['immutable']); const fixedOperation: WorkingOperation = { status: 'unknown', intent: { kind: 'create', documentId, sourceVersionId: published.versionId, body: { operationId: 'fixed', targetVersionId: working.versionId, expectedRevision: 7, title: '保持', items: [] }, files: new Map([['part', blob]]), prepared: { body: blob, contentType: 'multipart/form-data; boundary=fixed' } } };
  const metadata: MetadataOperation = { status: 'unknown', request: { operationId: 'meta', expectedDocumentRevision: 7, set: {}, unset: [], reason: '保持' } };
  act(() => { h.client.setQueryData(workingOperationKey(documentId), fixedOperation); metadataOperations(h.client).put(documentId, metadata); h.client.setQueryData(['organization-session'], { principal: 'keep' }); });
  await act(async () => { if (kind === 'folder move' || kind === 'ACL') await refreshFolderMoveReads(h.client); else if (kind === 'lifecycle' || kind === 'working') await refreshLifecycleQueries(h.client, documentId); else await h.client.invalidateQueries({ queryKey: ['document', documentId] }); });
  expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); fireEvent.click(restart()); await screen.findByText('新本文0');
  expect(h.client.getQueryData(workingOperationKey(documentId))).toBe(fixedOperation); expect((h.client.getQueryData(workingOperationKey(documentId)) as { intent: WorkingWriteIntent }).intent.prepared.body).toBe(blob); expect(metadataOperations(h.client).get(documentId)).toBe(metadata); expect(h.client.getQueryData(['organization-session'])).toEqual({ principal: 'keep' });
  expect(h.api.updateWorkingVersion).not.toHaveBeenCalled(); expect(h.api.publishVersion).not.toHaveBeenCalled();
});

test('指定WORKINGが一覧で未確認なら別の作業版へ黙って比較対象を置換しない', async () => {
  const h = setup(); await waitFor(() => expect(entry()).toBeEnabled());
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'authoring', tab: 'versions', versionId: version(4).versionId } }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: '現行公開版とこの作業版を比較' })).not.toBeInTheDocument()); expect(h.api.compareDocumentVersions).not.toHaveBeenCalled();
});

test.each(['published ID', 'working ID', 'working state'])('開始前の%s不一致は比較POSTを送らない', async kind => {
  const h = setup(); await waitFor(() => expect(entry()).toBeEnabled());
  h.api.getDocumentVersion.mockImplementation((_id, id) => Promise.resolve(id === published.versionId ? { ...published, ...(kind === 'published ID' ? { versionId: version(1).versionId } : {}) } : { ...working, ...(kind === 'working ID' ? { versionId: version(4).versionId } : kind === 'working state' ? { lifecycleState: 'published' } : {}) }));
  await open(); await result().findByRole('alert'); expect(h.api.compareDocumentVersions).not.toHaveBeenCalled();
});

test('原本readの一時失敗はtail POST障害と異なり既取得を隠す', async () => {
  const h = setup(); await loaded(); h.api.getDocumentVersion.mockRejectedValue(new TypeError('offline')); fireEvent.click(more()); await result().findByRole('alert');
  expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(result().queryByRole('button', { name: '内容比較の続きを再試行' })).not.toBeInTheDocument(); expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(1);
});

test('明示再読取は新しいcurrentを正規readから採用し固定pairを新しくする', async () => {
  const h = setup(); await loaded(); const next = { ...version(1), title: '復帰した公開版', isCurrent: true };
  h.api.getDocument.mockResolvedValue({ ...detail(), revision: 8, currentVersionId: next.versionId }); h.api.getDocumentVersion.mockImplementation((_id, id) => Promise.resolve(id === working.versionId ? working : next));
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); }); expect(screen.queryByText('新本文0')).not.toBeInTheDocument();
  fireEvent.click(restart()); await result().findByText('Version 1 · 復帰した公開版'); expect(h.api.compareDocumentVersions).toHaveBeenLastCalledWith(documentId, { ...fixed, baseVersionId: next.versionId });
});

test('閉じた後の旧拒否は新しい明示比較を拒否しない', async () => {
  const h = setup(); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockReturnValueOnce(pending.promise); await open(); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(1));
  fireEvent.click(close()); await loaded(); await act(async () => { pending.reject(problem('FORBIDDEN', 403)); await pending.promise.catch(() => undefined); });
  expect(result().getByText('新本文0')).toBeVisible(); expect(result().queryByRole('alert')).not.toBeInTheDocument();
});

test('明示再読取は全ページを捨て、同batch連打でも先頭を一度読む', async () => {
  const h = setup(); await loaded(); fireEvent.click(more()); await screen.findByText('新本文1'); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockReturnValue(pending.promise);
  const button = restart(); act(() => { button.click(); button.click(); }); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(3)); expect(screen.queryByText('新本文0')).not.toBeInTheDocument(); expect(screen.queryByText('新本文1')).not.toBeInTheDocument();
  expect(h.api.compareDocumentVersions).toHaveBeenLastCalledWith(documentId, fixed); await act(async () => pending.resolve(comparison([9], null))); await screen.findByText('新本文9');
});

test('restart一覧refreshで明示WORKING消失なら別WORKINGを比較せず旧tailも隠す', async () => {
  const h = setup(); const pending = deferred<DiffDisplayProjection>(); h.api.compareDocumentVersions.mockImplementation((_id, body) => body.cursor ? pending.promise : Promise.resolve(comparison())); await loaded(); fireEvent.click(more()); await waitFor(() => expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(2));
  await act(async () => { await h.client.invalidateQueries({ queryKey: ['document', documentId] }); });
  const otherWorking = { ...version(4), lifecycleState: 'working', isCurrent: false };
  h.api.listDocumentVersions.mockResolvedValue({ items: [otherWorking], nextCursor: null }); h.api.getDocumentVersion.mockImplementation((_id, id) => Promise.resolve(id === published.versionId ? published : id === working.versionId ? working : otherWorking));
  fireEvent.click(restart()); await screen.findByRole('heading', { name: '選択中: WORKING · 版 4' });
  expect(screen.queryByRole('region', { name: '公開前の内容比較' })).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: '現行公開版とこの作業版を比較' })).not.toBeInTheDocument();
  await act(async () => pending.resolve(comparison([1], null))); expect(screen.queryByText('新本文1')).not.toBeInTheDocument(); expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(2); expect(h.router.state.location.search.versionId).toBe(working.versionId);
});

test('通常cacheのGC後も拒否をclose/reopenで迂回せず明示再読取を待つ', async () => {
  const h = setup(false, 1); await loaded(); h.api.compareDocumentVersions.mockRejectedValueOnce(problem('FORBIDDEN', 403)); fireEvent.click(more()); await result().findByRole('alert');
  fireEvent.click(close()); await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); }); await open(); await result().findByRole('alert');
  expect(h.api.compareDocumentVersions).toHaveBeenCalledTimes(2); expect(screen.queryByText('新本文0')).not.toBeInTheDocument();
});
