import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(),
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

// Removing the selected-parent entry, using Root's revision, or substituting child revision fails this test.
test('ツリーで選択した非root自身のfresh row revisionとcapabilityで作成する', async () => {
  const { api, client, invalidate, router } = setup();
  await choose(); const dialog = await open(); fill(dialog);
  expect(within(dialog).getByText(/登録先：資料100/)).toHaveTextContent(p.folderId);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([{ ...p, revision: 29 }, q]) : page([folder(200, p.folderId, 0)])));
  submit(dialog); await within(dialog).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls[0]![0]).toMatchObject({ parentFolderId: p.folderId, expectedParentRevision: 29, name: '新資料', reason: '合成理由' });
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['folder-tree', p.folderId] });
  expect(client.getQueryData(['folder-tree', p.folderId])).toEqual(page([folder(200, p.folderId, 0)]));
  expect(client.getQueryData(['folder-tree', 'root'])).toMatchObject({ revision: 17 });
  expect(router.state.location.search.folderId).toBe(p.folderId);
});

test('2ページ目を選択して折り畳んでも先頭からfresh opaque cursorだけを順次読んで作成する', async () => {
  const { api, client } = setup(); const oldCursor = 'OLD+/=?'; const freshCursor = '新+/=?opaque';
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id !== rootId ? page([]) : cursor ? page([p]) : page([q], oldCursor)));
  await screen.findByRole('button', { name: q.name });
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーをさらに表示' }));
  await choose(); fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーを閉じる' }));
  const dialog = await open(); fill(dialog); const reads: [string, string | undefined][] = [];
  api.listFolderChildren.mockImplementation((id, cursor) => { reads.push([id, cursor]); return Promise.resolve(id !== rootId ? page([]) : cursor === freshCursor ? page([{ ...p, revision: 33 }]) : page([], freshCursor)); });
  submit(dialog); await within(dialog).findByText('フォルダーを作成しました。');
  expect(reads.slice(0, 3)).toEqual([[rootId, undefined], [rootId, freshCursor], [p.folderId, undefined]]);
  expect(reads).not.toContainEqual([rootId, oldCursor]);
  expect(api.createFolder.mock.calls[0]![0]).toMatchObject({ parentFolderId: p.folderId, expectedParentRevision: 33 });
  expect(client.getQueryData(['folder-tree', rootId, 'pages'])).toMatchObject({ pageParams: [undefined, oldCursor] });
});

test('fresh chainの後の重複行を採用し、上限を選択時のページ数から増やさない', async () => {
  const { api } = setup(); api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id !== rootId ? page([]) : cursor ? page([p]) : page([q], 'old')));
  await screen.findByRole('button', { name: q.name }); fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーをさらに表示' }));
  await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id !== rootId ? page([]) : cursor === 'fresh' ? page([{ ...p, revision: 41 }], 'beyond-limit') : page([{ ...p, revision: 40 }], 'fresh')));
  submit(dialog); await within(dialog).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls[0]![0].expectedParentRevision).toBe(41);
  expect(api.listFolderChildren).not.toHaveBeenCalledWith(rootId, 'beyond-limit');
});

test.each([
  ['未発見', page([q])], ['上限外', page([q], 'beyond-limit')],
  ['親変更', page([{ ...p, parentFolderId: q.folderId }])], ['null親', page([{ ...p, parentFolderId: null }])],
  ['不正revision', page([{ ...p, revision: -1 }])],
  ['現在403', problem('FORBIDDEN', 403)], ['現在404', problem('FOLDER_NOT_FOUND', 404)],
] as const)('fresh source %sではcached選択を使わずPOSTを停止して再選択を案内する', async (_case, response) => {
  const { api } = setup(); await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation(id => id === rootId && !('items' in response) ? Promise.reject(response) : Promise.resolve(id === rootId ? response : page([])));
  submit(dialog); await within(dialog).findByText(/ツリーで選び直してください/);
  expect(api.createFolder).not.toHaveBeenCalled(); expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled();
});

test('先頭で対象が見つかっても後続read失敗ならPOSTしない', async () => {
  const { api } = setup(); api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id !== rootId ? page([]) : cursor ? page([q]) : page([p], 'old')));
  await screen.findByRole('button', { name: p.name }); fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーをさらに表示' }));
  await screen.findByRole('button', { name: q.name }); await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation((id, cursor) => id === rootId && cursor ? Promise.reject(new Error('later read failed')) : Promise.resolve(id === rootId ? page([p], 'fresh') : page([])));
  submit(dialog); await within(dialog).findByText(/ツリーで選び直してください/); expect(api.createFolder).not.toHaveBeenCalled();
});

test.each(['permission', 'lifecycle', 'notHumanInteractive', 'unsupported'] as const)('対象自身の現在capability disabled:%sを表示してRoot/その親のavailableで補わない', async reason => {
  const { api } = setup(); await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([p, q]) : page([], null, { status: 'disabled', reason })));
  submit(dialog); await within(dialog).findByRole('alert'); expect(api.createFolder).not.toHaveBeenCalled();
  expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled();
  expect(within(dialog).getByRole('alert').textContent).not.toMatch(/System Root/);
});

test('直URLとroute再mountはrowの読取根拠がなく、通常一覧/登録を保って再選択を案内する', async () => {
  const { api, router, history } = setup(`/documents?view=published&folderId=${p.folderId}`);
  await screen.findByRole('button', { name: p.name });
  expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled();
  expect(screen.getByText(/ツリーで選び直してください/)).toBeVisible(); expect(screen.getByRole('button', { name: '文書を登録' })).toBeEnabled();
  await choose(); await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => history.back()); await screen.findByRole('button', { name: p.name });
  expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled(); expect(api.createFolder).not.toHaveBeenCalled();
});

test.each(['cancel', 'selection', 'navigation', 'back'] as const)('新規fresh read中の%sで遅延完了からPOSTしない', async action => {
  const { api, router, history } = setup(); await choose(); const dialog = await open(); fill(dialog);
  const read = deferred<FolderChildren>(); api.listFolderChildren.mockImplementation(id => id === rootId ? read.promise : Promise.resolve(page([])));
  act(() => { submit(dialog); submit(dialog); }); expect(api.createFolder).not.toHaveBeenCalled();
  if (action === 'cancel') fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  else if (action === 'selection') fireEvent.click(screen.getByRole('button', { name: q.name, hidden: true }));
  else if (action === 'back') await act(async () => history.back());
  else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(page([{ ...p, revision: 71 }])));
  expect(api.createFolder).not.toHaveBeenCalled();
});

test.each(['root-to-selected', 'selected-to-root'] as const)('unknown %sは同じ共有storeで作成先名/IDと全固定要求を保持してread無しで再送する', async direction => {
  const { api, router, history } = setup(); await choose();
  const initialTitle = direction === 'root-to-selected' ? rootTitle : selectedTitle;
  const nextTitle = direction === 'root-to-selected' ? selectedTitle : rootTitle;
  api.createFolder.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockRejectedValueOnce(problem('FOLDER_NOT_FOUND', 404));
  const dialog = await open(initialTitle); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const request = api.createFolder.mock.calls[0]![0];
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); await choose(q);
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  const reopened = await open(nextTitle);
  expect(within(reopened).getByText(/登録先：/)).toHaveTextContent(direction === 'root-to-selected' ? 'System Root直下' : p.name);
  if (direction === 'selected-to-root') { expect(within(reopened).getByText(/登録先：/)).toHaveTextContent(p.folderId); expect(within(reopened).getByText(/登録先：/)).not.toHaveTextContent('System Root'); }
  api.listFolderChildren.mockRejectedValue(new Error('all reads failed')); api.getRootFolder.mockRejectedValue(new Error('root read failed'));
  const count = api.listFolderChildren.mock.calls.length;
  for (let i = 0; i < 3; i++) {
    fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' }));
    await within(reopened).findByRole('button', { name: i === 2 ? '確認して閉じる' : '同じ内容で再試行' });
    if (i < 2) expect(within(reopened).getByText(/初回の作成結果は未確定/)).toBeVisible();
  }
  expect(api.createFolder).toHaveBeenCalledTimes(4); for (const [body] of api.createFolder.mock.calls) expect(body).toBe(request);
  // Only post-success invalidation may read again; retry must not require successful reads.
  expect(api.listFolderChildren.mock.calls.slice(count).every(([id]) => id === q.folderId || id === rootId)).toBe(true);
});

test('初回確定拒否の見直しは別親選択/再mount後も保存済みsourceを使い、次の作成は元親のfresh revision', async () => {
  const { api, router, history } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const first = api.createFolder.mock.calls[0]![0]; fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); await choose(q);
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  const reopened = await open(rootTitle); const reads: string[] = [];
  api.listFolderChildren.mockImplementation(id => { reads.push(id); return Promise.resolve(id === rootId ? page([{ ...p, revision: 51 }, { ...q, revision: 999 }]) : page([])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(reopened).getByLabelText('フォルダー名')).toBeEnabled());
  expect(reads).toEqual([rootId, p.folderId]); fill(reopened, '次資料'); submit(reopened);
  await within(reopened).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls[1]![0]).toMatchObject({ parentFolderId: p.folderId, expectedParentRevision: 51, name: '次資料' });
  expect(api.createFolder.mock.calls[1]![0].operationId).not.toBe(first.operationId);
});

test('selected pendingは連打/navigation中も保持し、元親へ遅延成功/refresh failure後にreceipt確認してから新操作を始める', async () => {
  const { api, router, history, invalidate } = setup(); await choose(); const dialog = await open(); fill(dialog); const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValueOnce(write.promise);
  act(() => { submit(dialog); submit(dialog); }); await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1)); const first = api.createFolder.mock.calls[0]![0];
  expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toBeDisabled(); const event = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(event); expect(event.defaultPrevented).toBe(true);
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); invalidate.mockRejectedValue(new Error('refresh failure'));
  await act(async () => write.resolve(success(first))); expect(router.state.location.pathname).toBe('/tasks');
  await act(async () => history.back()); const reopened = await open(rootTitle); expect(within(reopened).getByText('フォルダーを作成しました。')).toBeVisible();
  expect(within(reopened).getByText(/登録先：/)).toHaveTextContent(p.name);
  fireEvent.click(within(reopened).getByRole('button', { name: '確認して閉じる' })); await choose(q); const next = await open(); fill(next); submit(next);
  await within(next).findByText('フォルダーを作成しました。'); expect(api.createFolder.mock.calls[1]![0].parentFolderId).toBe(q.folderId);
});

test('確認できなかった選択はdialog再表示や一覧navigationで再利用せず、実際のツリー再選択後だけ入力へ戻る', async () => {
  const { api, router } = setup(); await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([q]) : page([])));
  submit(dialog); await within(dialog).findByText(/ツリーで選び直してください/);
  fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled();
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ ...router.state.location.search, titleContains: '条件' }) }); });
  expect(screen.getByRole('button', { name: selectedTitle })).toBeDisabled();
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([p, q]) : page([])));
  await choose(); const again = await open(); fill(again); submit(again); await within(again).findByText('フォルダーを作成しました。');
});

test.each([problem('FORBIDDEN', 403), problem('FOLDER_NOT_FOUND', 404)])('自身capabilityの現在read拒否%pではRootのcapabilityやcached rowで送らない', async error => {
  const { api } = setup(); await choose(); const dialog = await open(); fill(dialog);
  api.listFolderChildren.mockImplementation(id => id === rootId ? Promise.resolve(page([p, q])) : Promise.reject(error));
  submit(dialog); await within(dialog).findByText(/ツリーで選び直してください/); expect(api.createFolder).not.toHaveBeenCalled();
});

test('キャンセルした古いfresh readが別親のpending要求を変更/消去しない', async () => {
  const { api } = setup(); await choose(); const dialog = await open(); fill(dialog);
  const oldRead = deferred<FolderChildren>(); api.listFolderChildren.mockImplementationOnce(() => oldRead.promise); submit(dialog);
  fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' })); await choose(q);
  const next = await open(); fill(next, '新しい入力'); const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValueOnce(write.promise); submit(next);
  await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1)); const body = api.createFolder.mock.calls[0]![0];
  await act(async () => oldRead.resolve(page([{ ...p, revision: 90 }, q])));
  expect(api.createFolder).toHaveBeenCalledTimes(1); expect(within(next).getByLabelText('フォルダー名')).toHaveValue('新しい入力');
  await act(async () => write.resolve(success(body))); await within(next).findByText('フォルダーを作成しました。');
  expect(body.parentFolderId).toBe(q.folderId);
});

test('拒否の見直し失敗/古い完了は元要求を保持し、次のpendingをclearしない', async () => {
  const { api } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const first = api.createFolder.mock.calls[0]![0]; api.listFolderChildren.mockRejectedValueOnce(Error('read failed'));
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByText(/ツリーで選び直してください/);
  expect(within(dialog).getByLabelText('フォルダー名')).toBeDisabled(); expect(within(dialog).getByText(/操作ID：/)).toHaveTextContent(first.operationId);
  const oldRead = deferred<FolderChildren>(); api.listFolderChildren.mockReturnValueOnce(oldRead.promise);
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); const again = await open(rootTitle);
  fireEvent.click(within(again).getByRole('button', { name: '最新の状態を取得して見直す' })); await waitFor(() => expect(within(again).getByLabelText('フォルダー名')).toBeEnabled());
  const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValueOnce(write.promise); fill(again, '第二要求'); submit(again);
  await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(2)); const second = api.createFolder.mock.calls[1]![0];
  await act(async () => oldRead.resolve(page([{ ...p, revision: 99 }, q])));
  expect(within(again).getByText('作成結果を確認しています…')).toBeVisible(); expect(within(again).getByLabelText('フォルダー名')).toHaveValue('第二要求');
  await act(async () => write.resolve(success(second))); await within(again).findByText('フォルダーを作成しました。');
});

test('selected receiptの不一致は別navigation後もunknownとなり、read無しの同一再送でのみ解決する', async () => {
  const { api, router, history } = setup(); await choose(); const dialog = await open(); fill(dialog);
  api.createFolder.mockImplementationOnce(body => Promise.resolve({ ...success(body), resourceId: q.folderId }));
  submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' }); const first = api.createFolder.mock.calls[0]![0];
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back());
  const reopened = await open(rootTitle); expect(within(reopened).getByText(/登録先：/)).toHaveTextContent(p.name);
  api.listFolderChildren.mockRejectedValue(Error('read failed')); fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' }));
  await within(reopened).findByText('フォルダーを作成しました。'); expect(api.createFolder.mock.calls[1]![0]).toBe(first);
});

test('選択自身の初回disabled理由を表示し、Rootがavailableでも入口を無効にする', async () => {
  const { api } = setup(); api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([p, q]) : page([], null, denied)));
  fireEvent.click(await screen.findByRole('button', { name: p.name }));
  const entry = await screen.findByRole('button', { name: selectedTitle }); await waitFor(() => expect(entry).toBeDisabled());
  expect(document.getElementById(entry.getAttribute('aria-describedby')!)?.textContent).toContain('権限がありません');
  expect(screen.getByRole('button', { name: rootTitle })).toBeEnabled(); expect(api.createFolder).not.toHaveBeenCalled();
});

test('深いツリーのsource親/Rootがcreate不可でも対象自身の現在capabilityだけを使う', async () => {
  const { api } = setup(); const source = folder(300); const nested = { ...p, parentFolderId: source.folderId };
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'System Root', revision: 17, parentFolderId: null, capabilities: { createFolder: denied, createDocument: available } });
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([source], null, denied) : id === source.folderId ? page([nested], null, denied) : page([])));
  await screen.findByRole('button', { name: source.name }); fireEvent.click(screen.getByRole('button', { name: `${source.name}の子フォルダーを開く` }));
  await choose(nested); const dialog = await open(); fill(dialog); const reads: string[] = [];
  api.listFolderChildren.mockImplementation(id => { reads.push(id); return Promise.resolve(id === source.folderId ? page([{ ...nested, revision: 42 }], null, denied) : page([])); });
  submit(dialog); await within(dialog).findByText('フォルダーを作成しました。');
  expect(reads.slice(0, 2)).toEqual([source.folderId, nested.folderId]); expect(api.createFolder.mock.calls[0]![0]).toMatchObject({ parentFolderId: nested.folderId, expectedParentRevision: 42 });
});


test('context追加前の既存Root operation形もselected入口から元のRoot宛てとして表示する', async () => {
  const { client, api } = setup();
  act(() => rootFolderOperations(client).put({ status: 'unknown', request: {
    operationId: p.folderId, folderId: q.folderId, parentFolderId: rootId, expectedParentRevision: 17, name: '旧Root要求', reason: '合成理由',
  } }));
  await choose(); const dialog = await open();
  expect(screen.getByRole('dialog', { name: rootTitle })).toBe(dialog);
  expect(within(dialog).getByText(/登録先：System Root直下/)).toBeVisible();
  expect(within(dialog).getByText(/作成先フォルダーID：/)).toHaveTextContent(rootId); expect(api.createFolder).not.toHaveBeenCalled();
});

// R1: a definitive rejection may re-read new provenance only after the same stable target is really reselected.
test('R1 移動した同じ作成先を実ツリー再選択すると新sourceをfresh readして確定拒否から新操作へ戻る', async () => {
  const { api, client } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const saved = rootFolderOperations(client).get()!; const first = api.createFolder.mock.calls[0]![0];
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const moved = { ...p, parentFolderId: q.folderId, revision: 39 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId, 'pages'], exact: true }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole('button', { name: `${q.name}の子フォルダーを開く` })); await choose(moved);
  const reopened = await open(rootTitle); const reads: string[] = [];
  api.listFolderChildren.mockImplementation(id => { reads.push(id); expect(rootFolderOperations(client).get()).toBe(saved); return Promise.resolve(page(id === q.folderId ? [moved] : [])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(reopened).getByLabelText('フォルダー名')).toBeEnabled());
  expect(reads).toEqual([q.folderId, p.folderId]); expect(saved.context).toMatchObject({ sourceParentId: rootId, pageLimit: 1 }); expect(rootFolderOperations(client).get()).toBeUndefined();
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === q.folderId ? [moved] : [])));
  fill(reopened, '移動後の子'); submit(reopened); await within(reopened).findByText('フォルダーを作成しました。');
  const next = api.createFolder.mock.calls[1]![0]; expect(next).toMatchObject({ parentFolderId: p.folderId, expectedParentRevision: 39, name: '移動後の子' });
  expect(next.operationId).not.toBe(first.operationId); expect(next.folderId).not.toBe(first.folderId);
  expect(rootFolderOperations(client).get()?.context).toMatchObject({ folderId: p.folderId, sourceParentId: q.folderId });
});

test('R1 旧page上限外の同じ作成先を続き表示で再選択した場合だけ新しい取得済み上限で見直せる', async () => {
  const { api, client } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const saved = rootFolderOperations(client).get()!; fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id !== rootId ? page([]) : cursor ? page([{ ...p, revision: 40 }]) : page([q], 'display-next')));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId, 'pages'], exact: true }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーをさらに表示' })); await choose();
  const reopened = await open(); const reads: [string, string | undefined][] = [];
  api.listFolderChildren.mockImplementation((id, cursor) => { reads.push([id, cursor]); return Promise.resolve(id !== rootId ? page([]) : cursor === 'fresh-next' ? page([{ ...p, revision: 44 }]) : page([q], 'fresh-next')); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(reopened).getByLabelText('フォルダー名')).toBeEnabled());
  expect(reads).toEqual([[rootId, undefined], [rootId, 'fresh-next'], [p.folderId, undefined]]); expect(saved.context).toMatchObject({ pageLimit: 1 });
  fill(reopened); submit(reopened); await within(reopened).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls[1]![0]).toMatchObject({ parentFolderId: p.folderId, expectedParentRevision: 44 }); expect(rootFolderOperations(client).get()?.context).toMatchObject({ pageLimit: 2 });
});

test.each(['別対象', '同一IDの直URL'] as const)('R1 %sだけでは確定拒否の元対象provenanceを置き換えない', async selection => {
  const { api, client, router, history } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const saved = rootFolderOperations(client).get()!; fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const moved = { ...p, parentFolderId: q.folderId, revision: 31 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId, 'pages'], exact: true }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  if (selection === '別対象') await choose(q);
  else { await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); await act(async () => history.back()); }
  const reopened = await open(rootTitle); const reads: string[] = [];
  api.listFolderChildren.mockImplementation(id => { reads.push(id); return Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(reopened).findByText(/ツリーで選び直してください/);
  expect(reads).toEqual([rootId]); expect(rootFolderOperations(client).get()).toBe(saved); expect(api.createFolder).toHaveBeenCalledTimes(1);
});

test.each(['pending', 'unknown'] as const)('R1 %sで同じIDを移動後に再選択してもrequest/contextを変えず、unknown再送にreadを追加しない', async status => {
  const { api, client, router, history } = setup(); await choose(); const write = deferred<ReturnType<typeof success>>();
  api.createFolder.mockReturnValueOnce(write.promise); const dialog = await open(); fill(dialog); submit(dialog); await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1));
  if (status === 'unknown') await act(async () => write.reject(Error('lost')));
  const saved = rootFolderOperations(client).get()!;
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  const moved = { ...p, parentFolderId: q.folderId, revision: 45 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])));
  await act(async () => history.back()); await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId, 'pages'], exact: true }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole('button', { name: `${q.name}の子フォルダーを開く` })); await choose(moved); const reopened = await open(rootTitle);
  expect(rootFolderOperations(client).get()).toBe(saved); expect(saved.context).toMatchObject({ sourceParentId: rootId });
  if (status === 'pending') { expect(within(reopened).getByText('作成結果を確認しています…')).toBeVisible(); await act(async () => write.reject(Error('lost'))); }
  const context = rootFolderOperations(client).get()!.context; const count = api.listFolderChildren.mock.calls.length;
  api.listFolderChildren.mockRejectedValue(Error('no reads')); api.createFolder.mockRejectedValueOnce(problem('FORBIDDEN', 403));
  fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' })); await within(reopened).findByRole('button', { name: '同じ内容で再試行' });
  expect(api.createFolder.mock.calls[1]![0]).toBe(saved.request); expect(api.listFolderChildren).toHaveBeenCalledTimes(count);
  expect(rootFolderOperations(client).get()).toMatchObject({ status: 'unknown' }); expect(rootFolderOperations(client).get()!.context).toBe(context);
});

test.each(['selection', 'navigation'] as const)('R1 新しい同一対象provenanceの見直しread中の%s変更では古い完了が拒否要求をclearしない', async action => {
  const { api, client, router } = setup(); await choose(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const saved = rootFolderOperations(client).get()!; fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); const moved = { ...p, parentFolderId: q.folderId, revision: 39 };
  api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [q] : id === q.folderId ? [moved] : [])));
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId, 'pages'], exact: true }); });
  await waitFor(() => expect(screen.queryByRole('button', { name: p.name })).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole('button', { name: `${q.name}の子フォルダーを開く` })); await choose(moved); const reopened = await open(rootTitle);
  const read = deferred<FolderChildren>(); const calls: string[] = [];
  api.listFolderChildren.mockImplementation(id => { calls.push(id); return id === q.folderId ? read.promise : Promise.resolve(page([])); });
  fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' })); expect(calls[0]).toBe(q.folderId);
  if (action === 'selection') fireEvent.click(screen.getByRole('button', { name: q.name, hidden: true }));
  else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(page([moved])));
  expect(rootFolderOperations(client).get()).toBe(saved); expect(api.createFolder).toHaveBeenCalledTimes(1); expect(saved.context).toMatchObject({ sourceParentId: rootId });
});
