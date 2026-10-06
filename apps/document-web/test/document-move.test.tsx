import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type DocumentDetail, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { documentMoveOperations } from '../src/application/document-move';
import { folderMoveOperations } from '../src/application/document-folder-move';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), getDocument: jest.fn(), getDocumentVersion: jest.fn(), listVersionFiles: jest.fn(),
  listDocumentVersions: jest.fn(), listDocumentRevisions: jest.fn(), compareDocumentRevisions: jest.fn(), getDocumentHistory: jest.fn(), moveDocument: jest.fn(), createFolder: jest.fn(), renameFolder: jest.fn(), moveFolder: jest.fn(),
} }));
const documentId = '019a0010-0000-7000-8000-000000000010';
const otherId = '019a0010-0000-7000-8000-000000000020';
const rootId = '019a0010-0000-7000-8000-000000000099';
const available = { status: 'available' as const }; const denied = { status: 'disabled' as const, reason: 'permission' as const };
const source: Folder = { folderId: 'source', name: '共有', parentFolderId: rootId, revision: 8 };
const destinationRow: Folder = { folderId: 'destination', name: '移動先', parentFolderId: rootId, revision: 10 };
const page = (items: Folder[], nextCursor: string | null = null): FolderChildren => ({ items, nextCursor, capabilities: { createDocument: denied, createFolder: denied, renameFolder: denied, moveFolder: denied, manageAccess: denied } });
const rootFolder = { folderId: rootId, name: 'System Root', parentFolderId: null, revision: 17, capabilities: page([]).capabilities };
function detail(id = documentId, patch: Record<string, unknown> = {}) {
  return { documentId: id, title: id === documentId ? '合成文書' : '別文書', view: 'published', folderId: source.folderId, folderName: source.name, revision: 7,
    currentVersionId: 'version', documentVersionId: 'version', lifecycleState: 'PUBLISHED', metadata: {}, readState: { isRead: true, firstReadAt: null }, displayVersion: { versionId: 'version', versionNo: 2, lifecycleState: 'PUBLISHED', updatedAt: '2026-10-01T00:00:00Z', fileSummary: { authoritativeItemCount: 0, totalSizeBytes: 0, primary: null } },
    displayRevision: { revisionId: 'rev', label: '2.4', major: 2, minor: 4 }, displayTimestamp: { kind: 'revisionCreatedAt', value: '2026-10-01T00:00:00Z' },
    capabilities: { updateMetadata: denied, createVersion: denied, manageAccess: denied, compareVersions: denied, endPublication: denied, moveDocument: available }, ...patch } as unknown as DocumentDetail;
}
const title = '文書を移動'; const recovery = '文書移動の保持結果を確認'; const confirmation = 'アクセス設定への影響を確認しました';
function problem(code: string, status: number) { return { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false }; }
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => {
  for (const store of [rootFolderOperations(client), folderRenameOperations(client), folderMoveOperations(client), documentMoveOperations(client)]) {
    const operation = store.get(); if (operation) { store.put({ ...operation, status: 'succeeded' } as never); store.clearSettled(store.get()! as never); }
  }
  client.clear();
}));
function setup(entry = `/documents/${documentId}?view=published&tab=overview`, patch: Record<string, unknown> = {}) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock }; Object.values(api).forEach(mock => mock.mockReset());
  const current = new Map([[documentId, detail(documentId, patch)], [otherId, detail(otherId)]]); const rows = new Map([[rootId, [source, destinationRow]]]);
  api.getDocument.mockImplementation((id: string) => Promise.resolve(current.get(id)));
  api.getRootFolder.mockResolvedValue(rootFolder); api.listFolderChildren.mockImplementation(id => Promise.resolve(page(rows.get(id) ?? [])));
  api.listDocuments.mockResolvedValue({ view: 'published', items: [], nextCursor: null }); api.getDocumentVersion.mockResolvedValue({ currentPublicationScheduleId: null, capabilities: { download: denied, withdraw: denied } });
  api.listVersionFiles.mockResolvedValue({ items: [] }); api.listDocumentVersions.mockResolvedValue({ items: [], nextCursor: null }); api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null }); api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.moveDocument.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: body.expectedDocumentRevision + (body.fromFolderId === body.toFolderId ? 0 : 1), changed: body.fromFolderId !== body.toFolderId, occurredAt: '2026-10-06T10:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const detailPage = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: ['/documents?view=published', entry] }); const router = createRouter({ routeTree: root.addChildren([list, detailPage, tasks]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 15_000, gcTime: Infinity } } });
  clients.push(client);
  const rendered = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, current, rows, router, history, client, ...rendered };
}
async function openMove() {
  fireEvent.click(await screen.findByRole('button', { name: title })); const dialog = await screen.findByRole('dialog', { name: title });
  await waitFor(() => expect(within(dialog).getByLabelText('移動理由')).toBeEnabled()); return dialog;
}
async function chooseDestination(dialog: HTMLElement, name = destinationRow.name) {
  fireEvent.click(await within(dialog).findByRole('button', { name })); await within(dialog).findByText(/移動先フォルダーID：/);
}
function fill(dialog: HTMLElement) { fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: ' 合成理由 ' } }); fireEvent.click(within(dialog).getByLabelText(confirmation)); }
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '移動する' }).closest('form')!); }
async function goList(h: ReturnType<typeof setup>, extra: Record<string, unknown> = {}) { await act(async () => { await h.router.navigate({ to: '/documents', search: validateListSearch(extra) }); }); }
async function closeWithHeldFrames(dialog: HTMLElement, name = 'キャンセル') {
  const frames: FrameRequestCallback[] = [];
  const raf = jest.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length; });
  fireEvent.click(within(dialog).getByRole('button', { name }));
  await waitFor(() => expect(screen.queryByRole('dialog', { name: title })).not.toBeInTheDocument());
  raf.mockRestore();
  expect(frames.length).toBeGreaterThan(0);
  return async () => { await act(async () => { frames.splice(0).forEach(callback => callback(performance.now())); }); };
}

// An unconditional close callback steals the next control's focus and makes Enter reopen move.
test.each([false, true])('成功closeの遅いfocus復帰は次の操作へ移したfocus=%pを尊重し、旧triggerのfallbackを保つ', async movedFocus => {
  const h = setup(); const original = await screen.findByRole('button', { name: title });
  const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog);
  await within(dialog).findByText('文書を移動しました。');
  await waitFor(() => expect(documentMoveOperations(h.client).get()).toMatchObject({ status: 'succeeded', refresh: 'complete' }));
  expect(original).not.toBeInTheDocument();
  const runFrames = await closeWithHeldFrames(dialog, '確認して閉じる');
  expect(documentMoveOperations(h.client).get()).toBeUndefined();
  const next = screen.getByRole('tab', { name: '版・改訂' });
  if (movedFocus) next.focus(); else expect(document.body).toHaveFocus();
  await runFrames();
  expect(movedFocus ? next : screen.getByRole('button', { name: title })).toHaveFocus();
  if (movedFocus) {
    await userEvent.setup().keyboard('{Enter}');
    await waitFor(() => expect(h.router.state.location.search.tab).toBe('versions'));
    expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog', { name: title })).not.toBeInTheDocument();
  }
  expect(h.api.moveDocument).toHaveBeenCalledTimes(1);
});

test.each(['ordinary', 'unknown'])('通常closeのfocus復帰は%sの入口と保持要求を維持する', async state => {
  const h = setup(); const dialog = await openMove();
  if (state === 'unknown') {
    h.api.moveDocument.mockRejectedValueOnce(new Error('lost')); await chooseDestination(dialog); fill(dialog); submit(dialog);
    await within(dialog).findByText('移動結果を確認できません');
  }
  const saved = documentMoveOperations(h.client).get();
  const runFrames = await closeWithHeldFrames(dialog, state === 'unknown' ? '閉じる' : 'キャンセル');
  expect(document.body).toHaveFocus(); await runFrames();
  expect(screen.getByRole('button', { name: state === 'unknown' ? recovery : title })).toHaveFocus();
  expect(documentMoveOperations(h.client).get()).toBe(saved);
});

test.each(['disabled', 'removed'])('close後に入口が%sになったときfocus復帰しない', async state => {
  const h = setup(); const dialog = await openMove(); const runFrames = await closeWithHeldFrames(dialog);
  await act(async () => { h.client.setQueryData(['document', documentId, 'published'], detail(documentId, { capabilities: { ...detail().capabilities, moveDocument: state === 'disabled' ? denied : undefined } })); });
  if (state === 'disabled') await waitFor(() => expect(screen.getByRole('button', { name: title })).toBeDisabled());
  else await waitFor(() => expect(screen.queryByRole('button', { name: title })).not.toBeInTheDocument());
  expect(document.body).toHaveFocus(); await runFrames(); expect(document.body).toHaveFocus();
});

test('pendingの移動は閉じる操作を受け付けず同じ要求を保持する', async () => {
  const h = setup(); h.api.moveDocument.mockReturnValue(deferred<unknown>().promise);
  const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog);
  await waitFor(() => expect(documentMoveOperations(h.client).get()?.status).toBe('pending'));
  const saved = documentMoveOperations(h.client).get(); const close = within(dialog).getByRole('button', { name: '閉じる' });
  expect(close).toBeDisabled(); fireEvent.click(close); await userEvent.setup().keyboard('{Escape}');
  expect(dialog).toBeVisible(); expect(documentMoveOperations(h.client).get()).toBe(saved); expect(h.api.moveDocument).toHaveBeenCalledTimes(1);
});

test.each(['reopen', 'reopen-idle', 'tab', 'other-document', 'unmount'])('閉じた移動の遅いfocus復帰は%s後のfocusを上書きしない', async change => {
  const h = setup(); const dialog = await openMove(); const runFrames = await closeWithHeldFrames(dialog);
  let focus: HTMLElement = document.body;
  if (change === 'reopen' || change === 'reopen-idle') {
    const again = await openMove();
    if (change === 'reopen') { focus = within(again).getByLabelText('移動理由'); focus.focus(); }
    else (document.activeElement as HTMLElement).blur();
  }
  else if (change === 'unmount') h.unmount();
  else {
    await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: change === 'tab' ? documentId : otherId }, search: validateDetailSearch({ tab: change === 'tab' ? 'versions' : 'overview' }) }); });
    (document.activeElement as HTMLElement).blur();
  }
  expect(focus).toHaveFocus(); await runFrames(); expect(focus).toHaveFocus();
  expect(h.api.moveDocument).not.toHaveBeenCalled();
});

test('閉じた移動の遅いfocus復帰は初回改訂readと明示pair選択後の比較Enterを妨げない', async () => {
  const h = setup(); const baseRevisionId = '019a0010-0000-7000-8000-000000000111'; const targetRevisionId = '019a0010-0000-7000-8000-000000000110';
  h.api.listDocumentRevisions.mockResolvedValue({ items: [
    { revisionId: baseRevisionId, documentVersionId: 'version', major: 1, minor: 1, label: '1.1', createdAt: '2026-10-02T00:00:00Z', sourceKind: 'metadataRevision', metadataSnapshotStatus: 'complete' },
    { revisionId: targetRevisionId, documentVersionId: 'version', major: 1, minor: 0, label: '1.0', createdAt: '2026-10-01T00:00:00Z', sourceKind: 'publication', metadataSnapshotStatus: 'complete' },
  ], nextCursor: null });
  h.api.compareDocumentRevisions.mockReturnValue(deferred<unknown>().promise);
  const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog);
  await within(dialog).findByText('文書を移動しました。');
  await waitFor(() => expect(documentMoveOperations(h.client).get()).toMatchObject({ status: 'succeeded', refresh: 'complete' }));
  const runFrames = await closeWithHeldFrames(dialog, '確認して閉じる'); const user = userEvent.setup();
  screen.getByRole('tab', { name: '版・改訂' }).focus(); await user.keyboard('{Enter}');
  const revisions = await screen.findByRole('list', { name: '正式改訂一覧' }); expect(within(revisions).getAllByRole('listitem')).toHaveLength(2);
  expect(h.api.listDocumentRevisions).toHaveBeenCalledTimes(1);
  await user.selectOptions(screen.getByRole('combobox', { name: '基準' }), baseRevisionId);
  await user.selectOptions(screen.getByRole('combobox', { name: '対象' }), targetRevisionId);
  await waitFor(() => expect(h.router.state.location.search).toMatchObject({ tab: 'versions', baseRevisionId, targetRevisionId }));
  expect(screen.getByRole('combobox', { name: '基準' })).toHaveValue(baseRevisionId);
  expect(screen.getByRole('combobox', { name: '対象' })).toHaveValue(targetRevisionId);
  const next = screen.getByRole('tab', { name: '新旧比較' }); next.focus(); await runFrames(); expect(next).toHaveFocus();
  await user.keyboard('{Enter}');
  await waitFor(() => expect(h.api.compareDocumentRevisions).toHaveBeenCalledTimes(1));
  expect(h.api.compareDocumentRevisions).toHaveBeenCalledWith(documentId, { baseRevisionId, targetRevisionId, projection: 'display', pageSize: 50 });
  expect(h.router.state.location.search).toMatchObject({ tab: 'compare', baseRevisionId, targetRevisionId });
  expect(screen.queryByRole('dialog', { name: title })).not.toBeInTheDocument(); expect(h.api.moveDocument).toHaveBeenCalledTimes(1);
});

// Removing the ordinary detail entry must fail before any POST can run.
test('通常概要から文書IDと元所属を示し、移動先と継承影響を明示確認する', async () => {
  const h = setup(); const dialog = await openMove();
  expect(within(dialog).getByText(`対象文書ID：${documentId}`)).toBeVisible(); expect(within(dialog).getByText('現在の対象名：合成文書（revision 7）')).toBeVisible();
  expect(within(dialog).getByText('現在の元所属名：共有')).toBeVisible(); expect(within(dialog).getByText('元フォルダーID：source')).toBeVisible();
  expect(within(dialog).queryByText(/移動先フォルダーID：/)).not.toBeInTheDocument(); expect(within(dialog).getByRole('button', { name: '移動する' })).toBeDisabled();
  await chooseDestination(dialog); fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: '理由' } });
  expect(within(dialog).getByText(/明示アクセス設定は保持/)).toBeVisible(); expect(within(dialog).getByText(/自分を含む閲覧・編集権限が変わり得る/)).toBeVisible();
  submit(dialog); expect(h.api.moveDocument).not.toHaveBeenCalled();
});
test.each([['System Root', rootId, true], ['共有', source.folderId, false], ['移動先', destinationRow.folderId, true]])('移動先のmove/create hintに依存せず%sへ5項目を送りno-opも照合する', async (name, toFolderId, changed) => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog, name); fill(dialog); submit(dialog);
  await within(dialog).findByText(changed ? '文書を移動しました。' : '所属の変更はありませんでした。');
  expect(h.api.moveDocument).toHaveBeenCalledTimes(1); const [id, body] = h.api.moveDocument.mock.calls[0]!;
  expect(id).toBe(documentId); expect(body).toMatchObject({ fromFolderId: source.folderId, toFolderId, expectedDocumentRevision: 7, reason: '合成理由' });
  expect(Object.keys(body).sort()).toEqual(['expectedDocumentRevision', 'fromFolderId', 'operationId', 'reason', 'toFolderId']); expect(screen.getByText('2.4')).toBeInTheDocument();
});
test.each([null, undefined, ''])('元folderId %pはURLやRootで補完せず入口を停止する', async folderId => {
  const h = setup(`/documents/${documentId}?view=published&returnTo=%2Fdocuments%3FfolderId%3Dsource`, { folderId });
  expect(await screen.findByRole('button', { name: title })).toBeDisabled(); expect(h.api.moveDocument).not.toHaveBeenCalled();
});
test.each([null, '', undefined])('元Folder名 %pも推測しない', async folderName => {
  setup(undefined, { folderName }); expect(await screen.findByRole('button', { name: title })).toBeDisabled();
});
test('authoring詳細は同じauthoring viewでfresh GETして送る', async () => {
  const h = setup(`/documents/${documentId}?view=authoring`, { view: 'authoring' }); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog);
  await within(dialog).findByText('文書を移動しました。'); expect(h.api.getDocument.mock.calls.every(call => call[1] === 'authoring')).toBe(true);
});
test.each(['title', 'folderId', 'folderName', 'revision'])('開く時点の%s変更は黙って基準にせず明示見直しを要求する', async field => {
  const h = setup(); await screen.findByRole('button', { name: title }); h.current.set(documentId, detail(documentId, { [field]: field === 'revision' ? 10 : '変更' }));
  const dialog = await openMove(); await within(dialog).findByText(/対象の名前・所属またはrevisionが変わりました/); expect(h.api.moveDocument).not.toHaveBeenCalled();
});
test.each(['title', 'folderId', 'folderName', 'revision'])('送信前の%s変更は理由を保持して確認を解除し見直し後だけ送る', async field => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); h.current.set(documentId, detail(documentId, { [field]: field === 'revision' ? 10 : '変更' }));
  submit(dialog); await within(dialog).findByText(/対象の名前・所属またはrevisionが変わりました/); expect(h.api.moveDocument).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 '); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked();
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByRole('button', { name: '移動する' });
  await waitFor(() => expect(within(dialog).getByLabelText(confirmation)).toBeEnabled()); fireEvent.click(within(dialog).getByLabelText(confirmation)); submit(dialog); await within(dialog).findByText('文書を移動しました。');
  expect(h.api.moveDocument.mock.calls[0]![1]).toMatchObject({ fromFolderId: field === 'folderId' ? '変更' : 'source', expectedDocumentRevision: field === 'revision' ? 10 : 7 });
});
test.each(['null-folder', 'denied', 'wrong-id', 'read-error'])('送信前fresh %sから権限や所属を推測しない', async change => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog);
  if (change === 'read-error') h.api.getDocument.mockRejectedValue(problem('DOCUMENT_NOT_FOUND', 404)); else h.current.set(documentId, detail(change === 'wrong-id' ? otherId : documentId, change === 'null-folder' ? { folderId: null } : change === 'denied' ? { capabilities: { ...detail().capabilities, moveDocument: denied } } : {}));
  submit(dialog); await waitFor(() => expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked()); expect(h.api.moveDocument).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 ');
});
test.each(['cancel', 'reopen', 'back', 'other-document', 'tab', 'navigation'])('遅い送信前GET後の%sで自動送信しない', async change => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); const read = deferred<DocumentDetail>(); h.api.getDocument.mockReturnValue(read.promise); submit(dialog);
  if (change === 'cancel' || change === 'reopen') { fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' })); if (change === 'reopen') fireEvent.click(screen.getByRole('button', { name: title })); }
  else if (change === 'back') await act(async () => h.history.back());
  else if (change === 'other-document') await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: validateDetailSearch({}) }); });
  else if (change === 'tab') await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId }, search: validateDetailSearch({ tab: 'versions' }) }); });
  else await act(async () => { await h.router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(detail())); expect(h.api.moveDocument).not.toHaveBeenCalled();
});
test('移動先変更とsubmitが同batchでも旧移動先と旧確認から送らない', async () => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); const read = deferred<typeof rootFolder>(); h.api.getRootFolder.mockReturnValue(read.promise);
  act(() => { fireEvent.click(within(dialog).getByRole('button', { name: 'System Root' })); submit(dialog); }); await act(async () => read.resolve(rootFolder));
  expect(h.api.moveDocument).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked();
});
test('遅い移動先readが新選択を上書きしない', async () => {
  const h = setup(); const dialog = await openMove(); await within(dialog).findByRole('button', { name: destinationRow.name });
  const read = deferred<FolderChildren>(); h.api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  fireEvent.click(within(dialog).getByRole('button', { name: destinationRow.name })); fireEvent.click(within(dialog).getByRole('button', { name: 'System Root' }));
  await within(dialog).findByText(`移動先フォルダーID：${rootId}`); await act(async () => read.resolve(page([source, destinationRow]))); expect(within(dialog).getByText(`移動先フォルダーID：${rootId}`)).toBeVisible();
});
test('移動先のfresh変更は理由を保持し明示見直しへ止める', async () => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); h.rows.set(rootId, [source, { ...destinationRow, name: '外部変更', revision: 11 }]); submit(dialog);
  await within(dialog).findByText(/移動先の状態が変わりました/); expect(h.api.moveDocument).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 ');
});
test('UNKNOWNはCloseと一覧往復と再送403後も同一path/body/表示contextを保持する', async () => {
  const h = setup(); h.api.moveDocument.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403)); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog);
  await within(dialog).findByText('移動結果を確認できません'); const first = h.api.moveDocument.mock.calls[0]!; fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  await goList(h); expect(await screen.findByRole('button', { name: recovery })).toBeEnabled(); expect(screen.getByRole('button', { name: 'System Rootにフォルダーを作成' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: recovery })); const recovered = await screen.findByRole('dialog', { name: title });
  expect(within(recovered).getByText('送信時の元所属名：共有')).toBeVisible(); fireEvent.click(within(recovered).getByRole('button', { name: '同じ内容で再試行' }));
  await within(recovered).findByText(/FORBIDDEN.*初回の移動結果は未確定/); expect(h.api.moveDocument.mock.calls[1]![0]).toBe(first[0]); expect(h.api.moveDocument.mock.calls[1]![1]).toBe(first[1]);
});
test.each(['unknown', 'succeeded'])('移動後GET403でも%sの保持結果を詳細外と一覧から回復する', async status => {
  const h = setup(); h.api.moveDocument.mockImplementation(async (id, body) => { h.api.getDocument.mockRejectedValue(problem('FORBIDDEN', 403)); if (status === 'unknown') throw new Error('lost'); return { operationId: body.operationId, resourceId: id, changed: true, resultingRevision: 8, occurredAt: '2026-10-06T10:00:00Z' }; });
  const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); submit(dialog); await within(dialog).findByText(status === 'unknown' ? '移動結果を確認できません' : '文書を移動しました。');
  if (status === 'succeeded') await within(dialog).findByText(/表示を更新できませんでした/); else await act(async () => { await h.client.invalidateQueries({ queryKey: ['document'] }); });
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); expect(screen.getByRole('button', { name: recovery })).toBeEnabled();
  await goList(h, { cursor: 'opaque-old-cursor', titleContains: '条件' }); fireEvent.click(await screen.findByRole('button', { name: recovery })); const saved = await screen.findByRole('dialog', { name: title });
  expect(within(saved).getByText(`対象文書ID：${documentId}`)).toBeVisible(); expect(h.router.state.location.search).toMatchObject({ cursor: 'opaque-old-cursor', titleContains: '条件' });
});
test('送信後別navigationへ移っても遅延receipt/refreshは元URLへ戻さない', async () => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); const response = deferred<unknown>(); h.api.moveDocument.mockReturnValue(response.promise); submit(dialog); await waitFor(() => expect(h.api.moveDocument).toHaveBeenCalledTimes(1));
  const body = h.api.moveDocument.mock.calls[0]![1]; await goList(h, { cursor: 'opaque', titleContains: '新条件' }); await act(async () => response.resolve({ operationId: body.operationId, resourceId: documentId, changed: true, resultingRevision: 8, occurredAt: '2026-10-06T10:00:00Z' }));
  expect(h.router.state.location.search).toMatchObject({ cursor: 'opaque', titleContains: '新条件' }); fireEvent.click(await screen.findByRole('button', { name: recovery })); await screen.findByText('文書を移動しました。');
});
test.each(['create', 'rename', 'folder-move'])('Folder %s未確定が送信前GET中に発生しても文書移動を送らない', async kind => {
  const h = setup(); const dialog = await openMove(); await chooseDestination(dialog); fill(dialog); const read = deferred<DocumentDetail>(); h.api.getDocument.mockReturnValue(read.promise); submit(dialog);
  act(() => { if (kind === 'create') rootFolderOperations(h.client).put({ status: 'unknown', request: { operationId: 'fixed', folderId: 'new', parentFolderId: rootId, expectedParentRevision: 17, name: '新', reason: '理由' } });
    else if (kind === 'rename') folderRenameOperations(h.client).put({ status: 'unknown', targetFolderId: source.folderId, currentName: source.name, expectedChanged: true, context: { kind: 'selected', folderId: source.folderId, sourceParentId: rootId, pageLimit: 1, name: source.name }, request: { operationId: 'fixed', expectedFolderRevision: 8, name: '新', reason: '理由' } });
    else folderMoveOperations(h.client).put({ status: 'unknown', targetFolderId: source.folderId, currentName: source.name, expectedChanged: true, context: { kind: 'selected', folderId: source.folderId, sourceParentId: rootId, pageLimit: 1, name: source.name }, destination: { kind: 'root', folderId: rootId, name: 'System Root' }, request: { operationId: 'fixed', fromParentId: rootId, toParentId: 'other', expectedFolderRevision: 8, reason: '理由' } }); });
  await act(async () => read.resolve(detail())); expect(h.api.moveDocument).not.toHaveBeenCalled();
});

test('通常詳細も読めない元Folder名をRootと表示しない', async () => {
  setup(undefined, { folderId: null, folderName: null }); await screen.findByRole('heading', { name: '基本情報' });
  expect(screen.getByText('所属フォルダーを確認できません')).toBeVisible(); expect(screen.queryByText('ルート')).not.toBeInTheDocument();
});

const destinationReadError = '移動先の最新の状態を取得できません。移動先ツリーで選び直してください。';
const invalidDestinationNames = [true, { unexpected: '名前' }, 42];
const destinationTypeCases = ['root', 'selected'].flatMap(kind => invalidDestinationNames.flatMap(name => ['selection', 'submit', 'review'].map(stage => ({ kind, name, stage }))));
test.each(destinationTypeCases)('移動先$kindのfresh name $nameは$stage時に採用せず理由と明示確認を保持する', async ({ kind, name, stage }) => {
  const h = setup(); const dialog = await openMove(); const label = kind === 'root' ? 'System Root' : destinationRow.name;
  await within(dialog).findByRole('button', { name: label });
  fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: ' 合成理由 ' } });
  if (stage !== 'selection') { await chooseDestination(dialog, label); fireEvent.click(within(dialog).getByLabelText(confirmation)); }
  if (stage === 'review') {
    h.current.set(documentId, detail(documentId, { revision: 8 })); submit(dialog);
    await within(dialog).findByText(/対象の名前・所属またはrevisionが変わりました/);
  }
  if (kind === 'root') h.api.getRootFolder.mockResolvedValue({ ...rootFolder, name });
  else h.rows.set(rootId, [source, { ...destinationRow, name } as never]);
  if (stage === 'selection') fireEvent.click(within(dialog).getByRole('button', { name: label }));
  else if (stage === 'submit') submit(dialog);
  else fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await within(dialog).findByText(stage === 'review' ? '最新の文書と移動先を確認できません。状態を読み直してから見直してください。' : destinationReadError);
  expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 '); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked(); expect(within(dialog).getByLabelText(confirmation)).toBeDisabled();
  expect(h.api.moveDocument).not.toHaveBeenCalled();
  if (kind === 'root') h.api.getRootFolder.mockResolvedValue(rootFolder); else h.rows.set(rootId, [source, destinationRow]);
  if (stage === 'review') {
    fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByRole('button', { name: '移動する' });
  } else await chooseDestination(dialog, label);
  await waitFor(() => expect(within(dialog).getByLabelText(confirmation)).toBeEnabled());
  expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked(); submit(dialog); expect(h.api.moveDocument).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByLabelText(confirmation)); submit(dialog); await within(dialog).findByText('文書を移動しました。');
  expect(h.api.moveDocument).toHaveBeenCalledTimes(1); expect(h.api.moveDocument.mock.calls[0]![1]).toMatchObject({ toFolderId: kind === 'root' ? rootId : destinationRow.folderId, reason: '合成理由', expectedDocumentRevision: stage === 'review' ? 8 : 7 });
});
const rootIdTypeCases = [42, true, { unexpected: 'ID' }, ''].flatMap(folderId => ['selection', 'submit', 'review'].map(stage => ({ folderId, stage })));
test.each(rootIdTypeCases)('Root ID $folderIdを$stage時に移動先の文字列IDとみなさずPOSTを止める', async ({ folderId, stage }) => {
  const h = setup(); if (stage === 'selection') h.api.getRootFolder.mockResolvedValue({ ...rootFolder, folderId });
  const dialog = await openMove();
  fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: ' 合成理由 ' } });
  if (stage !== 'selection') { await chooseDestination(dialog, 'System Root'); fireEvent.click(within(dialog).getByLabelText(confirmation)); }
  if (stage === 'review') {
    h.current.set(documentId, detail(documentId, { revision: 8 })); submit(dialog);
    await within(dialog).findByText(/対象の名前・所属またはrevisionが変わりました/);
  }
  h.api.getRootFolder.mockResolvedValue({ ...rootFolder, folderId });
  if (stage === 'selection') fireEvent.click(await within(dialog).findByRole('button', { name: 'System Root' }));
  else if (stage === 'submit') submit(dialog);
  else fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await within(dialog).findByText(stage === 'review' ? '最新の文書と移動先を確認できません。状態を読み直してから見直してください。' : destinationReadError);
  expect(within(dialog).getByLabelText(confirmation)).toBeDisabled(); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked(); expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 '); expect(h.api.moveDocument).not.toHaveBeenCalled();
  h.api.getRootFolder.mockResolvedValue(rootFolder);
  if (stage === 'review') { fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByRole('button', { name: '移動する' }); }
  else {
    if (stage === 'selection') await act(async () => { await h.client.invalidateQueries({ queryKey: ['folder-tree', 'root'] }); });
    await chooseDestination(dialog, 'System Root');
  }
  await waitFor(() => expect(within(dialog).getByLabelText(confirmation)).toBeEnabled()); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked(); submit(dialog); expect(h.api.moveDocument).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByLabelText(confirmation)); submit(dialog); await within(dialog).findByText('文書を移動しました。'); expect(h.api.moveDocument).toHaveBeenCalledTimes(1); expect(h.api.moveDocument.mock.calls[0]![1].toFolderId).toBe(rootId);
});
