import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(), renameFolder: jest.fn(), getDocument: jest.fn(), getDocumentVersion: jest.fn(),
} }));
const rootId = '019a0010-0000-7000-8000-000000000041';
const available = { status: 'available' as const };
const denied = { status: 'disabled' as const, reason: 'permission' as const };
const folder = (index: number, parentFolderId = rootId, revision = 8): Folder => ({ folderId: `019a0010-0000-7000-8000-${String(index).padStart(12, '0')}`, name: `資料${index}`, revision, parentFolderId });
const p = folder(100); const q = folder(101);
const page = (items: Folder[], nextCursor: string | null = null, createFolder = available as FolderChildren['capabilities']['createFolder']): FolderChildren => ({ items, nextCursor, capabilities: { createDocument: available, createFolder, renameFolder: available, moveFolder: available, manageAccess: available } });
const success = (request: { operationId: string; folderId: string }) => ({ operationId: request.operationId, resourceId: request.folderId, resultingRevision: 0, changed: true, occurredAt: '2026-10-05T12:00:00Z' });
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => client.clear()));
function setup(entry = '/documents?view=published') {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'System Root', revision: 17, parentFolderId: null, capabilities: { createFolder: available, createDocument: available } });
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [p, q] : [])));
  api.listDocuments.mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  api.getDocument.mockResolvedValue({ metadata: {}, capabilities: { createVersion: denied, compareVersions: denied, manageAccess: denied } });
  api.getDocumentVersion.mockResolvedValue({ capabilities: { download: denied } });
  api.createFolder.mockImplementation(request => Promise.resolve(success(request)));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [entry] });
  const router = createRouter({ routeTree: root.addChildren([list, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, history, invalidate };
}
const rootTitle = 'System Rootにフォルダーを作成';
const selectedTitle = '選択したフォルダーに子フォルダーを作成';
const renameTitle = '選択したフォルダー名を変更';
async function choose(row = p) {
  fireEvent.click(await screen.findByRole('button', { name: row.name }));
  await waitFor(() => expect(screen.getByRole('button', { name: selectedTitle })).toBeEnabled());
}
async function open(title = selectedTitle) {
  fireEvent.click(await screen.findByRole('button', { name: title }));
  return screen.findByRole('dialog');
}
function fill(dialog: HTMLElement, name = '新資料') {
  fireEvent.change(within(dialog).getByLabelText('フォルダー名'), { target: { value: name } });
  fireEvent.change(within(dialog).getByLabelText('作成理由'), { target: { value: '合成理由' } });
}
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '作成する' }).closest('form')!); }

// Removing the actual route entry must make this fail.
test('実一覧routeの選択行から改名dialogを開ける', async () => {
  setup(); await choose();
  fireEvent.click(screen.getByRole('button', { name: renameTitle }));
  const dialog = await screen.findByRole('dialog', { name: renameTitle });
  await waitFor(() => expect(within(dialog).getByLabelText('変更先のフォルダー名')).toBeEnabled());
  expect(within(dialog).getByLabelText('変更先のフォルダー名')).toHaveValue(p.name);
  expect(within(dialog).getByLabelText('変更理由')).toBeVisible();
});
const renameResult = (id: string, body: { operationId: string; expectedFolderRevision: number; name: string }, currentName = p.name) => ({ operationId: body.operationId, resourceId: id, resultingRevision: body.expectedFolderRevision + (currentName === body.name ? 0 : 1), changed: currentName !== body.name, occurredAt: '2026-10-06T00:00:00Z' });
async function openRename() {
  const entry = await screen.findByRole('button', { name: renameTitle });
  await waitFor(() => expect(entry).toBeEnabled()); fireEvent.click(entry);
  const dialog = await screen.findByRole('dialog', { name: renameTitle });
  await waitFor(() => expect(within(dialog).getByLabelText('変更先のフォルダー名')).toBeEnabled());
  return dialog;
}
function fillRename(dialog: HTMLElement, name = '変更資料') {
  fireEvent.change(within(dialog).getByLabelText('変更先のフォルダー名'), { target: { value: name } });
  fireEvent.change(within(dialog).getByLabelText('変更理由'), { target: { value: '変更の合成理由' } });
}
function submitRename(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '変更を保存する' }).closest('form')!); }
function renameSetup(entry?: string) {
  const h = setup(entry); h.api.renameFolder.mockImplementation((id, body) => Promise.resolve(renameResult(id, body))); return h;
}
// Substituting the display cache for the second fresh read, or patching chosenFolder from the receipt, fails.
test('fresh基準と再readで送信しG/P/documentだけ無効化、URLと文書filterを保ちPコピーだけ破棄する', async () => {
  const { api, client, router, invalidate } = renameSetup('/documents?view=published&titleContains=条件&selectedDocumentId=doc&panel=open');
  await choose(); const dialog = await openRename(); fillRename(dialog, '\u2003e\u0301\u2003');
  submitRename(dialog); await within(dialog).findByText('フォルダー名を変更しました。');
  expect(api.renameFolder.mock.calls[0]).toEqual([p.folderId, expect.objectContaining({ expectedFolderRevision: 8, name: 'é', reason: '変更の合成理由' })]);
  expect(Object.keys(api.renameFolder.mock.calls[0]![1]).sort()).toEqual(['expectedFolderRevision', 'name', 'operationId', 'reason']);
  expect(invalidate.mock.calls.map(([arg]) => arg!.queryKey)).toEqual([['folder-tree', rootId], ['folder-tree', p.folderId], ['documents'], ['document']]);
  expect(router.state.location.search).toMatchObject({ folderId: p.folderId, titleContains: '条件', selectedDocumentId: 'doc', panel: 'open' });
  expect(client.getQueryData(['folder-tree', 'root'])).toMatchObject({ revision: 17 });
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' }));
  expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled(); expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled();
  expect(screen.getByText(/改名対象はフォルダーツリーからもう一度選択してください/)).toBeVisible(); expect(screen.getByRole('button', { name: '文書を登録' })).toBeEnabled();
  await waitFor(() => expect(screen.getByRole('button', { name: p.name })).toHaveFocus());
});
test('正規化同名も実PATCHでno-op receiptを確認する', async () => {
  const { api } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog, `\u2003${p.name}\u2003`); submitRename(dialog);
  await within(dialog).findByText('名前の変更はありませんでした。'); expect(api.renameFolder).toHaveBeenCalledTimes(1);
});
test.each(['name', 'revision'] as const)('基準%s変更は希望名/理由を保持して止め、明示見直しread後にだけ新基準で送る', async kind => {
  const { api } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  const current = { ...p, name: kind === 'name' ? '外部変更' : p.name, revision: 19 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [current, q] : [])));
  submitRename(dialog); await within(dialog).findByText(/編集中にフォルダーの名前またはrevisionが変わりました/);
  expect(api.renameFolder).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText('変更先のフォルダー名')).toHaveValue('変更資料');
  expect(within(dialog).getByLabelText('変更理由')).toHaveValue('変更の合成理由');
  expect(within(dialog).getByText(/最新の現在名：/)).toHaveTextContent(current.name);
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '変更を保存する' })).toBeEnabled());
  submitRename(dialog); await within(dialog).findByText('フォルダー名を変更しました。');
  expect(api.renameFolder.mock.calls[0]![1]).toMatchObject({ expectedFolderRevision: 19, name: '変更資料' });
});
test('背景query更新で未送信の希望名/理由/編集基準を消さない', async () => {
  const { api, client } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [{ ...p, name: '外部更新', revision: 15 }, q] : [])));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId] }); });
  expect(within(dialog).getByLabelText('変更先のフォルダー名')).toHaveValue('変更資料'); expect(within(dialog).getByText(/現在名：/)).toHaveTextContent(p.name);
  submitRename(dialog); await within(dialog).findByText(/編集中に/); expect(api.renameFolder).not.toHaveBeenCalled();
});
test.each(['cancel', 'same-selection', 'other-selection', 'back', 'navigation', 'filter'] as const)('送信前read中%sは遅延完了からPATCHしない', async action => {
  const { api, router, history } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  act(() => { submitRename(dialog); submitRename(dialog); });
  if (action === 'cancel') fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  else if (action === 'same-selection' || action === 'other-selection') fireEvent.click(screen.getByRole('button', { name: action === 'same-selection' ? p.name : q.name, hidden: true }));
  else if (action === 'back') await act(async () => history.back());
  else if (action === 'filter') await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ ...router.state.location.search, titleContains: '別' }) }); });
  else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(page([p, q]))); expect(api.renameFolder).not.toHaveBeenCalled();
});
test('fresh sourceの新opaque cursorだけを開始時2ページまで読んで、折畳みqueryをfresh取得済み扱いしない', async () => {
  const { api, client } = renameSetup();
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(page(id === rootId ? cursor ? [p] : [q] : [], id === rootId && !cursor ? 'old' : null)));
  await screen.findByRole('button', { name: q.name }); fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーをさらに表示' })); await choose();
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーを閉じる' }));
  const reads: [string, string | undefined][] = [];
  api.listFolderChildren.mockImplementation((id, cursor) => { reads.push([id, cursor]); return Promise.resolve(page(id === rootId && cursor === 'fresh' ? [p] : [], id === rootId && !cursor ? 'fresh' : id === rootId ? 'outside' : null)); });
  const dialog = await openRename(); fillRename(dialog); submitRename(dialog); await within(dialog).findByText('フォルダー名を変更しました。');
  expect(reads.slice(0, 6)).toEqual([[rootId, undefined], [rootId, 'fresh'], [p.folderId, undefined], [rootId, undefined], [rootId, 'fresh'], [p.folderId, undefined]]);
  expect(reads).not.toContainEqual([rootId, 'old']); expect(reads).not.toContainEqual([rootId, 'outside']);
  expect(client.getQueryData(['folder-tree', rootId, 'pages'])).toMatchObject({ pageParams: [undefined, 'old'] });
  expect(client.getQueryState(['folder-tree', rootId, 'pages'])?.isInvalidated).toBe(true);
});
test.each(['permission', 'lifecycle', 'notHumanInteractive', 'unsupported'] as const)('自身rename capability %sを使い親/createのavailableで補わない', async reason => {
  const { api } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([p, q]) : { ...page([]), capabilities: { ...page([]).capabilities, renameFolder: { status: 'disabled', reason } } }));
  submitRename(dialog); await within(dialog).findByRole('alert'); expect(api.renameFolder).not.toHaveBeenCalled(); expect(within(dialog).getByRole('button', { name: '変更を保存する' })).toBeDisabled();
});
test.each(['unsupported', 'permission'] as const)('Root自身のserver理由%sと直URLの再選択案内を保持する', async reason => {
  const { api, router, history } = renameSetup(`/documents?view=published&folderId=${p.folderId}`);
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'System Root', revision: 17, capabilities: { createFolder: available, createDocument: available, renameFolder: { status: 'disabled', reason } } });
  await screen.findByRole('button', { name: p.name }); expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled();
  expect(screen.getByRole('button', { name: '文書を登録' })).toBeEnabled(); await choose();
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  await screen.findByRole('button', { name: p.name }); expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'System Root' })); await waitFor(() => expect(router.state.location.search.folderId).toBeUndefined());
  expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled(); expect(api.renameFolder).not.toHaveBeenCalled();
});
test.each([page([q]), page([q], 'outside'), page([{ ...p, parentFolderId: q.folderId }]), page([{ ...p, revision: -1 }]), problem('FORBIDDEN', 403)])('再freshの未発見/移動/不正/read拒否は停止 %p', async response => {
  const { api } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.listFolderChildren.mockImplementation(id => id === rootId && !('items' in response) ? Promise.reject(response) : Promise.resolve(id === rootId ? response : page([])));
  submitRename(dialog); await within(dialog).findByText(/ツリーで選び直してください/); expect(api.renameFolder).not.toHaveBeenCalled();
});
test('UNKNOWNは画面往復/別Q/現在read失敗でも元path/bodyだけ再送し403/404後も保持する', async () => {
  const { api, router, history } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.renameFolder.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockRejectedValueOnce(problem('FOLDER_NOT_FOUND', 404));
  submitRename(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' }); const [id, request] = api.renameFolder.mock.calls[0]!;
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); fireEvent.click(screen.getByRole('button', { name: q.name }));
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  api.listFolderChildren.mockRejectedValue(new Error('reads failed')); fireEvent.click(await screen.findByRole('button', { name: renameTitle })); const reopened = await screen.findByRole('dialog');
  const reads = api.listFolderChildren.mock.calls.length;
  for (let i = 0; i < 3; i++) { fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' })); await within(reopened).findByRole('button', { name: i === 2 ? '確認して閉じる' : '同じ内容で再試行' }); }
  expect(api.renameFolder).toHaveBeenCalledTimes(4); for (const [actualId, body] of api.renameFolder.mock.calls) { expect(actualId).toBe(id); expect(body).toBe(request); }
  expect(within(reopened).getByText(/送信時の現在名/)).toHaveTextContent(p.name); expect(router.state.location.search.folderId).toBe(q.folderId);
  // New reads only come from post-success invalidation; replay itself is read-independent.
  expect(api.listFolderChildren.mock.calls.slice(reads).every(([actualId]) => actualId === rootId || actualId === q.folderId)).toBe(true);
});
function createSaved(status: 'pending' | 'unknown') { return { status, request: { operationId: 'saved-create', folderId: 'new-child', parentFolderId: rootId, expectedParentRevision: 17, name: 'saved-child', reason: 'reason' } }; }
function renameSaved(status: 'pending' | 'unknown') { return { status, targetFolderId: p.folderId, context: { kind: 'selected' as const, folderId: p.folderId, sourceParentId: rootId, pageLimit: 1, name: p.name }, currentName: p.name, expectedChanged: true, request: { operationId: 'saved-rename', expectedFolderRevision: 8, name: 'saved-name', reason: 'reason' } }; }
test.each(['pending', 'unknown'] as const)('create %sは新renameを止め、元createの確認入口は保持', async status => {
  const { client, api } = renameSetup(); await choose(); const saved = createSaved(status); act(() => rootFolderOperations(client).put(saved));
  expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled(); expect(screen.getByRole('button', { name: rootTitle })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: rootTitle })); const dialog = await screen.findByRole('dialog'); expect(within(dialog).getByLabelText('フォルダー名')).toHaveValue(saved.request.name);
  expect(api.renameFolder).not.toHaveBeenCalled(); act(() => { const settled = { ...saved, status: 'rejected' as const }; rootFolderOperations(client).put(settled); rootFolderOperations(client).clearSettled(settled); });
});
test.each(['pending', 'unknown'] as const)('rename %sはRoot/選択親の新createを止め、互いの要求を変えない', async status => {
  const { client } = renameSetup(); await choose(); const saved = renameSaved(status); act(() => folderRenameOperations(client).put(saved));
  expect(screen.getByRole('button', { name: rootTitle })).toBeDisabled(); expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled();
  expect(screen.getByRole('button', { name: renameTitle })).toBeEnabled(); expect(folderRenameOperations(client).get()).toBe(saved);
  act(() => { const settled = { ...saved, status: 'rejected' as const }; folderRenameOperations(client).put(settled); folderRenameOperations(client).clearSettled(settled); });
});
test.each(['pending', 'unknown'] as const)('rename read中createが%sになるraceは送信直前の実storeで止める', async status => {
  const { api, client } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  submitRename(dialog); const saved = createSaved(status); act(() => rootFolderOperations(client).put(saved)); await act(async () => read.resolve(page([p, q])));
  expect(api.renameFolder).not.toHaveBeenCalled(); expect(rootFolderOperations(client).get()).toBe(saved);
  act(() => { const settled = { ...saved, status: 'rejected' as const }; rootFolderOperations(client).put(settled); rootFolderOperations(client).clearSettled(settled); });
});
test.each(['root', 'selected'] as const)('create %s read中renameがUNKNOWNになるraceはPOST前の実storeで止める', async target => {
  const { api, client } = renameSetup(); await choose(); const dialog = await open(target === 'root' ? rootTitle : selectedTitle); fill(dialog);
  const read = deferred<FolderChildren>(); const rootRead = deferred<any>();
  api.getRootFolder.mockReturnValue(rootRead.promise); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  submit(dialog); const saved = renameSaved('unknown'); act(() => folderRenameOperations(client).put(saved));
  await act(async () => { read.resolve(page([p, q])); rootRead.resolve({ folderId: rootId, name: 'System Root', revision: 17, capabilities: { createFolder: available } }); });
  expect(api.createFolder).not.toHaveBeenCalled(); expect(folderRenameOperations(client).get()).toBe(saved);
  act(() => { const settled = { ...saved, status: 'rejected' as const }; folderRenameOperations(client).put(settled); folderRenameOperations(client).clearSettled(settled); });
});
test('既知拒否を閉じただけではclearせず明示終了でき、別Qの次操作は通常freshで始める', async () => {
  const { api, client } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog); api.renameFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); submitRename(dialog);
  await within(dialog).findByRole('button', { name: '拒否された操作を確認して終了' }); const saved = folderRenameOperations(client).get(); fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); expect(folderRenameOperations(client).get()).toBe(saved);
  fireEvent.click(screen.getByRole('button', { name: q.name })); fireEvent.click(screen.getByRole('button', { name: renameTitle })); const reopened = await screen.findByRole('dialog');
  fireEvent.click(within(reopened).getByRole('button', { name: '拒否された操作を確認して終了' })); expect(folderRenameOperations(client).get()).toBeUndefined();
  const next = await openRename(); fillRename(next); submitRename(next); await within(next).findByText('フォルダー名を変更しました。'); expect(api.renameFolder.mock.calls[1]![0]).toBe(q.folderId);
});
test('既知拒否の見直しは別Qから保存Pのsourceを使い希望名/理由を保持、次は新operation', async () => {
  const { api } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog); api.renameFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); submitRename(dialog);
  await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' }); const first = api.renameFolder.mock.calls[0]![1];
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); fireEvent.click(screen.getByRole('button', { name: q.name })); fireEvent.click(screen.getByRole('button', { name: renameTitle })); const reopened = await screen.findByRole('dialog');
  const reads: string[] = []; api.listFolderChildren.mockImplementation(id => { reads.push(id); return Promise.resolve(page(id === rootId ? [{ ...p, name: '最新P', revision: 21 }, q] : [])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' })); await waitFor(() => expect(within(reopened).getByLabelText('変更先のフォルダー名')).toBeEnabled());
  expect(reads).toEqual([rootId, p.folderId]); expect(within(reopened).getByLabelText('変更先のフォルダー名')).toHaveValue('変更資料');
  submitRename(reopened); await within(reopened).findByText('フォルダー名を変更しました。'); expect(api.renameFolder.mock.calls[1]).toEqual([p.folderId, expect.objectContaining({ name: '変更資料', expectedFolderRevision: 21 })]);
  expect(api.renameFolder.mock.calls[1]![1].operationId).not.toBe(first.operationId); expect(screen.getByRole('button', { name: q.name, hidden: true })).toHaveAttribute('aria-current', 'location');
});
test('拒否後の同P実再選択だけ新親/page provenanceで見直せる', async () => {
  const { api, client } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog); api.renameFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); submitRename(dialog);
  await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' }); fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const moved = { ...p, parentFolderId: q.folderId, revision: 31 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId] }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  const newParent = screen.getByRole('button', { name: q.name }).closest('li')!;
  fireEvent.click(screen.getByRole('button', { name: `${q.name}の子フォルダーを開く` }));
  fireEvent.click(await within(newParent).findByRole('button', { name: p.name }));
  fireEvent.click(screen.getByRole('button', { name: renameTitle })); const reopened = await screen.findByRole('dialog'); const reads: string[] = [];
  api.listFolderChildren.mockImplementation(id => { reads.push(id); return Promise.resolve(page(id === q.folderId ? [moved] : [])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' })); await waitFor(() => expect(reads).toEqual([q.folderId, p.folderId])); await waitFor(() => expect(within(reopened).getByLabelText('変更先のフォルダー名')).toBeEnabled());
  expect(reads).toEqual([q.folderId, p.folderId]); submitRename(reopened); await within(reopened).findByText('フォルダー名を変更しました。'); expect(api.renameFolder.mock.calls[1]![1].expectedFolderRevision).toBe(31);
});
test.each(['cancel', 'same-selection', 'navigation'] as const)('拒否のreview read中%sは遅延結果で保存要求をclearしない', async action => {
  const { api, client, router } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog); api.renameFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); submitRename(dialog);
  await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' }); const saved = folderRenameOperations(client).get(); const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  if (action === 'cancel') fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  else if (action === 'same-selection') fireEvent.click(screen.getByRole('button', { name: p.name, hidden: true }));
  else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(page([p, q]))); expect(folderRenameOperations(client).get()).toBe(saved); expect(api.renameFolder).toHaveBeenCalledTimes(1);
});
test('遅延成功は現在別Qとcreate成功receiptの送信時表示を保ち、再mount後もP旧copyを復活させない', async () => {
  const { api, client, router, history, invalidate } = renameSetup(); await choose();
  const create = { ...createSaved('pending'), status: 'succeeded' as const, context: { kind: 'selected' as const, folderId: p.folderId, sourceParentId: rootId, pageLimit: 1, name: p.name }, result: success(createSaved('pending').request) };
  act(() => rootFolderOperations(client).put(create)); const dialog = await openRename(); fillRename(dialog); const write = deferred<ReturnType<typeof renameResult>>(); api.renameFolder.mockReturnValueOnce(write.promise); submitRename(dialog);
  await waitFor(() => expect(api.renameFolder).toHaveBeenCalledTimes(1)); const [id, request] = api.renameFolder.mock.calls[0]!;
  fireEvent.click(screen.getByRole('button', { name: q.name, hidden: true })); await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  await screen.findByRole('button', { name: q.name }); fireEvent.click(screen.getByRole('button', { name: q.name })); invalidate.mockRejectedValue(new Error('refresh failed'));
  await act(async () => write.resolve(renameResult(id, request))); expect(rootFolderOperations(client).get()).toBe(create); expect(router.state.location.search.folderId).toBe(q.folderId);
  expect(screen.getByRole('button', { name: q.name })).toHaveAttribute('aria-current', 'location');
  fireEvent.click(screen.getByRole('button', { name: renameTitle })); const reopened = await screen.findByRole('dialog'); expect(within(reopened).getByText('フォルダー名を変更しました。')).toBeVisible(); expect(within(reopened).getByText(/表示を更新できませんでした/)).toBeVisible();
});
function documentRow(folderName: string) {
  return { documentId: 'doc', title: '関連文書', folderId: p.folderId, folderName, revision: 73,
    displayVersion: { versionId: 'version', versionNo: 2, lifecycleState: 'PUBLISHED', isCurrent: true, approvedAt: null, scheduledPublishAt: null, fileSummary: { authoritativeItemCount: 0, primary: null } },
    displayRevision: null, readState: { isRead: true }, displayTimestamp: { kind: 'revisionCreatedAt', value: '2026-10-06T00:00:00Z' } };
}
test('実documentのfolderName/header/panel projectionを再GETで更新しDocument/Version revisionをreceiptでpatchしない', async () => {
  const { api, client } = renameSetup(); let currentName = p.name;
  api.listDocuments.mockImplementation(() => Promise.resolve({ view: 'published', items: [documentRow(currentName)], nextCursor: null }));
  await choose(); const dialog = await openRename(); fillRename(dialog);
  api.renameFolder.mockImplementation((id, body) => { currentName = body.name; return Promise.resolve(renameResult(id, body)); });
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [{ ...p, name: currentName, revision: currentName === p.name ? 8 : 9 }, q] : [])));
  submitRename(dialog); await within(dialog).findByText('フォルダー名を変更しました。');
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' }));
  await screen.findByRole('heading', { name: '変更資料の文書' });
  expect(within(screen.getByRole('complementary', { name: '選択中の文書' })).getByText('変更資料')).toBeVisible();
  const documents = client.getQueriesData<{ items: { revision: number }[] }>({ queryKey: ['documents'] }); expect(documents.every(([, data]) => data?.items[0]?.revision === 73)).toBe(true);
  expect(api.getDocumentVersion).toHaveBeenCalledTimes(1);
});
test('UNKNOWN過去receipt再生後も現在tree名を再readし希望名へ巻き戻さない', async () => {
  const { api, client } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog, '過去希望名');
  api.renameFolder.mockRejectedValueOnce(new Error('lost')); submitRename(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const current = { ...p, name: '後続の外部改名', revision: 51 }; api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [current, q] : [])));
  fireEvent.click(within(dialog).getByRole('button', { name: '同じ内容で再試行' })); await within(dialog).findByText('フォルダー名を変更しました。');
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' })); await screen.findByRole('button', { name: current.name });
  expect(screen.queryByRole('button', { name: '過去希望名' })).not.toBeInTheDocument(); expect(client.getQueryData(['folder-tree', rootId, 'pages'])).toMatchObject({ pages: [{ items: [current, q] }] });
});
test('実refetch errorでも成功receipt/既表示tree/documentを保持しUNKNOWN/空一覧にしない', async () => {
  const { api, client } = renameSetup(); api.listDocuments.mockResolvedValue({ view: 'published', items: [documentRow(p.name)], nextCursor: null }); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.renameFolder.mockImplementation((id, body) => { api.listFolderChildren.mockRejectedValue(new Error('refresh failed')); api.listDocuments.mockRejectedValue(new Error('refresh failed')); return Promise.resolve(renameResult(id, body)); });
  submitRename(dialog); await within(dialog).findByText(/表示を更新できませんでした/); expect(folderRenameOperations(client).get()).toMatchObject({ status: 'succeeded', refresh: 'failed' });
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' })); expect(screen.getByRole('button', { name: p.name })).toBeVisible(); expect(screen.getByRole('button', { name: '関連文書' })).toBeVisible(); expect(screen.queryByText('文書がありません')).not.toBeInTheDocument();
});
test('改名でPが既取得範囲から消えてもURL選択IDを保ち、確認後のfocusは可視Root行に戻る', async () => {
  const { api, router } = renameSetup(); await choose(); const dialog = await openRename(); fillRename(dialog);
  api.renameFolder.mockImplementation((id, body) => { api.listFolderChildren.mockImplementation(readId => Promise.resolve(page(readId === rootId ? [q] : []))); return Promise.resolve(renameResult(id, body)); });
  submitRename(dialog); await within(dialog).findByText('フォルダー名を変更しました。'); await waitFor(() => expect(screen.queryByRole('button', { name: p.name, hidden: true })).not.toBeInTheDocument());
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' })); expect(router.state.location.search.folderId).toBe(p.folderId);
  await waitFor(() => expect(screen.getByRole('button', { name: 'System Root' })).toHaveFocus()); expect(screen.getByRole('button', { name: renameTitle })).toBeDisabled();
});
