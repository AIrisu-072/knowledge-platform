import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(),
} }));
const rootId = '019a0010-0000-7000-8000-000000000041';
const available = { status: 'available' as const };
const denied = { status: 'disabled' as const, reason: 'permission' as const };
const folder = (index: number, parentFolderId = rootId): Folder => ({ folderId: `019a0010-0000-7000-8000-${String(index).padStart(12, '0')}`, name: `資料${index}`, revision: 0, parentFolderId });
const page = (items: Folder[], nextCursor: string | null = null, createDocument = available as FolderChildren['capabilities']['createDocument']): FolderChildren => ({ items, nextCursor, capabilities: { createDocument, createFolder: available, renameFolder: available, moveFolder: available, manageAccess: available } });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const more = (name = 'System Root') => screen.getByRole('button', { name: `${name}の子フォルダーをさらに表示` });
const restart = (name = 'System Root') => screen.getByRole('button', { name: `${name}の子フォルダーを最初から読み直す` });
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => client.clear()));
function setup() {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'System Root', revision: 17, parentFolderId: null, capabilities: { createFolder: available, createDocument: available } });
  api.listFolderChildren.mockResolvedValue(page([]));
  api.listDocuments.mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  api.createFolder.mockImplementation(request => Promise.resolve({ operationId: request.operationId, resourceId: request.folderId, resultingRevision: 0, changed: true, occurredAt: '2026-10-05T12:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: ['/documents?view=published'] });
  const router = createRouter({ routeTree: root.addChildren([list, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, history };
}

// Removing explicit continuation leaves the 201st child unreachable in the real route DOM.
test('先頭200件からサーバーのopaque cursorだけで続きと3ページ目を追加し、末尾選択をURLへ保持する', async () => {
  const { api, router } = setup();
  const first = Array.from({ length: 200 }, (_, i) => folder(i + 100));
  const second = folder(300); const third = folder(301);
  const cursor = 'opaque+/=?日本語';
  api.listFolderChildren.mockImplementation((id, token) => Promise.resolve(id !== rootId ? page([]) : token === cursor ? page([second], 'opaque-third') : token === 'opaque-third' ? page([third]) : page(first, cursor)));
  await screen.findByRole('button', { name: first[199]!.name });
  expect(screen.queryByRole('button', { name: second.name })).not.toBeInTheDocument();
  fireEvent.click(more());
  await screen.findByRole('button', { name: second.name });
  expect(api.listFolderChildren).toHaveBeenCalledWith(rootId, cursor);
  expect(screen.getByRole('button', { name: first[0]!.name })).toBeVisible();
  fireEvent.click(more());
  fireEvent.click(await screen.findByRole('button', { name: third.name }));
  await waitFor(() => expect(router.state.location.search.folderId).toBe(third.folderId));
  expect(screen.getByRole('button', { name: third.name })).toHaveAttribute('aria-current', 'location');
  expect(api.listFolderChildren).toHaveBeenCalledWith(rootId, 'opaque-third');
  expect(screen.queryByRole('button', { name: /System Rootの子フォルダーをさらに表示/ })).not.toBeInTheDocument();
});

test('途中変更で同じIDが再登場しても既存位置で最新の名前を一度だけ表示する', async () => {
  const { api } = setup(); const a = folder(100); const b = folder(101);
  api.listFolderChildren.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([{ ...a, name: '改名後' }, b]) : page([a], 'second')));
  await screen.findByRole('button', { name: a.name }); fireEvent.click(more());
  await screen.findByRole('button', { name: b.name });
  expect(screen.getAllByRole('button', { name: '改名後' })).toHaveLength(1);
  expect(screen.queryByRole('button', { name: a.name })).not.toBeInTheDocument();
  expect(screen.getAllByRole('button', { name: /の子フォルダーを開く$/ })).toHaveLength(2);
});

test('連打・取得中の閉再表示でも同じ親への次ページ要求は一つで既存選択を保つ', async () => {
  const { api, router } = setup(); const first = folder(100); const tail = folder(101); const next = deferred<FolderChildren>();
  api.listFolderChildren.mockImplementation((id, cursor) => id !== rootId ? Promise.resolve(page([])) : cursor ? next.promise : Promise.resolve(page([first], 'next')));
  fireEvent.click(await screen.findByRole('button', { name: first.name }));
  await waitFor(() => expect(router.state.location.search.folderId).toBe(first.folderId));
  const button = more(); act(() => { fireEvent.click(button); fireEvent.click(button); });
  expect(api.listFolderChildren.mock.calls.filter(([, token]) => token === 'next')).toHaveLength(1);
  await waitFor(() => expect(more()).toBeDisabled());
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーを閉じる' }));
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーを開く' }));
  expect(screen.getByRole('button', { name: first.name })).toBeVisible();
  await act(async () => next.resolve(page([tail])));
  await screen.findByRole('button', { name: tail.name });
  expect(router.state.location.search.folderId).toBe(first.folderId);
  expect(screen.getByRole('button', { name: first.name })).toHaveAttribute('aria-current', 'location');
});

test('別親のページとcursorを混ぜず、閉再表示と画面往復でも取得済み行と選択を保つ', async () => {
  const { api, router, history } = setup(); const a = folder(100); const b = folder(101); const a1 = folder(102, a.folderId); const a2 = folder(103, a.folderId); const b1 = folder(104, b.folderId); const b2 = folder(105, b.folderId);
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id === rootId ? page([a, b]) : id === a.folderId ? (cursor === 'a-next' ? page([a2]) : page([a1], 'a-next')) : id === b.folderId ? (cursor === 'b-next' ? page([b2]) : page([b1], 'b-next')) : page([])));
  await screen.findByRole('button', { name: a.name });
  fireEvent.click(screen.getByRole('button', { name: `${a.name}の子フォルダーを開く` }));
  fireEvent.click(screen.getByRole('button', { name: `${b.name}の子フォルダーを開く` }));
  await screen.findByRole('button', { name: a1.name }); await screen.findByRole('button', { name: b1.name });
  fireEvent.click(more(a.name)); fireEvent.click(more(b.name));
  await screen.findByRole('button', { name: a2.name });
  fireEvent.click(await screen.findByRole('button', { name: b2.name }));
  await waitFor(() => expect(router.state.location.search.folderId).toBe(b2.folderId));
  expect(api.listFolderChildren).toHaveBeenCalledWith(a.folderId, 'a-next');
  expect(api.listFolderChildren).toHaveBeenCalledWith(b.folderId, 'b-next');
  expect(api.listFolderChildren).not.toHaveBeenCalledWith(a.folderId, 'b-next');
  fireEvent.click(screen.getByRole('button', { name: `${a.name}の子フォルダーを閉じる` }));
  fireEvent.click(screen.getByRole('button', { name: `${a.name}の子フォルダーを開く` }));
  expect(screen.getByRole('button', { name: a2.name })).toBeVisible();
  fireEvent.click(restart(a.name));
  await waitFor(() => expect(screen.queryByRole('button', { name: a2.name })).not.toBeInTheDocument());
  expect(screen.getByRole('button', { name: b2.name })).toHaveAttribute('aria-current', 'location');
  await screen.findByRole('button', { name: a1.name });
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => history.back());
  await screen.findByRole('button', { name: b.name });
  fireEvent.click(screen.getByRole('button', { name: `${b.name}の子フォルダーを開く` }));
  expect(await screen.findByRole('button', { name: b2.name })).toHaveAttribute('aria-current', 'location');
  expect(screen.queryByRole('button', { name: `${b.name}の子フォルダーをさらに表示` })).not.toBeInTheDocument();
});

test('次ページ失敗は既存行を残して同じcursorを明示再試行できる', async () => {
  const { api } = setup(); const first = folder(100); const tail = folder(101); let attempts = 0;
  api.listFolderChildren.mockImplementation((_id, cursor) => cursor ? (++attempts === 1 ? Promise.reject(new Error('offline')) : Promise.resolve(page([tail]))) : Promise.resolve(page([first], 'retry-exact')));
  await screen.findByRole('button', { name: first.name }); fireEvent.click(more());
  await screen.findByRole('alert');
  expect(screen.getByRole('button', { name: first.name })).toBeVisible();
  expect(screen.queryByRole('button', { name: tail.name })).not.toBeInTheDocument();
  expect(restart()).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'System Rootの子フォルダーの続きを再試行' }));
  await screen.findByRole('button', { name: tail.name });
  expect(api.listFolderChildren.mock.calls.filter(([, cursor]) => cursor === 'retry-exact')).toHaveLength(2);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test.each([['CURSOR_STALE', 409], ['VALIDATION_FAILED', 422]])('%sのcursorは完了にせず最初から読み直し、新cursorでもURL選択は変えない', async (code, status) => {
  const { api, router } = setup(); const old = folder(100); const fresh = folder(101); const tail = folder(102); let reset = false;
  api.listFolderChildren.mockImplementation((id, cursor) => id !== rootId ? Promise.resolve(page([])) : cursor === 'expired' ? Promise.reject({ type: 'about:blank', title: 'Invalid cursor', status, code, traceId: 'synthetic', retryable: false }) : Promise.resolve(cursor === 'fresh' ? page([tail]) : reset ? page([fresh], 'fresh') : page([old], 'expired')));
  fireEvent.click(await screen.findByRole('button', { name: old.name }));
  await waitFor(() => expect(router.state.location.search.folderId).toBe(old.folderId));
  fireEvent.click(more()); await screen.findByRole('alert');
  expect(screen.getByRole('button', { name: old.name })).toBeVisible();
  reset = true; fireEvent.click(restart());
  await screen.findByRole('button', { name: fresh.name });
  expect(screen.queryByRole('button', { name: old.name })).not.toBeInTheDocument();
  expect(router.state.location.search.folderId).toBe(old.folderId);
  fireEvent.click(more()); await screen.findByRole('button', { name: tail.name });
  expect(api.listFolderChildren).toHaveBeenCalledWith(rootId, 'fresh');
});

test('初回読取失敗は空とせず、再読み込みで先頭から回復する', async () => {
  const { api } = setup(); api.listFolderChildren.mockRejectedValueOnce(new Error('offline')).mockResolvedValue(page([folder(100)]));
  const alert = await screen.findByRole('alert');
  fireEvent.click(within(alert).getByRole('button', { name: '再読み込み' }));
  await screen.findByRole('button', { name: folder(100).name });
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('選択親の通常capability queryとページ列を分離し、親権限で登録可否を補わない', async () => {
  const { api, client } = setup(); const parent = folder(100); const child = folder(101, parent.folderId); const tail = folder(102, parent.folderId);
  api.listFolderChildren.mockImplementation((id, cursor) => Promise.resolve(id === rootId ? page([parent]) : id === parent.folderId ? page(cursor ? [tail] : [child], cursor ? null : 'next', denied) : page([])));
  fireEvent.click(await screen.findByRole('button', { name: parent.name }));
  await waitFor(() => expect(screen.getByRole('button', { name: '文書を登録' })).toBeDisabled());
  fireEvent.click(screen.getByRole('button', { name: `${parent.name}の子フォルダーを開く` }));
  await screen.findByRole('button', { name: child.name }); fireEvent.click(more(parent.name));
  await screen.findByRole('button', { name: tail.name });
  expect(client.getQueryData(['folder-tree', parent.folderId])).toEqual(page([child], 'next', denied));
  expect(screen.getByRole('button', { name: '文書を登録' })).toBeDisabled();
});

test('Root作成の既存prefix無効化でページ列を再取得し、新cursorだけを使用する', async () => {
  const { api, client } = setup(); const first = folder(100); const old = folder(101); const fresh = folder(102); let updated = false;
  api.listFolderChildren.mockImplementation((_id, cursor) => Promise.resolve(cursor ? page([updated ? fresh : old]) : page([first], updated ? 'new-cursor' : 'old-cursor')));
  await screen.findByRole('button', { name: first.name }); fireEvent.click(more());
  await screen.findByRole('button', { name: old.name }); updated = true;
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId] }); });
  await screen.findByRole('button', { name: fresh.name });
  expect(screen.queryByRole('button', { name: old.name })).not.toBeInTheDocument();
  expect(api.listFolderChildren).toHaveBeenLastCalledWith(rootId, 'new-cursor');
});

test('一覧の再読取と無効化はunknownのRoot作成要求を消さず同一要求の再送を維持する', async () => {
  const { api, client } = setup(); api.listFolderChildren.mockResolvedValue(page([folder(100)], 'next')); api.createFolder.mockRejectedValueOnce(new Error('lost'));
  const trigger = await screen.findByRole('button', { name: 'System Rootにフォルダーを作成' });
  await waitFor(() => expect(trigger).toBeEnabled()); fireEvent.click(trigger);
  const dialog = await screen.findByRole('dialog');
  fireEvent.change(within(dialog).getByLabelText('フォルダー名'), { target: { value: '合成作成' } });
  fireEvent.change(within(dialog).getByLabelText('作成理由'), { target: { value: '合成理由' } });
  fireEvent.submit(within(dialog).getByRole('button', { name: '作成する' }).closest('form')!);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const request = api.createFolder.mock.calls[0]![0];
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  fireEvent.click(restart());
  await act(async () => { await client.invalidateQueries({ queryKey: ['folder-tree', rootId] }); });
  fireEvent.click(screen.getByRole('button', { name: 'System Rootにフォルダーを作成' })); const reopened = await screen.findByRole('dialog');
  expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue(request.name);
  fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' }));
  await within(reopened).findByText('フォルダーを作成しました。');
  expect(api.createFolder).toHaveBeenCalledTimes(2); expect(api.createFolder.mock.calls[1]![0]).toBe(request);
});

test('空のページでもnextCursorがあれば続きを表示し、空の末尾でだけ終了する', async () => {
  const { api } = setup();
  api.listFolderChildren.mockImplementation((_id, cursor) => Promise.resolve(page([], cursor ? null : 'empty-page-next')));
  await screen.findByRole('button', { name: 'System Rootの子フォルダーをさらに表示' });
  fireEvent.click(more());
  await waitFor(() => expect(screen.queryByRole('button', { name: 'System Rootの子フォルダーをさらに表示' })).not.toBeInTheDocument());
  expect(api.listFolderChildren).toHaveBeenCalledWith(rootId, 'empty-page-next');
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test.each([['FORBIDDEN', 403], ['FOLDER_NOT_FOUND', 404]])('続きの現在readが%sなら完了にせず行とサーバーエラーを表示する', async (code, status) => {
  const { api } = setup(); const first = folder(100);
  api.listFolderChildren.mockImplementation((_id, cursor) => cursor ? Promise.reject({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false }) : Promise.resolve(page([first], 'next')));
  await screen.findByRole('button', { name: first.name }); fireEvent.click(more());
  expect(await screen.findByRole('alert')).toHaveTextContent(status === 403 ? 'アクセスできません' : '見つかりません');
  expect(screen.getByRole('button', { name: first.name })).toBeVisible();
  expect(screen.getByRole('button', { name: 'System Rootの子フォルダーの続きを再試行' })).toBeEnabled();
  expect(restart()).toBeEnabled();
});

test('pendingのRoot作成は画面往復と一覧再読取を跨いで保持し、成功後の無効化で新しい行を読む', async () => {
  const { api, router, history } = setup(); const write = deferred<unknown>(); const first = folder(100); let created: Folder | undefined;
  api.listFolderChildren.mockImplementation(() => Promise.resolve(page(created ? [first, created] : [first], 'next')));
  api.createFolder.mockReturnValue(write.promise);
  const trigger = await screen.findByRole('button', { name: 'System Rootにフォルダーを作成' });
  await waitFor(() => expect(trigger).toBeEnabled()); fireEvent.click(trigger);
  const dialog = await screen.findByRole('dialog');
  fireEvent.change(within(dialog).getByLabelText('フォルダー名'), { target: { value: '追加する合成資料' } });
  fireEvent.change(within(dialog).getByLabelText('作成理由'), { target: { value: '合成理由' } });
  fireEvent.submit(within(dialog).getByRole('button', { name: '作成する' }).closest('form')!);
  await within(dialog).findByText('作成結果を確認しています…');
  const request = api.createFolder.mock.calls[0]![0];
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => history.back());
  await screen.findByRole('button', { name: first.name }); fireEvent.click(restart());
  await screen.findByRole('button', { name: first.name });
  fireEvent.click(screen.getByRole('button', { name: 'System Rootにフォルダーを作成' }));
  const reopened = await screen.findByRole('dialog');
  expect(within(reopened).getByText('作成結果を確認しています…')).toBeVisible();
  expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue(request.name);
  expect(api.createFolder).toHaveBeenCalledTimes(1);
  created = { folderId: request.folderId, name: request.name, parentFolderId: rootId, revision: 0 };
  await act(async () => write.resolve({ operationId: request.operationId, resourceId: request.folderId, changed: true, resultingRevision: 0, occurredAt: '2026-10-05T12:00:00Z' }));
  await within(reopened).findByText('フォルダーを作成しました。');
  fireEvent.click(within(reopened).getByRole('button', { name: '確認して閉じる' }));
  expect(await screen.findByRole('button', { name: created.name })).toBeVisible();
});
