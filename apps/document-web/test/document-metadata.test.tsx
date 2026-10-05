import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(),
  getDocument: jest.fn(), getDocumentVersion: jest.fn(), listVersionFiles: jest.fn(),
  listDocumentVersions: jest.fn(), listDocumentRevisions: jest.fn(), getDocumentHistory: jest.fn(),
  patchDocumentMetadata: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000010';
const otherId = '00000000-0000-4000-8000-000000000020';
const versionId = '00000000-0000-4000-8000-000000000011';
const available = { status: 'available' };
const denied = { status: 'disabled', reason: 'permission' };
const metadata = { document_type: '手順\r\n書', owning_department: ' 正本部署 ', category: '分類', department: '旧部署', documentType: '旧種別', extensions: { x: ['保持'] }, custom: '独自値' };
function detail(id = documentId, values: Record<string, unknown> = metadata, capability: unknown = available) {
  return { documentId: id, title: id === documentId ? '合成文書' : '別文書', view: 'published', folderId: '00000000-0000-4000-8000-000000000099', folderName: '共有', revision: 7,
    currentVersionId: versionId, metadata: values, unread: false, readState: { isRead: true, firstReadAt: null },
    displayVersion: { versionId, versionNo: 2, lifecycleState: 'PUBLISHED', updatedAt: '2026-10-01T00:00:00Z', fileSummary: { authoritativeItemCount: 0, totalSizeBytes: 0, primary: null } },
    displayRevision: { revisionId: 'rev', label: '2.4', major: 2, minor: 4 }, displayTimestamp: { kind: 'revisionCreatedAt', value: '2026-10-01T00:00:00Z' },
    capabilities: { updateMetadata: capability, createVersion: denied, manageAccess: denied, compareVersions: denied, endPublication: denied, moveDocument: denied } };
}
function setup(values: Record<string, unknown> = metadata, capability: unknown = available, entry = `/documents/${documentId}?view=published&tab=overview`) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation((id: string) => Promise.resolve(detail(id, values, capability)));
  api.getRootFolder.mockResolvedValue({ folderId: '00000000-0000-4000-8000-000000000099', name: 'ルート', capabilities: {} });
  api.listFolderChildren.mockResolvedValue({ items: [], nextCursor: null, capabilities: {} });
  api.listDocuments.mockResolvedValue({ view: 'published', items: [{ ...detail(), folderName: null }], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue({ capabilities: { download: denied } });
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.listDocumentVersions.mockResolvedValue({ items: [], nextCursor: null });
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.patchDocumentMetadata.mockImplementation((id: string, body: { operationId: string }) => Promise.resolve({ operationId: body.operationId, resourceId: id, changed: true, resultingRevision: 8, occurredAt: '2026-10-05T00:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const page = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const router = createRouter({ routeTree: root.addChildren([list, page]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  const view = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, invalidate, ...view };
}
async function open() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'メタデータを編集' }));
  const dialog = await screen.findByRole('dialog', { name: 'メタデータを編集' });
  return { user, dialog };
}
async function fillReason(dialog: HTMLElement, text = '  合成の変更理由  ') {
  fireEvent.change(within(dialog).getByLabelText('変更理由'), { target: { value: text } });
}
function save(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: '保存する' }).closest('form')!); }
function problem(code: string, status: number) { return { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false }; }
async function go(router: ReturnType<typeof setup>['router'], id: string) {
  await act(async () => { await router.navigate({ to: '/documents/$documentId', params: { documentId: id }, search: validateDetailSearch({ view: 'published', tab: 'overview' }) }); });
}

test('概要は正本snake_caseを主要表示し、legacy/extensionsをその他属性に残す', async () => {
  setup();
  await screen.findByRole('heading', { name: '基本情報' });
  const basics = screen.getByRole('heading', { name: '基本情報' }).closest('section')!;
  expect(within(basics).getByText('正本部署')).toBeVisible();
  expect(within(basics).queryByText('旧部署')).not.toBeInTheDocument();
  const extras = screen.getByRole('heading', { name: 'その他の属性' }).closest('section')!;
  expect(within(extras).getByText('department')).toBeVisible();
  expect(within(extras).getByText('旧部署')).toBeVisible();
  expect(within(extras).getByText('extensions')).toBeVisible();
});

test('Homeの選択パネルもsnake_caseを使いlegacy aliasへfallbackしない', async () => {
  setup(metadata, available, '/documents?view=published');
  await screen.findByRole('button', { name: '合成文書' });
  const panel = screen.getByRole('complementary', { name: '選択中の文書' });
  expect(await within(panel).findByText('正本部署')).toBeVisible();
  expect(within(panel).queryByText('旧部署')).not.toBeInTheDocument();
});

test('変更値と明示削除だけをPATCHし、未変更の改行や独自属性は送らない', async () => {
  const { api, invalidate } = setup();
  const { user, dialog } = await open();
  expect(within(dialog).getByLabelText('所管部署')).toHaveValue(' 正本部署 ');
  fireEvent.change(within(dialog).getByLabelText('所管部署'), { target: { value: ' 新部署 ' } });
  await user.click(within(dialog).getByLabelText('カテゴリを削除'));
  await fillReason(dialog); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(api.patchDocumentMetadata).toHaveBeenCalledTimes(1);
  expect(api.patchDocumentMetadata).toHaveBeenCalledWith(documentId, { operationId: expect.stringMatching(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/), expectedDocumentRevision: 7, set: { owning_department: ' 新部署 ' }, unset: ['category'], reason: '合成の変更理由' });
  for (const key of ['document', 'documents', 'document-versions', 'document-revisions', 'document-history', 'revision-comparison']) expect(invalidate).toHaveBeenCalledWith({ queryKey: key === 'documents' ? [key] : [key, documentId] });
  expect(screen.getByText('2.4')).toBeInTheDocument();
});

test('不在の空欄は未変更ならsetしない、明示編集した空文字と空白は値として保持する', async () => {
  const { api } = setup({ category: '' });
  const { dialog } = await open();
  fireEvent.change(within(dialog).getByLabelText('文書種別'), { target: { value: 'x' } });
  fireEvent.change(within(dialog).getByLabelText('文書種別'), { target: { value: '' } });
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: '  ' } });
  await fillReason(dialog); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(api.patchDocumentMetadata.mock.calls[0][1]).toMatchObject({ set: { document_type: '', category: '  ' }, unset: [] });
});

test('非文字列の現在値を表示して保持し、明示編集した項目だけ文字列へ置換する', async () => {
  const { api } = setup({ document_type: { old: true }, owning_department: null, category: 7 });
  const { dialog } = await open();
  expect(within(dialog).getByText('{"old":true}')).toBeVisible();
  expect(within(dialog).getByText('null')).toBeVisible();
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: '文字列' } });
  await fillReason(dialog); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(api.patchDocumentMetadata.mock.calls[0][1]).toMatchObject({ set: { category: '文字列' }, unset: [] });
});

test('無変更はサーバーへ期待revision付きで確認し、changed:falseのまま表示する', async () => {
  const { api } = setup({});
  api.patchDocumentMetadata.mockImplementation((id: string, body: { operationId: string }) => Promise.resolve({ operationId: body.operationId, resourceId: id, changed: false, resultingRevision: 7, occurredAt: '2026-10-05T00:00:00Z' }));
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  await within(dialog).findByText('変更はありませんでした。');
  expect(api.patchDocumentMetadata.mock.calls[0][1]).toMatchObject({ expectedDocumentRevision: 7, set: {}, unset: [] });
  expect(screen.getByText('2.4')).toBeInTheDocument();
});

test.each(['', '  ', '改\n行', '\u0085中\u0000', 'あ'.repeat(342)])('無効な理由では送信しない %p', async reason => {
  const { api } = setup(); const { dialog } = await open(); await fillReason(dialog, reason); save(dialog);
  expect(within(dialog).getByRole('button', { name: '保存する' })).toBeDisabled();
  expect(api.patchDocumentMetadata).not.toHaveBeenCalled();
});

test('UTF-8理由の1024 bytes境界とmetadata制御文字を区別する', async () => {
  const { api } = setup(); const { dialog } = await open();
  fireEvent.change(within(dialog).getByLabelText('文書種別'), { target: { value: ' 改\n行\t保持 ' } });
  await fillReason(dialog, 'あ'.repeat(341) + 'a'); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(api.patchDocumentMetadata.mock.calls[0][1]).toMatchObject({ set: { document_type: ' 改\n行\t保持 ' }, reason: 'あ'.repeat(341) + 'a' });
});

test.each([denied, { status: 'disabled', reason: 'pendingSchedule' }, undefined])('capability無効/不明では編集を開始できない %p', async capability => {
  setup(metadata, capability === undefined ? null : capability);
  await screen.findByRole('heading', { name: '基本情報' });
  const button = screen.queryByRole('button', { name: 'メタデータを編集' });
  if (button) expect(button).toBeDisabled();
});

test('二重submitと送信中の取消を抑止する', async () => {
  const { api } = setup(); api.patchDocumentMetadata.mockReturnValue(new Promise(() => {}));
  const { dialog, user } = await open(); await fillReason(dialog);
  act(() => { save(dialog); save(dialog); });
  expect(api.patchDocumentMetadata).toHaveBeenCalledTimes(1);
  expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toBeDisabled();
  await user.keyboard('{Escape}'); expect(dialog).toBeVisible();
});

test('取消・再表示で未送信入力を捨て、トリガーへfocusを戻す', async () => {
  const { api } = setup(); const { user, dialog } = await open();
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: '破棄する' } });
  await fillReason(dialog); await user.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await waitFor(() => expect(screen.getByRole('button', { name: 'メタデータを編集' })).toHaveFocus());
  const reopened = await open();
  expect(within(reopened.dialog).getByLabelText('カテゴリ')).toHaveValue('分類');
  expect(within(reopened.dialog).getByLabelText('変更理由')).toHaveValue('');
  expect(api.patchDocumentMetadata).not.toHaveBeenCalled();
});

test.each([['REVISION_CONFLICT', 409], ['FORBIDDEN', 403], ['RESERVED_DOCUMENT', 409]])('現在状態の拒否 %s を成功扱いせず再取得を促す', async (code, status) => {
  const { api } = setup(); api.patchDocumentMetadata.mockRejectedValue(problem(code as string, status as number));
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  await within(dialog).findByRole('alert');
  expect(within(dialog).queryByText('メタデータを更新しました。')).not.toBeInTheDocument();
  expect(within(dialog).queryByRole('button', { name: '同じ内容で再送' })).not.toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '最新の内容を読み直す' })).toBeEnabled();
});

test('通信断・閉じる・再表示後も同じ操作IDとpayloadだけを明示再送する', async () => {
  const { api, client } = setup(); api.patchDocumentMetadata.mockRejectedValueOnce(new Error('lost'));
  const { dialog, user } = await open();
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: '新分類' } });
  await fillReason(dialog); save(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再送' });
  const initial = JSON.stringify(api.patchDocumentMetadata.mock.calls[0]);
  expect(within(dialog).getByLabelText('カテゴリ')).toBeDisabled();
  await act(async () => { client.setQueryData(['document', documentId, 'published'], detail(documentId, { ...metadata, category: '新分類' })); });
  expect(within(dialog).queryByText('メタデータを更新しました。')).not.toBeInTheDocument();
  await user.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const again = await open();
  await again.user.click(within(again.dialog).getByRole('button', { name: '同じ内容で再送' }));
  await within(again.dialog).findByText('メタデータを更新しました。');
  expect(JSON.stringify(api.patchDocumentMetadata.mock.calls[1])).toBe(initial);
});

test('未知結果後の権限拒否も未確定のまま保持し、新しいpayloadを作らない', async () => {
  const { api } = setup(); api.patchDocumentMetadata.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403));
  const { dialog, user } = await open(); await fillReason(dialog); save(dialog);
  await user.click(await within(dialog).findByRole('button', { name: '同じ内容で再送' }));
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '同じ内容で再送' })).toBeEnabled());
  expect(within(dialog).getByLabelText('変更理由')).toBeDisabled();
  expect(within(dialog).queryByRole('button', { name: '最新の内容を読み直す' })).not.toBeInTheDocument();
  expect(api.patchDocumentMetadata.mock.calls[1]).toEqual(api.patchDocumentMetadata.mock.calls[0]);
});

test('URL別文書とBack/Forwardで未送信フォームを閉じ、古い入力を持ち越さない', async () => {
  const { api, router } = setup(); const { dialog } = await open(); await fillReason(dialog);
  await go(router, otherId); await screen.findByRole('heading', { name: '別文書' });
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  await act(async () => router.history.back());
  await screen.findByRole('heading', { name: '合成文書' });
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  const back = await open(); expect(within(back.dialog).getByLabelText('変更理由')).toHaveValue('');
  await act(async () => router.history.forward());
  await screen.findByRole('heading', { name: '別文書' }); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(api.patchDocumentMetadata).not.toHaveBeenCalled();
});

test('遷移後の遅延成功は別文書や再表示フォームへ適用せず、元文書だけ無効化する', async () => {
  const { api, router, invalidate } = setup(); let resolve!: (value: unknown) => void;
  api.patchDocumentMetadata.mockReturnValue(new Promise(done => { resolve = done; }));
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  const body = api.patchDocumentMetadata.mock.calls[0][1];
  await go(router, otherId); const other = await open();
  await act(async () => resolve({ operationId: body.operationId, resourceId: documentId, changed: true, resultingRevision: 8, occurredAt: '2026-10-05T00:00:00Z' }));
  expect(within(other.dialog).queryByText('メタデータを更新しました。')).not.toBeInTheDocument();
  expect(within(other.dialog).getByRole('button', { name: '保存する' })).toBeDisabled();
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['document', documentId] });
  expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['document', otherId] });
});

test('一覧を経たBackでも未確定操作をメモリー内だけに保持する', async () => {
  const { api, router } = setup(); api.patchDocumentMetadata.mockRejectedValueOnce(new Error('lost'));
  const stored = jest.spyOn(Storage.prototype, 'setItem');
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再送' });
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({}) }); });
  await screen.findByRole('heading', { name: '文書一覧' });
  await act(async () => router.history.back());
  const again = await open();
  expect(within(again.dialog).getByRole('button', { name: '同じ内容で再送' })).toBeEnabled();
  expect(api.patchDocumentMetadata).toHaveBeenCalledTimes(1);
  expect(stored).not.toHaveBeenCalled();
});

test('開いた後のcapability失効は保存を止め、既に入力したpayloadを送らない', async () => {
  const { api, client } = setup(); const { dialog } = await open(); await fillReason(dialog);
  await act(async () => { client.setQueryData(['document', documentId, 'published'], detail(documentId, metadata, denied)); });
  await waitFor(() => expect(within(dialog).getByRole('button', { name: '保存する' })).toBeDisabled());
  save(dialog); expect(api.patchDocumentMetadata).not.toHaveBeenCalled();
});

test('別operation/resourceの応答を成功にせず、元の操作だけを再送可能にする', async () => {
  const { api } = setup(); api.patchDocumentMetadata.mockResolvedValue({ operationId: 'wrong', resourceId: otherId, changed: true, resultingRevision: 8, occurredAt: '2026-10-05T00:00:00Z' });
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  await within(dialog).findByRole('button', { name: '同じ内容で再送' });
  expect(within(dialog).queryByText('メタデータを更新しました。')).not.toBeInTheDocument();
});

test('成功後の再取得失敗をmutation結果不明へ読み替えない', async () => {
  const { client } = setup(); jest.spyOn(client, 'invalidateQueries').mockRejectedValue(new Error('refresh lost'));
  const { dialog } = await open(); await fillReason(dialog); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(within(dialog).queryByRole('button', { name: '同じ内容で再送' })).not.toBeInTheDocument();
});

test('metadata patchのUTF-8 JSON 64KiB境界でだけ送信を止める', async () => {
  const { api } = setup(); const { dialog } = await open(); await fillReason(dialog);
  const overhead = JSON.stringify({ set: { category: '' }, unset: [] }).length;
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: 'x'.repeat(64 * 1024 - overhead + 1) } });
  expect(within(dialog).getByRole('button', { name: '保存する' })).toBeDisabled();
  save(dialog); expect(api.patchDocumentMetadata).not.toHaveBeenCalled();
  fireEvent.change(within(dialog).getByLabelText('カテゴリ'), { target: { value: 'x'.repeat(64 * 1024 - overhead) } });
  expect(within(dialog).getByRole('button', { name: '保存する' })).toBeEnabled();
});

test('理由のtrimはRust White_Spaceに揃えNELは除去しBOMは残す', async () => {
  const { api } = setup(); const { dialog } = await open(); await fillReason(dialog, '\u0085\uFEFF\u0085'); save(dialog);
  await within(dialog).findByText('メタデータを更新しました。');
  expect(api.patchDocumentMetadata.mock.calls[0][1].reason).toBe('\uFEFF');
});

test.each(['success', 'failure'])('古い読み直し %s は閉じる/再表示後の新しい入力を閉じず上書きしない', async outcome => {
  const { api } = setup(); api.patchDocumentMetadata.mockRejectedValue(problem('REVISION_CONFLICT', 409));
  const { dialog, user } = await open(); await fillReason(dialog); save(dialog);
  const refresh = await within(dialog).findByRole('button', { name: '最新の内容を読み直す' });
  let resolve!: (value: unknown) => void, reject!: (error: unknown) => void;
  api.getDocument.mockReturnValue(new Promise((yes, no) => { resolve = yes; reject = no; }));
  await user.click(refresh);
  await user.click(within(dialog).getByRole('button', { name: '閉じる' }));
  const again = await open();
  fireEvent.change(within(again.dialog).getByLabelText('カテゴリ'), { target: { value: '新しい未送信入力' } });
  await act(async () => { if (outcome === 'success') resolve(detail()); else reject(new Error('read failed')); });
  expect(again.dialog).toBeInTheDocument();
  expect(within(again.dialog).getByLabelText('カテゴリ')).toHaveValue('新しい未送信入力');
  expect(within(again.dialog).queryByText('最新の内容を取得できません。もう一度読み直してください。')).not.toBeInTheDocument();
});
