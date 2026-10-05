import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(),
} }));
const rootId = '019a0010-0000-7000-8000-000000000041';
const selectedId = '019a0010-0000-7000-8000-000000000099';
const title = 'System Rootにフォルダーを作成';
const available = { status: 'available' as const };
const disabled = { status: 'disabled' as const, reason: 'permission' as const };
const rootFolder = (revision = 17, capability: unknown = available) => ({ folderId: rootId, parentFolderId: null, name: 'System Root', revision, capabilities: { createFolder: capability } });
const problem = (code: string, status: number) => ({ type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: status >= 500 });
const success = (request: { operationId: string; folderId: string }) => ({ operationId: request.operationId, resourceId: request.folderId, resultingRevision: 0, changed: true, occurredAt: '2026-10-05T12:00:00Z' });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => client.clear()));
function setup(input: { root?: unknown; entry?: string; read?: Promise<unknown> } = {}) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getRootFolder.mockImplementation(() => input.read ?? Promise.resolve(input.root ?? rootFolder()));
  api.listFolderChildren.mockResolvedValue({ items: [], nextCursor: null, capabilities: { createFolder: available } });
  api.listDocuments.mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  api.createFolder.mockImplementation(request => Promise.resolve(success(request)));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <h1>別画面</h1> });
  const history = createMemoryHistory({ initialEntries: [input.entry ?? '/documents?view=published'] });
  const router = createRouter({ routeTree: root.addChildren([list, other]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 15_000, gcTime: Infinity } } }); clients.push(client);
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  const rendered = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, history, client, invalidate, ...rendered };
}
async function open() {
  const trigger = await screen.findByRole('button', { name: title });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
  return screen.findByRole('dialog', { name: title });
}
function fill(dialog: HTMLElement, name = '  e\u0301資料  ', reason = '\u0085 合成理由 \u0085') {
  fireEvent.change(within(dialog).getByLabelText('フォルダー名'), { target: { value: name } });
  fireEvent.change(within(dialog).getByLabelText('作成理由'), { target: { value: reason } });
}
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '作成する' }).closest('form')!); }
async function goOther(router: ReturnType<typeof setup>['router']) { await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); }); }

// Removing the root-only entry or deriving its parent from selection must fail this test.
test('非root選択中も最新GETのSystem Root ID/revisionで作成し、子revisionを親へ書き戻さない', async () => {
  const { api, client, invalidate, router } = setup({ entry: `/documents?view=published&folderId=${selectedId}` });
  const dialog = await open();
  expect(within(dialog).getByText(/登録先：System Root直下/)).toBeVisible();
  fill(dialog); api.getRootFolder.mockResolvedValue(rootFolder(23)); submit(dialog);
  await within(dialog).findByText('フォルダーを作成しました。');
  expect(api.getRootFolder.mock.calls.length).toBeGreaterThanOrEqual(2);
  const request = api.createFolder.mock.calls[0]![0];
  expect(request).toEqual({ operationId: expect.stringMatching(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/), folderId: expect.stringMatching(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/), parentFolderId: rootId, expectedParentRevision: 23, name: 'é資料', reason: '合成理由' });
  expect(request.folderId).not.toBe(request.operationId);
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['folder-tree', 'root'] });
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['folder-tree', rootId] });
  expect(client.getQueryData(['folder-tree', 'root'])).toMatchObject({ revision: 23 });
  expect(router.state.location.search.folderId).toBe(selectedId);
});

test.each([disabled, { status: 'disabled', reason: 'lifecycle' }, { status: 'hidden' }, null])('サーバーcapability %p では新規要求を送らない', async capability => {
  const { api } = setup({ root: rootFolder(17, capability) });
  await screen.findByRole('heading', { name: '文書一覧' });
  await waitFor(() => expect(api.getRootFolder).toHaveBeenCalled());
  const trigger = screen.queryByRole('button', { name: title });
  if (trigger) expect(trigger).toBeDisabled();
  expect(api.createFolder).not.toHaveBeenCalled();
  if ((capability as { reason?: string })?.reason === 'permission') expect(await screen.findByText('権限がありません')).toBeVisible();
});

test('初回GET未完了では作成入口から送れない', async () => {
  const { api } = setup({ read: new Promise(() => {}) });
  await screen.findByText('フォルダーを読み込み中…');
  const trigger = screen.queryByRole('button', { name: title });
  if (trigger) expect(trigger).toBeDisabled();
  expect(api.createFolder).not.toHaveBeenCalled();
});

test.each([new Error('read failed'), problem('FORBIDDEN', 403)])('送信前の明示再取得に失敗したらcached rootを使わない %p', async error => {
  const { api } = setup(); const dialog = await open(); fill(dialog);
  api.getRootFolder.mockRejectedValue(error); submit(dialog);
  await within(dialog).findByText(/最新のSystem Rootを取得できません/);
  expect(api.createFolder).not.toHaveBeenCalled();
  expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled();
});

test.each([rootFolder(18, disabled), { ...rootFolder(), revision: undefined }, { ...rootFolder(), folderId: '' }])('最新rootが送信条件を満たさなければ作成しない %p', async updated => {
  const { api } = setup(); const dialog = await open(); fill(dialog); api.getRootFolder.mockResolvedValue(updated); submit(dialog);
  await within(dialog).findByText(/現在のSystem Rootでは作成できません/);
  expect(api.createFolder).not.toHaveBeenCalled();
});

test('二重送信を最新GET中とPOST中に防ぎ、POST中はcancel/escapeと入力を無効にする', async () => {
  const { api } = setup(); const dialog = await open(); fill(dialog);
  const read = deferred<ReturnType<typeof rootFolder>>(); const write = deferred<ReturnType<typeof success>>();
  api.getRootFolder.mockReturnValue(read.promise); api.createFolder.mockReturnValue(write.promise);
  act(() => { submit(dialog); submit(dialog); });
  expect(api.getRootFolder).toHaveBeenCalledTimes(2); expect(api.createFolder).not.toHaveBeenCalled();
  await act(async () => read.resolve(rootFolder()));
  await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1));
  act(() => { submit(dialog); submit(dialog); });
  expect(api.createFolder).toHaveBeenCalledTimes(1);
  expect(within(dialog).getByLabelText('フォルダー名')).toBeDisabled();
  expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toBeDisabled();
  await userEvent.setup().keyboard('{Escape}'); expect(dialog).toBeVisible();
  const event = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(event); expect(event.defaultPrevented).toBe(true);
  await act(async () => write.resolve(success(api.createFolder.mock.calls[0]![0])));
  await within(dialog).findByText('フォルダーを作成しました。');
});

test('取消は未送信入力だけを捨て、閉じて開き直せる', async () => {
  const { api } = setup(); const dialog = await open(); fill(dialog);
  await userEvent.setup().click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  const reopened = await open(); expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue('');
  expect(within(reopened).getByLabelText('作成理由')).toHaveValue(''); expect(api.createFolder).not.toHaveBeenCalled();
});

test.each(['\u0085名前', '名前\t', '\n名前', '名前\n', '\r名前', '.', '..', 'a/b', 'a\\b', '😀'.repeat(256)])('不正なraw名前は送信前に拒否 %p', async name => {
  const { api } = setup(); const dialog = await open(); fill(dialog, name);
  expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled(); submit(dialog); expect(api.createFolder).not.toHaveBeenCalled();
});

test.each(['😀'.repeat(255), 'e\u0301'.repeat(255), '\ufeff'])('UTF16長でなくNFC後Unicode scalar境界を使う %p', async name => {
  const { api } = setup(); const dialog = await open(); fill(dialog, name); submit(dialog);
  await within(dialog).findByText('フォルダーを作成しました。'); expect(api.createFolder.mock.calls[0]![0].name).toBe(name.normalize('NFC'));
});

test.each(['', ' ', '中\u0000', 'あ'.repeat(342)])('不正な理由は送信しない %p', async reason => {
  const { api } = setup(); const dialog = await open(); fill(dialog, '資料', reason);
  expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled(); submit(dialog); expect(api.createFolder).not.toHaveBeenCalled();
});

test('1024 UTF8 bytesの理由はRust trimして受理する', async () => {
  const { api } = setup(); const dialog = await open(); const reason = 'あ'.repeat(341) + 'a'; fill(dialog, '資料', `\u0085${reason}\u0085`); submit(dialog);
  await within(dialog).findByText('フォルダーを作成しました。'); expect(api.createFolder.mock.calls[0]![0].reason).toBe(reason);
});

test('unknownは入力変更・root更新・閉再表示・Back/Forward・別routeを跨いで同一要求だけを再送', async () => {
  const { api, client, router, history } = setup(); api.createFolder.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockRejectedValueOnce(problem('FOLDER_NOT_FOUND', 404));
  const dialog = await open(); fill(dialog); submit(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  const request = api.createFolder.mock.calls[0]![0];
  expect(within(dialog).getByText(/管理者へ/)).toBeVisible(); expect(within(dialog).getByText(/タブ終了/)).toBeVisible();
  fireEvent.change(within(dialog).getByLabelText('フォルダー名'), { target: { value: '別の名前' } });
  act(() => client.setQueryData(['folder-tree', 'root'], rootFolder(89, disabled)));
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); await goOther(router);
  await act(async () => history.back()); const again = await open();
  expect(within(again).getByLabelText('フォルダー名')).toHaveValue(request.name);
  fireEvent.click(within(again).getByRole('button', { name: '同じ内容で再試行' }));
  await within(again).findByRole('button', { name: '同じ内容で再試行' });
  expect(within(again).queryByRole('button', { name: /見直す/ })).not.toBeInTheDocument();
  await act(async () => history.forward()); await act(async () => history.back());
  const third = await open(); fireEvent.click(within(third).getByRole('button', { name: '同じ内容で再試行' }));
  await within(third).findByRole('button', { name: '同じ内容で再試行' });
  fireEvent.click(within(third).getByRole('button', { name: '同じ内容で再試行' }));
  await within(third).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls).toHaveLength(4);
  for (const [body] of api.createFolder.mock.calls) expect(body).toBe(request);
});

test('unknown回復入口はroot data喪失・read失敗でも消さない', async () => {
  const { api, client, router, history } = setup(); api.createFolder.mockRejectedValueOnce(new Error('lost'));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  await goOther(router); client.removeQueries({ queryKey: ['folder-tree', 'root'] }); api.getRootFolder.mockRejectedValue(problem('FOLDER_NOT_FOUND', 404));
  await act(async () => history.back()); const reopened = await open();
  expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue('é資料');
  fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' })); await within(reopened).findByText('フォルダーを作成しました。');
  expect(api.createFolder.mock.calls[1]![0]).toBe(api.createFolder.mock.calls[0]![0]);
});

test.each(['success', 'unknown'] as const)('別route滞在中の遅延%sはnavigationせず、戻ればreceiptを確認できる', async outcome => {
  const { api, router, history } = setup(); const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValue(write.promise);
  const dialog = await open(); fill(dialog); submit(dialog); await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1));
  await goOther(router); await act(async () => { if (outcome === 'success') write.resolve(success(api.createFolder.mock.calls[0]![0])); else write.reject(new Error('lost')); });
  expect(router.state.location.pathname).toBe('/tasks'); expect(screen.getByRole('heading', { name: '別画面' })).toBeVisible();
  await act(async () => history.back()); const reopened = await open();
  expect(within(reopened).getByText(outcome === 'success' ? 'フォルダーを作成しました。' : '作成結果を確認できません')).toBeVisible();
  expect(within(reopened).getByLabelText('フォルダー名')).toBeDisabled(); expect(api.createFolder).toHaveBeenCalledTimes(1);
});

test('再取得失敗は成功を覆さず、閉再表示でreceipt保持、明示確認後だけ次の作成', async () => {
  const { api, invalidate } = setup(); const dialog = await open(); fill(dialog); invalidate.mockRejectedValue(new Error('refresh')); submit(dialog);
  await within(dialog).findByText('フォルダーを作成しました。'); const first = api.createFolder.mock.calls[0]![0];
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); const receipt = await open();
  expect(within(receipt).getByText('フォルダーを作成しました。')).toBeVisible();
  fireEvent.click(within(receipt).getByRole('button', { name: '確認して閉じる' }));
  const next = await open(); fill(next, '次の資料'); api.getRootFolder.mockResolvedValue(rootFolder(31)); submit(next);
  await within(next).findByText('フォルダーを作成しました。'); const second = api.createFolder.mock.calls[1]![0];
  expect(second).toMatchObject({ expectedParentRevision: 31, name: '次の資料' }); expect(second.operationId).not.toBe(first.operationId); expect(second.folderId).not.toBe(first.folderId);
});

test('初回競合は重複名も案内し、明示見直しの実refetch成功後にだけ新操作へ進める', async () => {
  const { api } = setup(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  expect(within(dialog).getByText(/同名のフォルダー/)).toBeVisible();
  const first = api.createFolder.mock.calls[0]![0]; api.getRootFolder.mockRejectedValueOnce(new Error('refresh'));
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' })); await within(dialog).findByText(/最新のSystem Rootを取得できません/);
  expect(within(dialog).getByLabelText('フォルダー名')).toBeDisabled();
  api.getRootFolder.mockResolvedValue(rootFolder(22)); fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(dialog).getByLabelText('フォルダー名')).toBeEnabled()); fill(dialog, '別の資料'); submit(dialog);
  await within(dialog).findByText('フォルダーを作成しました。'); const second = api.createFolder.mock.calls[1]![0];
  expect(second.expectedParentRevision).toBe(22); expect(second.operationId).not.toBe(first.operationId);
});

test('送信前readの遅延完了は閉再表示後の入力を送信しない', async () => {
  const { api } = setup(); const dialog = await open(); fill(dialog); const read = deferred<ReturnType<typeof rootFolder>>(); api.getRootFolder.mockReturnValueOnce(read.promise); submit(dialog);
  fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  await act(async () => read.resolve(rootFolder(44)));
  const reopened = await open(); expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue(''); expect(api.createFolder).not.toHaveBeenCalled();
});

test.each([['AUTHENTICATION_REQUIRED', 401], ['REVISION_CONFLICT', 409]])('unknown後の%sは離脱や新入力を促さず、初回結果の照会を案内する', async (code, status) => {
  const { api } = setup(); api.createFolder.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem(code as string, status as number));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  fireEvent.click(within(dialog).getByRole('button', { name: '同じ内容で再試行' })); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  expect(within(dialog).getByText(/初回の作成結果は未確定/)).toBeVisible();
  expect(within(dialog).queryByText(/再読み込みして/)).not.toBeInTheDocument();
  expect(within(dialog).queryByText(/名前と最新の状態を見直して/)).not.toBeInTheDocument();
  expect(within(dialog).getByLabelText('フォルダー名')).toBeDisabled();
});

test('cached rootが残る背景read失敗でも新規POSTを送らない', async () => {
  const { api, client } = setup(); const dialog = await open(); fill(dialog);
  api.getRootFolder.mockRejectedValue(new Error('background refresh'));
  await act(async () => { await client.refetchQueries({ queryKey: ['folder-tree', 'root'] }); });
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '作成する' })).toBeDisabled());
  expect(client.getQueryData(['folder-tree', 'root'])).toMatchObject({ revision: 17 });
  submit(dialog); expect(api.createFolder).not.toHaveBeenCalled();
});

test('pendingを別route往復で保持し、戻った画面から別要求を開始できない', async () => {
  const { api, router, history } = setup(); const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValue(write.promise);
  const dialog = await open(); fill(dialog); submit(dialog); await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(1));
  await goOther(router); await act(async () => history.back()); const reopened = await open();
  expect(within(reopened).getByText('作成結果を確認しています…')).toBeVisible();
  expect(within(reopened).getByLabelText('フォルダー名')).toBeDisabled(); submit(reopened); expect(api.createFolder).toHaveBeenCalledTimes(1);
  await act(async () => write.resolve(success(api.createFolder.mock.calls[0]![0]))); await within(reopened).findByText('フォルダーを作成しました。');
});

test.each(['pending', 'unknown'] as const)('閉再表示前の見直しread完了が新しい%sを消さない', async state => {
  const { api } = setup(); api.createFolder.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' });
  const oldRead = deferred<ReturnType<typeof rootFolder>>(); api.getRootFolder.mockReturnValueOnce(oldRead.promise);
  fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); const reopened = await open();
  api.getRootFolder.mockResolvedValue(rootFolder(44)); fireEvent.click(within(reopened).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(reopened).getByLabelText('フォルダー名')).toBeEnabled());
  const write = deferred<ReturnType<typeof success>>(); api.createFolder.mockReturnValueOnce(write.promise); fill(reopened, '新しい要求'); submit(reopened);
  await waitFor(() => expect(api.createFolder).toHaveBeenCalledTimes(2)); const fixed = api.createFolder.mock.calls[1]![0];
  if (state === 'unknown') await act(async () => write.reject(new Error('lost')));
  await act(async () => oldRead.resolve(rootFolder(33)));
  expect(within(reopened).getByLabelText('フォルダー名')).toHaveValue('新しい要求');
  expect(within(reopened).getByLabelText('フォルダー名')).toBeDisabled();
  if (state === 'pending') { expect(within(reopened).getByText('作成結果を確認しています…')).toBeVisible(); await act(async () => write.resolve(success(fixed))); }
  else { fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' })); await within(reopened).findByText('フォルダーを作成しました。'); expect(api.createFolder.mock.calls[2]![0]).toBe(fixed); }
});

test.each([{ operationId: 'wrong' }, { resourceId: rootId }, { resultingRevision: 18 }, { changed: false }, { occurredAt: 'invalid' }])('照合不一致%pでは成功通知を出さず固定要求の回復だけを出す', async patch => {
  const { api } = setup(); api.createFolder.mockImplementationOnce(request => Promise.resolve({ ...success(request), ...patch }));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  expect(within(dialog).queryByText('フォルダーを作成しました。')).not.toBeInTheDocument();
  expect(within(dialog).getByLabelText('フォルダー名')).toBeDisabled();
});

test('pending/unknown payloadをsession/local storageへ書かない', async () => {
  const setItem = jest.spyOn(Storage.prototype, 'setItem'); const { api } = setup(); api.createFolder.mockRejectedValueOnce(new Error('lost'));
  const dialog = await open(); fill(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  expect(setItem).not.toHaveBeenCalled();
});

test('実Playwrightのexact labelは名前・理由の入力後とunknown再表示後にも一致する', async () => {
  const matches = require('./playwright-label-matcher.cjs')() as (element: Element, name: string) => boolean;
  const { api } = setup(); api.createFolder.mockRejectedValueOnce(new Error('lost'));
  const dialog = await open(); fill(dialog);
  const assertLabels = (container: HTMLElement) => {
    for (const label of ['フォルダー名', '作成理由']) {
      const field = within(container).getByLabelText(label) as HTMLTextAreaElement;
      expect(field.textContent).not.toBe(''); expect(matches(field, label)).toBe(true);
      expect(matches(field, `${label}${field.textContent}`)).toBe(false);
    }
  };
  assertLabels(dialog); submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' });
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); assertLabels(await open());
});
