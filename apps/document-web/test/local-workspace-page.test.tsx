import { TextDecoder, TextEncoder } from 'node:util';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { LocalWorkspacePage } from '../src/routes/LocalWorkspacePage';
import { RuntimeProvider } from '../src/runtime/runtime-context';
import { browserRuntime } from '../src/runtime/browser-runtime';
import { RuntimeFailure, type RuntimeAdapter } from '../src/runtime/contract';
import { createFakeRuntime } from './local-runtime-fake';

Object.assign(globalThis, { TextDecoder, TextEncoder });

function renderPage(runtime: RuntimeAdapter) {
  const root = createRootRoute({ component: Outlet });
  const page = createRoute({ getParentRoute: () => root, path: '/local-workspaces', component: LocalWorkspacePage });
  const documents = createRoute({ getParentRoute: () => root, path: '/documents', component: () => <h1>文書</h1> });
  const router = createRouter({ routeTree: root.addChildren([page, documents]), history: createMemoryHistory({ initialEntries: ['/local-workspaces'] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<RuntimeProvider runtime={runtime}><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></RuntimeProvider>);
  return { client, router };
}

test('browser runtime states that local folders are unavailable and offers no fake actions', async () => {
  renderPage(browserRuntime);
  await screen.findByRole('heading', { name: 'ローカルWorkspace', level: 1 });
  expect(await screen.findByText(/ブラウザー版ではローカルフォルダーとWorkspaceを利用できません/)).toBeVisible();
  expect(screen.getByText('ブラウザー版')).toBeVisible();
  expect(screen.queryByRole('button', { name: '新しいWorkspace' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /フォルダーを追加/ })).not.toBeInTheDocument();
});

test('workspaces restored by the runtime are listed with managed and explicit folders', async () => {
  const fake = createFakeRuntime({ workspaces: [{ name: '見積案件', folders: { '共有資料': { 'a.txt': 'alpha' } } }, { name: '社内手続' }] });
  renderPage(fake.runtime);
  const list = await screen.findByRole('list', { name: 'ローカルWorkspace一覧' });
  expect(within(list).getByRole('button', { name: '見積案件' })).toHaveAttribute('aria-current', 'true');
  expect(within(list).getByRole('button', { name: '社内手続' })).toBeVisible();
  const folders = screen.getByRole('list', { name: '見積案件のフォルダー' });
  expect(within(folders).getByText('管理フォルダー')).toBeVisible();
  expect(within(folders).getByText('共有資料')).toBeVisible();
  expect(screen.getByText('デスクトップ版')).toBeVisible();
});

test('creating a workspace sends one operation, returns focus and selects it', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime();
  renderPage(fake.runtime);
  const trigger = await screen.findByRole('button', { name: '新しいWorkspace' });
  await user.click(trigger);
  const dialog = await screen.findByRole('dialog', { name: '新しいWorkspace' });
  const name = within(dialog).getByRole('textbox', { name: 'Workspace名' });
  expect(name).toHaveFocus();
  await user.type(name, '../見積/2026');
  const submit = within(dialog).getByRole('button', { name: '作成する' });
  await user.dblClick(submit);
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(fake.callsOf('createLocalWorkspace')).toHaveLength(1);
  expect(fake.callsOf('createLocalWorkspace')[0]!.args[0]).toBe('../見積/2026');
  expect(await screen.findByRole('button', { name: '../見積/2026' })).toHaveAttribute('aria-current', 'true');
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.getByRole('status')).toHaveTextContent('Workspace「../見積/2026」を作成しました');
});

test('escape closes the create dialog without sending anything', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime();
  renderPage(fake.runtime);
  const trigger = await screen.findByRole('button', { name: '新しいWorkspace' });
  await user.click(trigger);
  await user.type(await screen.findByRole('textbox', { name: 'Workspace名' }), 'draft');
  await user.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(fake.callsOf('createLocalWorkspace')).toHaveLength(0);
  await waitFor(() => expect(trigger).toHaveFocus());
});

test('cancelling the native picker sends no attach request; choosing attaches once', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W' }] });
  renderPage(fake.runtime);
  const add = await screen.findByRole('button', { name: 'フォルダーを追加' });
  fake.pick(null);
  await user.click(add);
  expect(await screen.findByText('フォルダーの選択を取り消しました。変更はありません。')).toBeVisible();
  expect(fake.callsOf('attachDirectory')).toHaveLength(0);
  fake.pick('営業資料');
  await user.click(add);
  const folders = await screen.findByRole('list', { name: 'Wのフォルダー' });
  expect(await within(folders).findByText('営業資料')).toBeVisible();
  expect(fake.callsOf('attachDirectory')).toHaveLength(1);
  expect(add).toHaveFocus();
});

test('detaching needs confirmation; escape keeps the binding', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W', folders: { '資料': {} } }] });
  renderPage(fake.runtime);
  const detach = await screen.findByRole('button', { name: '資料を解除' });
  expect(screen.queryByRole('button', { name: '管理フォルダーを解除' })).not.toBeInTheDocument();
  await user.click(detach);
  const dialog = await screen.findByRole('dialog', { name: 'フォルダーの解除' });
  expect(within(dialog).getByText(/フォルダーの中身は削除されません/)).toBeVisible();
  await user.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(fake.callsOf('detachDirectory')).toHaveLength(0);
  await waitFor(() => expect(detach).toHaveFocus());
  await user.click(detach);
  await user.click(within(await screen.findByRole('dialog', { name: 'フォルダーの解除' })).getByRole('button', { name: '解除する' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: '資料を解除' })).not.toBeInTheDocument());
  expect(fake.callsOf('detachDirectory')).toHaveLength(1);
});

test('browsing lists relative entries, opens directories and previews a bounded file', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W', folders: { '資料': { 'readme.txt': 'こんにちは', sub: { 'inner.txt': 'inner' } } } }] });
  fake.setOmitted('b_x3', 2);
  renderPage(fake.runtime);
  await user.click(await screen.findByRole('button', { name: '資料を開く' }));
  const table = await screen.findByRole('table', { name: '資料の内容' });
  expect(within(table).getByRole('button', { name: 'sub' })).toBeVisible();
  expect(screen.getByText('表示できない項目が2件あります（リンク・特殊なファイル・使用できない名前）。')).toBeVisible();
  await user.click(within(table).getByRole('button', { name: 'readme.txt の内容を表示' }));
  const preview = await screen.findByRole('region', { name: 'readme.txt の内容' });
  expect(within(preview).getByText('こんにちは')).toBeVisible();
  expect(fake.callsOf('closeRead')).toHaveLength(1);
  await user.click(within(table).getByRole('button', { name: 'sub' }));
  const inner = await screen.findByRole('table', { name: '資料 / subの内容' });
  expect(within(inner).getByText('inner.txt')).toBeVisible();
  expect(fake.callsOf('listEntries').at(-1)!.args[1]).toEqual({ bindingId: expect.any(String), locator: ['sub'] });
  await user.click(screen.getByRole('button', { name: '上の階層へ' }));
  expect(await screen.findByRole('table', { name: '資料の内容' })).toBeVisible();
});

test('creating a file keeps input on conflict and replays the same operation after an unknown result', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W', folders: { '資料': { 'exists.txt': 'x' } } }] });
  renderPage(fake.runtime);
  await user.click(await screen.findByRole('button', { name: '資料を開く' }));
  await screen.findByRole('table', { name: '資料の内容' });
  const form = screen.getByRole('form', { name: 'この場所にファイルを作成' });
  await user.type(within(form).getByRole('textbox', { name: 'ファイル名' }), 'exists.txt');
  await user.type(within(form).getByRole('textbox', { name: '内容' }), '本文');
  await user.click(within(form).getByRole('button', { name: '作成する' }));
  expect(await within(form).findByText(/同じ名前のファイルが既にあります/)).toBeVisible();
  expect(within(form).getByRole('textbox', { name: 'ファイル名' })).toHaveValue('exists.txt');
  const name = within(form).getByRole('textbox', { name: 'ファイル名' });
  await user.clear(name);
  await user.type(name, 'new.txt');
  fake.fail('createFile', new RuntimeFailure('outcome_unknown'));
  await user.click(within(form).getByRole('button', { name: '作成する' }));
  const confirm = await within(form).findByRole('button', { name: '結果を確認' });
  expect(within(form).getByRole('textbox', { name: 'ファイル名' })).toBeDisabled();
  await user.click(confirm);
  expect(await screen.findByRole('button', { name: 'new.txt の内容を表示' })).toBeVisible();
  const attempts = fake.callsOf('createFile').slice(-2);
  expect(attempts[0]!.args[4]).toBe(attempts[1]!.args[4]);
  expect(new TextDecoder().decode(attempts[1]!.args[3] as Uint8Array)).toBe('本文');
  expect(screen.getByRole('status')).toHaveTextContent('ファイル「new.txt」を作成しました');
});

test('a stale context refreshes the workspace instead of acting on old state', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W', folders: { '資料': { 'a.txt': 'a' } } }] });
  renderPage(fake.runtime);
  await screen.findByRole('button', { name: '資料を開く' });
  const before = fake.callsOf('listWorkspaces').length;
  fake.bumpContext();
  await user.click(screen.getByRole('button', { name: '資料を開く' }));
  expect(await screen.findByText(/Workspaceの状態が更新されました/)).toBeVisible();
  await waitFor(() => expect(fake.callsOf('listWorkspaces').length).toBeGreaterThan(before));
  await user.click(screen.getByRole('button', { name: '資料を開く' }));
  expect(await screen.findByRole('table', { name: '資料の内容' })).toBeVisible();
});

test('renaming changes only the logical name', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: '旧名' }] });
  renderPage(fake.runtime);
  await user.click(await screen.findByRole('button', { name: '名前を変更' }));
  const input = screen.getByRole('textbox', { name: '新しいWorkspace名' });
  await user.clear(input);
  await user.type(input, '新名{Enter}');
  expect(await screen.findByRole('button', { name: '新名' })).toBeVisible();
  expect(fake.callsOf('renameWorkspace')).toHaveLength(1);
  expect(fake.callsOf('renameWorkspace')[0]!.args[1]).toBe('新名');
});

test('an uncertain file creation survives navigation and blocks moving elsewhere until confirmed', async () => {
  const user = userEvent.setup();
  const fake = createFakeRuntime({ workspaces: [{ name: 'W1', folders: { '資料': { sub: { 'a.txt': 'a' } } } }, { name: 'W2' }] });
  const { router } = renderPage(fake.runtime);
  await user.click(await screen.findByRole('button', { name: '資料を開く' }));
  await user.click(within(await screen.findByRole('table', { name: '資料の内容' })).getByRole('button', { name: 'sub' }));
  await screen.findByRole('table', { name: '資料 / subの内容' });
  const form = screen.getByRole('form', { name: 'この場所にファイルを作成' });
  await user.type(within(form).getByRole('textbox', { name: 'ファイル名' }), 'memo.txt');
  fake.fail('createFile', new RuntimeFailure('outcome_unknown'));
  await user.click(within(form).getByRole('button', { name: '作成する' }));
  await within(form).findByRole('button', { name: '結果を確認' });
  // Moving elsewhere would abandon the retained operation, so it is blocked.
  expect(screen.getByRole('button', { name: '上の階層へ' })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'W2' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '資料を開く' })).toBeDisabled();
  expect(screen.getByText(/結果を確認していない操作があります/)).toBeVisible();
  // Leaving the screen and returning restores the same pending confirmation.
  await router.navigate({ to: '/documents' } as never);
  await screen.findByRole('heading', { name: '文書' });
  await router.navigate({ to: '/local-workspaces' } as never);
  const restored = await screen.findByRole('form', { name: 'この場所にファイルを作成' });
  expect(within(restored).getByRole('textbox', { name: 'ファイル名' })).toHaveValue('memo.txt');
  await user.click(within(restored).getByRole('button', { name: '結果を確認' }));
  expect(await screen.findByRole('button', { name: 'memo.txt の内容を表示' })).toBeVisible();
  const attempts = fake.callsOf('createFile');
  expect(attempts).toHaveLength(2);
  expect(attempts[0]!.args[4]).toBe(attempts[1]!.args[4]);
  expect(attempts[1]!.args[1]).toEqual({ bindingId: expect.any(String), locator: ['sub'] });
  expect(screen.getByRole('button', { name: 'W2' })).toBeEnabled();
});
