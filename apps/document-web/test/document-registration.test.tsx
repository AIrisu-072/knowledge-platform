import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(),
  getDocument: jest.fn(), getDocumentVersion: jest.fn(), createDocument: jest.fn(),
  recoverDocumentCreation: jest.fn(),
} }));

const rootId = '00000000-0000-4000-8000-000000000001';
const childId = '00000000-0000-4000-8000-000000000002';
const result = { documentId: '00000000-0000-4000-8000-000000000003', documentVersionId: '00000000-0000-4000-8000-000000000004', fileId: '00000000-0000-4000-8000-000000000005' };
const available = { status: 'available' };
const denied = { status: 'disabled', reason: 'permission' };

function setup(entry = '/documents?view=authoring') {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'ルート', revision: 1, parentFolderId: null, capabilities: { createDocument: available } });
  api.listFolderChildren.mockImplementation((id: string) => Promise.resolve({ items: id === rootId ? [{ folderId: childId, name: '共有文書', revision: 1, parentFolderId: rootId }] : [], nextCursor: null, capabilities: { createDocument: available } }));
  api.listDocuments.mockResolvedValue({ view: 'authoring', items: [], nextCursor: null });
  api.createDocument.mockResolvedValue(result);
  api.recoverDocumentCreation.mockResolvedValue(result);
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const detail = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: () => <h1>登録した文書の詳細</h1> });
  const router = createRouter({ routeTree: root.addChildren([list, detail]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  const view = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, ...view };
}

beforeEach(() => window.sessionStorage.clear());

async function openForm() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: '文書を登録' }));
  const dialog = await screen.findByRole('dialog', { name: '文書を登録' });
  return { user, dialog };
}

async function fillForm() {
  const { user, dialog } = await openForm();
  const file = new File(['合成の原本文書'], '初回文書.txt', { type: 'text/plain' });
  await user.type(within(dialog).getByLabelText('文書名'), '新しい手順書');
  await user.upload(within(dialog).getByLabelText('原本ファイル'), file);
  return { user, dialog, file };
}

test('ルートへ初回WORKINGを1件登録し、公開せずauthoring詳細へ移動する', async () => {
  const { api, router } = setup('/documents?view=published');
  const { user, dialog, file } = await fillForm();
  expect(within(dialog).getByText('ルート')).toBeVisible();
  // jsdom FileList does not satisfy native required-file validity; the real browser journey clicks this button.
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await screen.findByRole('heading', { name: '登録した文書の詳細' });
  expect(api.createDocument).toHaveBeenCalledTimes(1);
  expect(api.createDocument).toHaveBeenCalledWith({ folderId: rootId, title: '新しい手順書', documentMetadata: {}, versionMetadata: {} }, file);
  expect(router.state.location.pathname).toBe(`/documents/${result.documentId}`);
  expect(router.state.location.search).toMatchObject({ view: 'authoring', tab: 'versions', versionId: result.documentVersionId });
});

test('子フォルダー自身のcapabilityを確認し、そのIDだけへ登録する', async () => {
  const { api } = setup();
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: '共有文書' }));
  const { dialog, file } = await fillForm();
  expect(within(dialog).getByText('共有文書')).toBeVisible();
  // jsdom FileList does not satisfy native required-file validity; the real browser journey clicks this button.
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(api.createDocument).toHaveBeenCalledWith(expect.objectContaining({ folderId: childId }), file));
  expect(api.listFolderChildren).toHaveBeenCalledWith(childId);
});

test('子フォルダーの登録権限なしを親の権限で上書きしない', async () => {
  const { api } = setup();
  api.listFolderChildren.mockImplementation((id: string) => Promise.resolve({ items: id === rootId ? [{ folderId: childId, name: '共有文書', revision: 1 }] : [], nextCursor: null, capabilities: { createDocument: id === rootId ? available : denied } }));
  await userEvent.setup().click(await screen.findByRole('button', { name: '共有文書' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '文書を登録' })).toBeDisabled());
  expect(api.createDocument).not.toHaveBeenCalled();
});

test('初回登録のcapability未取得では登録導線を表示しない', async () => {
  const { api } = setup();
  api.getRootFolder.mockResolvedValue({ folderId: rootId, name: 'ルート', revision: 1, parentFolderId: null, capabilities: {} });
  await screen.findByRole('heading', { name: '編集作業' });
  await screen.findByRole('button', { name: 'ルート' });
  expect(screen.queryByRole('button', { name: '文書を登録' })).not.toBeInTheDocument();
  expect(api.createDocument).not.toHaveBeenCalled();
});

test('空入力と上限超過では送信せず、取消時にボタンへフォーカスを戻す', async () => {
  const { api } = setup();
  const { user, dialog } = await openForm();
  expect(within(dialog).getByRole('button', { name: '下書きとして登録' })).toBeDisabled();
  await user.type(within(dialog).getByLabelText('文書名'), '大きいファイル');
  const file = new File(['x'], '大.txt', { type: 'text/plain' });
  Object.defineProperty(file, 'size', { value: 256 * 1024 * 1024 + 1 });
  await user.upload(within(dialog).getByLabelText('原本ファイル'), file);
  expect(within(dialog).getByRole('alert')).toHaveTextContent('256 MiB');
  expect(within(dialog).getByRole('button', { name: '下書きとして登録' })).toBeDisabled();
  await user.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await waitFor(() => expect(screen.getByRole('button', { name: '文書を登録' })).toHaveFocus());
  expect(api.createDocument).not.toHaveBeenCalled();
});

test('連続submitは初回POSTを一度しか実行しない', async () => {
  const { api } = setup();
  let resolve!: (value: typeof result) => void;
  api.createDocument.mockReturnValue(new Promise(done => { resolve = done; }));
  const { dialog } = await fillForm();
  const form = within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!;
  act(() => { fireEvent.submit(form); fireEvent.submit(form); });
  expect(api.createDocument).toHaveBeenCalledTimes(1);
  expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toBeDisabled();
  await act(async () => resolve(result));
});

test('通信断後は入力変更・閉じる・再表示でも再POSTを許可しない', async () => {
  const { api } = setup();
  api.createDocument.mockRejectedValue(new Error('connection lost'));
  const { user, dialog } = await fillForm();
  // jsdom FileList does not satisfy native required-file validity; the real browser journey clicks this button.
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent('再登録しない'));
  expect(within(dialog).getByLabelText('文書名')).toHaveValue('新しい手順書');
  expect(within(dialog).getByLabelText('文書名')).toBeDisabled();
  expect(within(dialog).queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
  await user.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const reopened = await openForm();
  expect(within(reopened.dialog).queryByRole('button', { name: '下書きとして登録' })).not.toBeInTheDocument();
  expect(api.createDocument).toHaveBeenCalledTimes(1);
});

test('結果不明はremount後も保持し、作成フォームへ自動復帰しない', async () => {
  const view = setup();
  view.api.createDocument.mockRejectedValue(new Error('response lost'));
  const { user, dialog } = await fillForm();
  // jsdom FileList does not satisfy native required-file validity; the real browser journey clicks this button.
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent('再登録しない'));
  view.unmount();
  const next = setup();
  const reopened = await openForm();
  expect(within(reopened.dialog).queryByRole('button', { name: '下書きとして登録' })).not.toBeInTheDocument();
  expect(next.api.createDocument).not.toHaveBeenCalled();
});

test('回復IDがある結果不明はGETだけで確認し、404後も再POSTしない', async () => {
  const { api } = setup();
  const problem = { type: 'about:blank', title: 'Unknown', status: 503, code: 'COMMIT_OUTCOME_UNKNOWN', traceId: 'synthetic', retryable: false, recovery: { ...result, recoveryEndpoint: 'https://untrusted.invalid/ignored' } };
  api.createDocument.mockRejectedValue({ problem });
  api.recoverDocumentCreation.mockRejectedValueOnce({ type: 'about:blank', title: 'Not found', status: 404, code: 'DOCUMENT_NOT_FOUND', traceId: 'synthetic', retryable: false });
  const { user, dialog } = await fillForm();
  // jsdom FileList does not satisfy native required-file validity; the real browser journey clicks this button.
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await user.click(await within(dialog).findByRole('button', { name: '登録結果を確認' }));
  await waitFor(() => expect(api.recoverDocumentCreation).toHaveBeenCalledWith(result));
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '登録結果を確認' })).toBeEnabled());
  expect(within(dialog).queryByRole('button', { name: '下書きとして登録' })).not.toBeInTheDocument();
  await user.click(within(dialog).getByRole('button', { name: '登録結果を確認' }));
  await screen.findByRole('heading', { name: '登録した文書の詳細' });
  expect(api.createDocument).toHaveBeenCalledTimes(1);
  expect(api.recoverDocumentCreation).toHaveBeenCalledTimes(2);
});

test('取消後に開き直すと未送信の入力だけを消し、登録は実行しない', async () => {
  const { api } = setup();
  const { user, dialog } = await fillForm();
  await user.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(dialog).not.toBeInTheDocument();
  const reopened = await openForm();
  expect(within(reopened.dialog).getByLabelText('文書名')).toHaveValue('');
  expect(within(reopened.dialog).getByLabelText('原本ファイル')).toHaveValue('');
  expect(api.createDocument).not.toHaveBeenCalled();
});

test('フォルダー遷移後の遅延成功は画面を移動させず、結果を明示して開ける', async () => {
  const { api, router } = setup();
  let resolve!: (value: typeof result) => void;
  api.createDocument.mockReturnValue(new Promise(done => { resolve = done; }));
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'authoring', folderId: childId }) }); });
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await act(async () => resolve(result));
  expect(router.state.location.pathname).toBe('/documents');
  expect(router.state.location.search).toMatchObject({ folderId: childId });
  const reopened = await openForm();
  expect(within(reopened.dialog).getByRole('status')).toHaveTextContent('下書きとして登録しました');
  await reopened.user.click(within(reopened.dialog).getByRole('button', { name: '登録した文書を開く' }));
  await screen.findByRole('heading', { name: '登録した文書の詳細' });
  expect(api.createDocument).toHaveBeenCalledTimes(1);
});

test('現在の権限で確実に拒否された入力は保持し、自動では再送しない', async () => {
  const { api } = setup();
  api.createDocument.mockRejectedValueOnce({ type: 'about:blank', title: 'Forbidden', status: 403, code: 'FORBIDDEN', traceId: 'synthetic', retryable: false });
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent('権限がありません'));
  expect(within(dialog).getByLabelText('文書名')).toHaveValue('新しい手順書');
  expect(within(dialog).getByLabelText('文書名')).toBeEnabled();
  expect(api.createDocument).toHaveBeenCalledTimes(1);
  expect(window.sessionStorage.length).toBe(0);
});

test('別フォルダーから元のURLへ戻っても古い登録成功は自動遷移しない', async () => {
  const { api, router } = setup();
  let resolve!: (value: typeof result) => void;
  api.createDocument.mockReturnValue(new Promise(done => { resolve = done; }));
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'authoring', folderId: childId }) }); });
  await act(async () => { router.history.back(); });
  await waitFor(() => expect(router.state.location.search.folderId).toBeUndefined());
  await act(async () => resolve(result));
  expect(router.state.location.pathname).toBe('/documents');
  expect(api.createDocument).toHaveBeenCalledTimes(1);
});

test('未送信のままフォルダーを切り替えて開き直すと古い入力を持ち越さない', async () => {
  const { api, router } = setup();
  await fillForm();
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'authoring', folderId: childId }) }); });
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  const { dialog } = await openForm();
  expect(within(dialog).getByLabelText('文書名')).toHaveValue('');
  expect(within(dialog).getByLabelText('原本ファイル')).toHaveValue('');
  expect(within(dialog).getByRole('button', { name: '下書きとして登録' })).toBeDisabled();
  expect(api.createDocument).not.toHaveBeenCalled();
});

test('遷移後に届く明確な拒否は古い入力を新しい登録先へ戻さない', async () => {
  const { api, router } = setup();
  let reject!: (error: unknown) => void;
  api.createDocument.mockReturnValue(new Promise((_resolve, fail) => { reject = fail; }));
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'authoring', folderId: childId }) }); });
  await act(async () => reject({ type: 'about:blank', title: 'Validation', status: 422, code: 'VALIDATION_FAILED', traceId: 'synthetic', retryable: false }));
  const reopened = await openForm();
  expect(within(reopened.dialog).getByLabelText('文書名')).toHaveValue('');
  expect(within(reopened.dialog).getByLabelText('原本ファイル')).toHaveValue('');
  expect(within(reopened.dialog).getByRole('button', { name: '下書きとして登録' })).toBeDisabled();
});

test('以前の登録結果を照会中に今のフォルダーを登録先として表示しない', async () => {
  const { api, router } = setup();
  api.createDocument.mockRejectedValue({ type: 'about:blank', title: 'Unknown', status: 503, code: 'COMMIT_OUTCOME_UNKNOWN', traceId: 'synthetic', retryable: false, recovery: { ...result, recoveryEndpoint: '/unused' } });
  api.recoverDocumentCreation.mockReturnValue(new Promise(() => {}));
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await within(dialog).findByRole('button', { name: '登録結果を確認' });
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'authoring', folderId: childId }) }); });
  const reopened = await openForm();
  await reopened.user.click(within(reopened.dialog).getByRole('button', { name: '登録結果を確認' }));
  expect(within(reopened.dialog).queryByText(/登録先：/)).not.toBeInTheDocument();
});

test('未解決markerにはタイトルや原本本文を保存しない', async () => {
  const { api } = setup();
  api.createDocument.mockReturnValue(new Promise(() => {}));
  const { dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  const stored = Array.from({ length: window.sessionStorage.length }, (_, index) => window.sessionStorage.getItem(window.sessionStorage.key(index)!)).join('');
  expect(stored).toContain('unknown');
  expect(stored).not.toMatch(/新しい手順書|初回文書|合成の原本文書/);
});

test('既存契約の422入力拒否は未知結果にせず、修正後の明示登録を許可する', async () => {
  const { api } = setup();
  api.createDocument.mockRejectedValueOnce({ type: 'about:blank', title: 'Validation', status: 422, code: 'VALIDATION_FAILED', traceId: 'synthetic', retryable: false });
  const { user, dialog } = await fillForm();
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent('入力内容を確認'));
  expect(within(dialog).getByLabelText('文書名')).toBeEnabled();
  await user.clear(within(dialog).getByLabelText('文書名'));
  await user.type(within(dialog).getByLabelText('文書名'), '修正した手順書');
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await screen.findByRole('heading', { name: '登録した文書の詳細' });
  expect(api.createDocument).toHaveBeenCalledTimes(2);
});

test('未解決markerの保存が拒否されたら初回POSTを送信しない', async () => {
  const { api } = setup();
  const { dialog } = await fillForm();
  jest.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('storage denied'); });
  fireEvent.submit(within(dialog).getByRole('button', { name: '下書きとして登録' }).closest('form')!);
  await waitFor(() => expect(within(dialog).getByRole('alert')).toBeVisible());
  expect(api.createDocument).not.toHaveBeenCalled();
});
