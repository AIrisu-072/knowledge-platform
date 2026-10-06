import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { folderMoveOperations } from '../src/application/document-folder-move';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(), renameFolder: jest.fn(), moveFolder: jest.fn(), getDocument: jest.fn(), getDocumentVersion: jest.fn(),
} }));
const rootId = '019a0010-0000-7000-8000-000000000041';
const available = { status: 'available' as const };
const denied = { status: 'disabled' as const, reason: 'permission' as const };
const row = (folderId: string, name: string, parentFolderId = rootId, revision = 8): Folder => ({ folderId, name, parentFolderId, revision });
const g = row('019a0010-0000-7000-8000-000000000100', '元親'); const d = row('019a0010-0000-7000-8000-000000000101', '移動先'); const p = row('019a0010-0000-7000-8000-000000000102', '対象資料', g.folderId); const other = row('019a0010-0000-7000-8000-000000000103', '別資料');
const page = (items: Folder[], nextCursor: string | null = null, moveFolder = available as FolderChildren['capabilities']['moveFolder']): FolderChildren => ({ items, nextCursor, capabilities: { createDocument: available, createFolder: available, renameFolder: available, moveFolder, manageAccess: available } });
const title = '選択したフォルダーを移動';
const confirmation = '継承アクセス設定への影響を確認しました';
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => {
  for (const store of [rootFolderOperations(client), folderRenameOperations(client), folderMoveOperations(client)]) {
    const operation = store.get(); if (operation) { store.put({ ...operation, status: 'succeeded' } as never); store.clearSettled(store.get()! as never); }
  }
  client.clear();
}));
function setup(entry = '/documents?view=published') {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  const rows = new Map([[rootId, [g, d, other]], [g.folderId, [p]]]);
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'System Root', parentFolderId: null, revision: 17, capabilities: { ...page([]).capabilities, moveFolder: denied } });
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(rows.get(id) ?? [])));
  api.listDocuments.mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  api.moveFolder.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: body.expectedFolderRevision + (body.fromParentId === body.toParentId ? 0 : 1), changed: body.fromParentId !== body.toParentId, occurredAt: '2026-10-06T07:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [entry] });
  const router = createRouter({ routeTree: root.addChildren([list, tasks]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, client, router, history, rows };
}
async function choose() {
  fireEvent.click(await screen.findByRole('button', { name: `${g.name}の子フォルダーを開く` }));
  fireEvent.click(await screen.findByRole('button', { name: p.name }));
  await waitFor(() => expect(screen.getByRole('button', { name: title })).toBeEnabled());
}
async function openMove() {
  fireEvent.click(screen.getByRole('button', { name: title }));
  const dialog = await screen.findByRole('dialog', { name: title });
  await waitFor(() => expect(within(dialog).getByLabelText('移動理由')).toBeEnabled());
  return dialog;
}
async function destination(dialog: HTMLElement, name = d.name) {
  fireEvent.click(await within(dialog).findByRole('button', { name }));
  await waitFor(() => expect(within(dialog).getByText(/移動先フォルダーID：/)).toBeVisible());
}
function fill(dialog: HTMLElement) {
  fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: ' 合成理由 ' } });
  fireEvent.click(within(dialog).getByLabelText(confirmation));
}
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '移動する' }).closest('form')!); }

// Removing the actual ordinary-navigation entry must fail this test.
test('通常の実選択対象と独立した移動先ツリーを示し、暗黙Rootと未確認送信を避ける', async () => {
  setup(); await choose(); const dialog = await openMove();
  expect(within(dialog).getByText(`対象フォルダーID：${p.folderId}`)).toBeVisible();
  expect(within(dialog).getByText(`現在の親ID：${g.folderId}`)).toBeVisible();
  expect(within(dialog).queryByText(/移動先フォルダーID：/)).not.toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '移動する' })).toBeDisabled();
  await destination(dialog); fireEvent.change(within(dialog).getByLabelText('移動理由'), { target: { value: '理由' } });
  expect(within(dialog).getByRole('button', { name: '移動する' })).toBeDisabled();
  expect(within(dialog).getByText(/配下や自分の閲覧・編集権限が変わる/)).toBeVisible();
});
test.each([['System Root', rootId, true], [g.name, g.folderId, false]])('Root move hintに依存せず%sを選択し、同親もPOSTしてreceiptを確認する', async (name, toParentId, changed) => {
  const { api, router } = setup('/documents?view=published&titleContains=条件'); await choose(); const dialog = await openMove(); await destination(dialog, name); fill(dialog); submit(dialog);
  await within(dialog).findByText(changed ? 'フォルダーを移動しました。' : '親の変更はありませんでした。');
  const [id, body] = api.moveFolder.mock.calls[0]!;
  expect(id).toBe(p.folderId); expect(body).toMatchObject({ fromParentId: g.folderId, toParentId, expectedFolderRevision: 8, reason: '合成理由' });
  expect(Object.keys(body).sort()).toEqual(['expectedFolderRevision', 'fromParentId', 'operationId', 'reason', 'toParentId']);
  expect(router.state.location.search).toMatchObject({ folderId: p.folderId, titleContains: '条件' });
});
test('Root自身と直URL対象は移動入口を許可しない', async () => {
  const { router } = setup(); await screen.findByRole('button', { name: 'System Root' }); expect(screen.getByRole('button', { name: title })).toBeDisabled();
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ folderId: p.folderId }) }); });
  expect(screen.getByRole('button', { name: title })).toBeDisabled();
});
test('対象自身の現在hintがdisabledなら元親hintを流用しない', async () => {
  const { api } = setup(); api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [g, d] : id === g.folderId ? [p] : [], null, id === p.folderId ? denied : available)));
  fireEvent.click(await screen.findByRole('button', { name: `${g.name}の子フォルダーを開く` })); fireEvent.click(await screen.findByRole('button', { name: p.name }));
  await waitFor(() => expect(screen.getByRole('button', { name: title })).toBeDisabled());
});
test('送信readとPOSTの連打でoperationを一度だけ生成して送る', async () => {
  const { api } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === g.folderId ? read.promise : Promise.resolve(page(id === rootId ? [g, d] : [])));
  act(() => { submit(dialog); submit(dialog); }); await act(async () => read.resolve(page([p])));
  await within(dialog).findByText('フォルダーを移動しました。'); expect(api.moveFolder).toHaveBeenCalledTimes(1);
});
test.each(['cancel', 'same-selection', 'other-selection', 'back', 'navigation', 'filter'])('送信前の遅延source read後に%sしても送らない', async action => {
  const { api, router, history } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === g.folderId ? read.promise : Promise.resolve(page(id === rootId ? [g, d] : [])));
  submit(dialog);
  if (action === 'cancel') fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  else if (action === 'same-selection' || action === 'other-selection') fireEvent.click(screen.getAllByRole('button', { name: action === 'same-selection' ? p.name : other.name, hidden: true })[0]!);
  else if (action === 'back') await act(async () => history.back());
  else if (action === 'filter') await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ ...router.state.location.search, titleContains: '別' }) }); });
  else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(page([p]))); expect(api.moveFolder).not.toHaveBeenCalled();
});
test('遅い移動先readは新しい移動先選択を上書きしない', async () => {
  const { api } = setup(); await choose(); const dialog = await openMove();
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page(id === g.folderId ? [p] : [])));
  fireEvent.click(within(dialog).getByRole('button', { name: d.name }));
  fireEvent.click(within(dialog).getByRole('button', { name: 'System Root' }));
  await within(dialog).findByText(`移動先フォルダーID：${rootId}`); await act(async () => read.resolve(page([g, d])));
  expect(within(dialog).getByText(`移動先フォルダーID：${rootId}`)).toBeVisible(); fill(dialog); submit(dialog);
  await within(dialog).findByText('フォルダーを移動しました。'); expect(api.moveFolder.mock.calls[0]![1].toParentId).toBe(rootId);
});
test.each(['rename', 'revision', 'missing', 'moved'])('対象%sをfresh検出し理由を保持して明示見直しへ止める', async change => {
  const { api, rows } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  rows.set(g.folderId, change === 'missing' || change === 'moved' ? [] : [{ ...p, name: change === 'rename' ? '外部改名' : p.name, revision: 19 }]);
  if (change === 'moved') rows.set(d.folderId, [{ ...p, parentFolderId: d.folderId }]);
  submit(dialog); await within(dialog).findByText(change === 'rename' || change === 'revision' ? /対象の名前またはrevisionが変わりました/ : /元の親の取得済み範囲を確認し/); expect(api.moveFolder).not.toHaveBeenCalled();
  expect(within(dialog).getByLabelText('移動理由')).toHaveValue(' 合成理由 '); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked();
  if (change === 'rename' || change === 'revision') {
    fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '移動する' })).toBeDisabled());
    fireEvent.click(within(dialog).getByLabelText(confirmation)); submit(dialog); await within(dialog).findByText('フォルダーを移動しました。');
    expect(api.moveFolder.mock.calls[0]![1].expectedFolderRevision).toBe(19);
  }
});
test('移動先の改名を送信前に検出して再確認まで止める', async () => {
  const { api, rows } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  rows.set(rootId, [g, { ...d, name: '新移動先名' }, other]); submit(dialog);
  await within(dialog).findByText(/移動先の状態が変わりました/); expect(api.moveFolder).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await within(dialog).findByText(/^移動先名：新移動先名$/);
  await within(dialog).findByRole('button', { name: '移動する' }); fireEvent.click(within(dialog).getByLabelText(confirmation)); submit(dialog);
  await within(dialog).findByText('フォルダーを移動しました。');
});
test('UNKNOWNは画面往復しても元path/bodyで再送し再送403も未確定のまま保持する', async () => {
  const { api, router, client } = setup(); api.moveFolder.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403));
  await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog); submit(dialog); await within(dialog).findByText('移動結果を確認できません');
  const first = api.moveFolder.mock.calls[0]!;
  expect(screen.getByRole('button', { name: 'System Rootにフォルダーを作成', hidden: true })).toBeDisabled();
  expect(screen.getByRole('button', { name: '選択したフォルダー名を変更', hidden: true })).toBeDisabled();
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  expect(window.dispatchEvent(new Event('beforeunload', { cancelable: true }))).toBe(false);
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({}) }); });
  fireEvent.click(await screen.findByRole('button', { name: title })); const reopened = await screen.findByRole('dialog'); fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' }));
  await within(reopened).findByText(/FORBIDDEN.*初回の移動結果は未確定/); expect(api.moveFolder.mock.calls[1]![0]).toBe(first[0]); expect(api.moveFolder.mock.calls[1]![1]).toBe(first[1]);
  expect(client.getQueryCache().getAll().length).toBeGreaterThan(0);
});
test.each(['create', 'rename'])('%sの未解決要求がfresh read中に発生しても新moveを送らない', async kind => {
  const { api, client } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === g.folderId ? read.promise : Promise.resolve(page(id === rootId ? [g, d] : []))); submit(dialog);
  act(() => { if (kind === 'create') rootFolderOperations(client).put({ status: 'unknown', request: { operationId: 'fixed', folderId: 'new', parentFolderId: rootId, expectedParentRevision: 17, name: '新', reason: '理由' } }); else folderRenameOperations(client).put({ status: 'unknown', targetFolderId: other.folderId, currentName: other.name, expectedChanged: true, context: { kind: 'selected', folderId: other.folderId, sourceParentId: rootId, pageLimit: 1, name: other.name }, request: { operationId: 'fixed', expectedFolderRevision: 8, name: '新', reason: '理由' } }); });
  await act(async () => read.resolve(page([p]))); expect(api.moveFolder).not.toHaveBeenCalled();
});
test('成功後はglobal READを取り直し選択根拠を無効にしても新navigationを戻さずread失敗を別表示する', async () => {
  const { api, router } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  api.moveFolder.mockImplementation(async (id, body) => { api.getRootFolder.mockRejectedValue(problem('FORBIDDEN', 403)); return { operationId: body.operationId, resourceId: id, resultingRevision: 9, changed: true, occurredAt: '2026-10-06T07:00:00Z' }; });
  submit(dialog); await within(dialog).findByText('フォルダーを移動しました。'); await within(dialog).findByText(/表示を更新できませんでした/);
  expect(router.state.location.search.folderId).toBe(p.folderId);
});
test('移動後に対象が閉じた移動先配下へ隠れても確認後のfocusは可視Root行へ戻る', async () => {
  const { api, client, router, rows } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  api.moveFolder.mockImplementation(async (id, body) => {
    rows.set(g.folderId, []); rows.set(d.folderId, [{ ...p, parentFolderId: d.folderId, revision: 9 }]);
    return { operationId: body.operationId, resourceId: id, changed: true, resultingRevision: 9, occurredAt: '2026-10-06T07:00:00Z' };
  });
  submit(dialog); await within(dialog).findByText('フォルダーを移動しました。');
  await waitFor(() => expect(folderMoveOperations(client).get()?.refresh).toBe('complete'));
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name, hidden: true })).not.toBeInTheDocument());
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' }));
  expect(router.state.location.search.folderId).toBe(p.folderId); expect(screen.getByRole('button', { name: title })).toBeDisabled();
  await waitFor(() => expect(screen.getByRole('button', { name: 'System Root' })).toHaveFocus());
});
test('送信後の別選択とURLを成功receiptや遅延refreshで元対象へ戻さない', async () => {
  const { api, router, client } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  const response = deferred<{ operationId: string; resourceId: string; changed: boolean; resultingRevision: number; occurredAt: string }>(); api.moveFolder.mockReturnValue(response.promise); submit(dialog);
  await waitFor(() => expect(api.moveFolder).toHaveBeenCalledTimes(1)); const body = api.moveFolder.mock.calls[0]![1];
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ folderId: other.folderId, titleContains: '新条件' }) }); });
  await act(async () => response.resolve({ operationId: body.operationId, resourceId: p.folderId, changed: true, resultingRevision: 9, occurredAt: '2026-10-06T07:00:00Z' }));
  await waitFor(() => expect(folderMoveOperations(client).get()?.refresh).toBe('complete'));
  expect(router.state.location.search).toMatchObject({ folderId: other.folderId, titleContains: '新条件' });
  expect(screen.getByRole('button', { name: '選択したフォルダー名を変更' })).toBeDisabled();
});
test('旧Document URL cursorは現在readのstaleを明示し条件の再適用まで保持する', async () => {
  const { api, router } = setup(); await choose();
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ ...router.state.location.search, cursor: 'old-opaque', titleContains: '条件' }) }); });
  const dialog = await openMove(); await destination(dialog); fill(dialog);
  api.moveFolder.mockImplementation(async (id, body) => { api.listDocuments.mockImplementation(query => query.cursor ? Promise.reject(problem('CURSOR_STALE', 409)) : Promise.resolve({ view: 'published', items: [], nextCursor: null })); return { operationId: body.operationId, resourceId: id, changed: true, resultingRevision: 9, occurredAt: '2026-10-06T07:00:00Z' }; });
  submit(dialog); await within(dialog).findByText('フォルダーを移動しました。'); await within(dialog).findByText(/表示を更新できませんでした/);
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' }));
  await screen.findByText('一覧が更新されています。条件を再適用してください。'); expect(router.state.location.search).toMatchObject({ folderId: p.folderId, cursor: 'old-opaque', titleContains: '条件' });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' })); await waitFor(() => expect(router.state.location.search.cursor).toBeUndefined()); expect(router.state.location.search.folderId).toBe(p.folderId);
});
test.each(['create', 'rename'])('moveが未確定でも既存%s UNKNOWNの同一要求回復は利用できる', async kind => {
  const { api, client } = setup(); await choose();
  const moveStore = folderMoveOperations(client);
  act(() => {
    moveStore.put({ status: 'unknown', targetFolderId: p.folderId, context: { kind: 'selected', folderId: p.folderId, sourceParentId: g.folderId, pageLimit: 1, name: p.name }, destination: { kind: 'root', folderId: rootId, name: 'System Root' }, currentName: p.name, expectedChanged: true, request: { operationId: 'move', fromParentId: g.folderId, toParentId: rootId, expectedFolderRevision: 8, reason: '理由' } });
    if (kind === 'create') rootFolderOperations(client).put({ status: 'unknown', request: { operationId: 'create', folderId: 'new', parentFolderId: rootId, expectedParentRevision: 17, name: '新', reason: '理由' } });
    else folderRenameOperations(client).put({ status: 'unknown', targetFolderId: p.folderId, context: { kind: 'selected', folderId: p.folderId, sourceParentId: g.folderId, pageLimit: 1, name: p.name }, currentName: p.name, expectedChanged: true, request: { operationId: 'rename', expectedFolderRevision: 8, name: '新', reason: '理由' } });
  });
  const store = kind === 'create' ? rootFolderOperations(client) : folderRenameOperations(client); const fixed = store.get()!.request;
  const method = kind === 'create' ? api.createFolder : api.renameFolder; method.mockRejectedValue(problem('FORBIDDEN', 403));
  fireEvent.click(screen.getByRole('button', { name: kind === 'create' ? 'System Rootにフォルダーを作成' : '選択したフォルダー名を変更' })); const dialog = await screen.findByRole('dialog');
  fireEvent.click(within(dialog).getByRole('button', { name: '同じ内容で再試行' })); await within(dialog).findByText(/FORBIDDEN.*初回/);
  expect(method.mock.calls[0]![kind === 'create' ? 0 : 1]).toBe(fixed); expect(store.get()?.status).toBe('unknown');
});
test('missing対象を実ツリーで移動先の新parentから再選択したとき理由を保持する', async () => {
  const { api, rows, client } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  rows.set(g.folderId, []); rows.set(d.folderId, [{ ...p, parentFolderId: d.folderId }]); submit(dialog); await within(dialog).findByText(/元の親の取得済み範囲を確認し/);
  fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', g.folderId] }); });
  fireEvent.click(screen.getByRole('button', { name: `${d.name}の子フォルダーを開く` }));
  const destinationRow = screen.getByRole('button', { name: d.name }).closest('li')!;
  fireEvent.click(await within(destinationRow).findByRole('button', { name: p.name }));
  await waitFor(() => expect(screen.getByRole('button', { name: title })).toBeEnabled()); const reopened = await openMove();
  expect(within(reopened).getByLabelText('移動理由')).toHaveValue(' 合成理由 '); expect(within(reopened).getByText(`現在の親ID：${d.folderId}`)).toBeVisible();
  expect(api.moveFolder).not.toHaveBeenCalled(); expect(within(reopened).queryByText(/移動先フォルダーID：/)).not.toBeInTheDocument();
});
test.each(['create', 'rename'])('新%sのfresh read中にmoveが未確定になったら送信直前に止める', async kind => {
  const { api, client } = setup(); await choose();
  const entry = kind === 'create' ? 'System Rootにフォルダーを作成' : '選択したフォルダー名を変更'; fireEvent.click(screen.getByRole('button', { name: entry })); const dialog = await screen.findByRole('dialog');
  const nameLabel = kind === 'create' ? 'フォルダー名' : '変更先のフォルダー名'; await waitFor(() => expect(within(dialog).getByLabelText(nameLabel)).toBeEnabled());
  fireEvent.change(within(dialog).getByLabelText(nameLabel), { target: { value: '新資料' } }); fireEvent.change(within(dialog).getByLabelText(kind === 'create' ? '作成理由' : '変更理由'), { target: { value: '理由' } });
  const read = deferred<FolderChildren>(); const rootRead = deferred<Awaited<ReturnType<typeof documentApi.getRootFolder>>>();
  if (kind === 'create') api.getRootFolder.mockReturnValue(rootRead.promise); else api.listFolderChildren.mockImplementation(id => id === g.folderId ? read.promise : Promise.resolve(page([])));
  fireEvent.submit(within(dialog).getByRole('button', { name: kind === 'create' ? '作成する' : '変更を保存する' }).closest('form')!);
  act(() => folderMoveOperations(client).put({ status: 'unknown', targetFolderId: p.folderId, context: { kind: 'selected', folderId: p.folderId, sourceParentId: g.folderId, pageLimit: 1, name: p.name }, destination: { kind: 'root', folderId: rootId, name: 'System Root' }, currentName: p.name, expectedChanged: true, request: { operationId: 'move', fromParentId: g.folderId, toParentId: rootId, expectedFolderRevision: 8, reason: '理由' } }));
  await act(async () => { if (kind === 'create') rootRead.resolve({ folderId: rootId, name: 'System Root', parentFolderId: null, revision: 17, capabilities: page([]).capabilities }); else read.resolve(page([p])); });
  expect(kind === 'create' ? api.createFolder : api.renameFolder).not.toHaveBeenCalled();
});
test('開いた時点で改名された対象は現在readを黙って基準にせず明示見直しする', async () => {
  const { api, rows } = setup(); await choose(); rows.set(g.folderId, [{ ...p, name: '開く前の改名', revision: 10 }]);
  const dialog = await openMove(); await within(dialog).findByText(/対象の名前またはrevisionが変わりました/); await destination(dialog);
  expect(within(dialog).queryByRole('button', { name: '移動する' })).not.toBeInTheDocument(); expect(api.moveFolder).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByRole('button', { name: '移動する' }); fill(dialog); submit(dialog);
  await within(dialog).findByText('フォルダーを移動しました。'); expect(api.moveFolder.mock.calls[0]![1].expectedFolderRevision).toBe(10);
});
test('移動先をfresh確認中に古い選択を使った見直しを並行開始しない', async () => {
  const { api, rows } = setup(); await choose(); rows.set(g.folderId, [{ ...p, name: '外部改名', revision: 10 }]); const dialog = await openMove();
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page(id === g.folderId ? rows.get(id)! : [])));
  fireEvent.click(within(dialog).getByRole('button', { name: d.name }));
  expect(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })).toBeDisabled();
  await act(async () => read.resolve(page([g, d])));
});
test('移動先変更とsubmitが同じevent batchでも旧移動先と旧確認状態で送らない', async () => {
  const { api } = setup(); await choose(); const dialog = await openMove(); await destination(dialog); fill(dialog);
  const read = deferred<Awaited<ReturnType<typeof documentApi.getRootFolder>>>(); api.getRootFolder.mockReturnValue(read.promise);
  act(() => { fireEvent.click(within(dialog).getByRole('button', { name: 'System Root' })); submit(dialog); });
  await act(async () => read.resolve({ folderId: rootId, name: 'System Root', parentFolderId: null, revision: 17, capabilities: page([]).capabilities }));
  expect(api.moveFolder).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText(confirmation)).not.toBeChecked();
});
