import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type DocumentDetail, type VersionDetail } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { validateDetailSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getDocument: jest.fn(), listDocumentVersions: jest.fn(), getDocumentVersion: jest.fn(),
  listDocumentRevisions: jest.fn(), getDocumentHistory: jest.fn(), listVersionFiles: jest.fn(),
  withdrawVersion: jest.fn(), endDocumentPublication: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000001';
const otherId = '00000000-0000-4000-8000-000000000002';
const versionId = '00000000-0000-4000-8000-000000000003';
const baseId = '00000000-0000-4000-8000-000000000004';
const available = { status: 'available' as const };
const denied = { status: 'disabled' as const, reason: 'permission' as const };
const version: VersionDetail = { versionId, versionNo: 2, baseVersionId: baseId, lifecycleState: 'published', isCurrent: true,
  createdAt: '2026-10-05T01:00:00Z', approvedAt: null, scheduledPublishAt: null, currentPublicationScheduleId: null, publishedAt: '2026-10-05T01:00:00Z', withdrawnAt: null,
  updatedAt: '2026-10-05T01:00:00Z', fileSummary: { authoritativeItemCount: 1, totalSizeBytes: 5, primary: { displayName: '合成.txt', mediaType: 'text/plain', sizeBytes: 5 } },
  firstReadAt: null, title: '合成文書', metadata: {}, capabilities: { edit: denied, rebase: denied, publish: denied, withdraw: available,
    schedulePublication: denied, cancelPublicationSchedule: denied, download: denied } };
function detail(id = documentId): DocumentDetail { return { documentId: id, documentVersionId: versionId, title: id === documentId ? '合成文書' : '別の合成文書', folderId: null,
  revision: 7, currentVersionId: versionId, displayVersion: { ...version, lifecycleState: 'PUBLISHED' }, displayRevision: null,
  readState: { isRead: false, firstReadAt: null }, displayTimestamp: { kind: 'workingUpdatedAt', value: version.updatedAt },
  capabilities: { createVersion: denied, updateMetadata: denied, moveDocument: denied, endPublication: available, manageAccess: denied, compareVersions: denied } }; }
const problem = (status: number, code: string) => ({ type: 'about:blank', title: '拒否', status, code, traceId: 'synthetic', retryable: status >= 500 });
const clients: QueryClient[] = [];
afterEach(() => { clients.splice(0).forEach(client => client.clear()); });
function setup() {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation((id: string) => Promise.resolve(detail(id)));
  api.listDocumentVersions.mockImplementation((_id: string, purpose: string) => Promise.resolve({ items: purpose === 'published' ? [version] : [], nextCursor: null }));
  api.getDocumentVersion.mockImplementation((_document: string, id: string, purpose: string) => id === versionId && purpose === 'published' ? Promise.resolve(version) : Promise.reject(problem(404, 'DOCUMENT_VERSION_NOT_FOUND')));
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.withdrawVersion.mockImplementation((id, target, body) => Promise.resolve({ operationId: body.operationId, documentId: id, targetVersionId: target,
    formerCurrentVersionId: versionId, resultingCurrentVersionId: baseId, resultingRevision: 8, restorationWithheldReason: null }));
  api.endDocumentPublication.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, documentId: id,
    formerCurrentVersionId: versionId, resultingCurrentVersionId: null, resultingDocumentRevision: 8, endedAt: '2026-10-05T02:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const history = createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=published&tab=versions`] });
  const router = createRouter({ routeTree: root.addChildren([route]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  clients.push(client);
  const view = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, history, ...view };
}
async function open(kind: 'withdraw' | 'end' = 'withdraw') {
  const user = userEvent.setup();
  const trigger = await screen.findByRole('button', { name: kind === 'withdraw' ? '選択版を取下げ' : '公開を終了' });
  await user.click(trigger);
  const dialog = await screen.findByRole('dialog', { name: kind === 'withdraw' ? '版の取下げを確認' : '文書の公開終了を確認' });
  return { user, dialog, trigger, confirm: within(dialog).getByRole('button', { name: kind === 'withdraw' ? '取下げを確定' : '公開終了を確定' }) };
}
async function reason(dialog: HTMLElement) { await userEvent.setup().type(within(dialog).getByLabelText('理由'), '合成の操作理由'); }

for (const kind of ['withdraw', 'end'] as const) {
  test(`${kind}: 影響と理由を確認し、キャンセルは送信せず入力を破棄する`, async () => {
    const { api } = setup(); const { user, dialog, trigger, confirm } = await open(kind);
    expect(confirm).toBeDisabled();
    expect(dialog).toHaveTextContent(kind === 'withdraw' ? '直前の公開版' : '再公開できません');
    await user.type(within(dialog).getByLabelText('理由'), '   '); expect(confirm).toBeDisabled();
    await user.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(api.withdrawVersion).not.toHaveBeenCalled(); expect(api.endDocumentPublication).not.toHaveBeenCalled();
    await user.click(trigger); expect(within(await screen.findByRole('dialog')).getByLabelText('理由')).toHaveValue('');
  });
}
test('現在capabilityだけで操作を提示し、未取得/拒否から推測しない', async () => {
  const { api } = setup(); api.getDocument.mockResolvedValue({ ...detail(), capabilities: { ...detail().capabilities, endPublication: denied } });
  api.getDocumentVersion.mockResolvedValue({ ...version, capabilities: { ...version.capabilities, withdraw: denied } });
  await screen.findByRole('heading', { name: '合成文書', level: 1 });
  expect(screen.queryByRole('button', { name: '選択版を取下げ' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '公開を終了' })).not.toBeInTheDocument();
});
test('取下げは選択版・期待revision・理由・UUIDv7を一度だけ送り、復帰を結果で表示する', async () => {
  const { api } = setup(); let finish!: (result: unknown) => void;
  api.withdrawVersion.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm); fireEvent.click(confirm);
  await waitFor(() => expect(api.withdrawVersion).toHaveBeenCalledTimes(1));
  const body = api.withdrawVersion.mock.calls[0]![2]; expect(body).toEqual({ operationId: expect.stringMatching(/^[\da-f-]{14}7[\da-f-]+$/), expectedRevision: 7, reason: '合成の操作理由' });
  expect(api.withdrawVersion.mock.calls[0]!.slice(0, 2)).toEqual([documentId, versionId]);
  await act(async () => finish({ operationId: body.operationId, documentId, targetVersionId: versionId, formerCurrentVersionId: versionId, resultingCurrentVersionId: baseId, resultingRevision: 8, restorationWithheldReason: null }));
  expect(await screen.findByText(/版を取下げました。直前の公開版へ復帰しました/)).toBeVisible();
});
test('公開終了はDocumentの現行IDを固定し、終了後404でも成功を残す', async () => {
  const { api, router } = setup();
  await screen.findByRole('button', { name: '公開を終了' });
  const { dialog, confirm } = await open('end'); await reason(dialog);
  api.endDocumentPublication.mockImplementation((id, body) => { api.getDocument.mockRejectedValue(problem(404, 'DOCUMENT_NOT_FOUND')); return Promise.resolve({ operationId: body.operationId, documentId: id,
    formerCurrentVersionId: versionId, resultingCurrentVersionId: null, resultingDocumentRevision: 8, endedAt: '2026-10-05T02:00:00Z' }); });
  fireEvent.click(confirm);
  expect(await screen.findByText(/文書の公開を終了しました/)).toBeVisible();
  expect(api.endDocumentPublication).toHaveBeenCalledWith(documentId, expect.objectContaining({ expectedRevision: 7, expectedCurrentVersionId: versionId, reason: '合成の操作理由' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: '公開を終了' })).not.toBeInTheDocument());
});
for (const kind of ['withdraw', 'end'] as const) {
  test(`${kind}: 結果不明後は理由もrevisionも変えず同じ要求だけを再送する`, async () => {
    const { api, client } = setup(); const method = kind === 'withdraw' ? api.withdrawVersion : api.endDocumentPublication;
    method.mockRejectedValueOnce({ ...problem(503, 'COMMIT_OUTCOME_UNKNOWN'), exactRetry: true });
    const { dialog, confirm } = await open(kind); await reason(dialog); fireEvent.click(confirm);
    const retry = await screen.findByRole('button', { name: '同じ内容で再試行' });
    expect(within(dialog).getByLabelText('理由')).toBeDisabled();
    act(() => client.setQueryData(['document', documentId, 'published'], { ...detail(), revision: 99 }));
    fireEvent.click(retry);
    await waitFor(() => expect(method).toHaveBeenCalledTimes(2)); expect(method.mock.calls[1]).toEqual(method.mock.calls[0]);
  });
}
for (const [status, code] of [[409, 'REVISION_CONFLICT'], [403, 'FORBIDDEN'], [422, 'VALIDATION_FAILED']] as const) {
  test(`${status}: 拒否後の自動再送をせず最新状態を確認する`, async () => {
    const { api } = setup(); api.withdrawVersion.mockRejectedValue(problem(status, code));
    const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
    const refresh = await screen.findByRole('button', { name: '最新状態を確認' });
    expect(screen.queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
    expect(api.withdrawVersion).toHaveBeenCalledTimes(1);
    api.getDocumentVersion.mockResolvedValue({ ...version, capabilities: { ...version.capabilities, withdraw: denied } });
    fireEvent.click(refresh);
    await waitFor(() => expect(screen.queryByRole('button', { name: '選択版を取下げ' })).not.toBeInTheDocument());
    expect(api.withdrawVersion).toHaveBeenCalledTimes(1);
  });
}
test('表示後の権限失効では確認を送信できない', async () => {
  const { client, api } = setup(); const { dialog, confirm } = await open(); await reason(dialog);
  act(() => client.setQueryData(['document-version', documentId, versionId, 'published'], { ...version, capabilities: { ...version.capabilities, withdraw: denied } }));
  await waitFor(() => expect(confirm).toBeDisabled()); fireEvent.click(confirm); expect(api.withdrawVersion).not.toHaveBeenCalled();
});
test('結果不明で閉じても別文書へ移動して戻っても同operationを保持し、別文書へ誤表示しない', async () => {
  const { api, router, history } = setup(); api.withdrawVersion.mockRejectedValueOnce(new TypeError('通信断'));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  await screen.findByRole('button', { name: '同じ内容で再試行' });
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'published', tab: 'versions' } }));
  expect(await screen.findByRole('heading', { name: '別の合成文書', level: 1 })).toBeVisible();
  expect(screen.queryByText('操作結果を確認できません')).not.toBeInTheDocument();
  await act(async () => history.back());
  const resume = await screen.findByRole('button', { name: '未確認の操作を開く' }); fireEvent.click(resume);
  fireEvent.click(await screen.findByRole('button', { name: '同じ内容で再試行' }));
  await waitFor(() => expect(api.withdrawVersion).toHaveBeenCalledTimes(2)); expect(api.withdrawVersion.mock.calls[1]).toEqual(api.withdrawVersion.mock.calls[0]);
});
test('送信中に別版へ移動して戻っても遅延応答は確認dialogを再表示しない', async () => {
  const { api, router, history } = setup(); let reject!: (reason: unknown) => void;
  api.withdrawVersion.mockImplementation(() => new Promise((_resolve, rejectPromise) => { reject = rejectPromise; }));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'published', tab: 'versions', versionId: baseId } }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await act(async () => history.back()); await act(async () => reject(new TypeError('遅延した通信断')));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  fireEvent.click(await screen.findByRole('button', { name: '未確認の操作を開く' }));
  expect(within(await screen.findByRole('dialog')).getByLabelText('理由')).toHaveValue('合成の操作理由');
  expect(api.withdrawVersion).toHaveBeenCalledTimes(1);
});
test('結果不明の再送が403でも前の結果を未実行と断定せず元要求を保持する', async () => {
  const { api } = setup(); api.withdrawVersion.mockRejectedValueOnce(new TypeError('通信断')).mockRejectedValueOnce(problem(403, 'FORBIDDEN'));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  fireEvent.click(await screen.findByRole('button', { name: '同じ内容で再試行' }));
  await waitFor(() => expect(api.withdrawVersion).toHaveBeenCalledTimes(2));
  expect(within(dialog).getByLabelText('理由')).toBeDisabled(); expect(screen.getByText('操作結果を確認できません')).toBeVisible();
  expect(screen.queryByRole('button', { name: '最新状態を確認' })).not.toBeInTheDocument();
});
test('成功応答の対象が不一致なら結果不明として固定要求を保持する', async () => {
  const { api } = setup(); api.withdrawVersion.mockResolvedValue({ operationId: 'wrong', documentId: otherId });
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  expect(await screen.findByRole('button', { name: '同じ内容で再試行' })).toBeVisible();
  expect(screen.getByText('操作結果を確認できません')).toBeVisible();
});
test.each([
  [null, 'validation_unavailable', '現行の公開版はありません。復帰候補の安全性を確認できなかったため、復帰していません。'],
  [versionId, null, '現行の公開版は変わりません。'],
])('取下げ結果の公開なし/過去版をAPI応答だけから区別する', async (resultingCurrentVersionId, restorationWithheldReason, message) => {
  const { api } = setup(); api.withdrawVersion.mockImplementation((id, target, body) => Promise.resolve({ operationId: body.operationId, documentId: id, targetVersionId: target,
    formerCurrentVersionId: versionId, resultingCurrentVersionId, resultingRevision: 8, restorationWithheldReason }));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  expect(await screen.findByText(`版を取下げました。${message}`)).toBeVisible();
});

test('結果不明の文書から別文書へ移動しても再読込で操作IDを失う前に警告する', async () => {
  const { api, router } = setup(); api.withdrawVersion.mockRejectedValueOnce(new TypeError('通信断'));
  const { dialog, confirm } = await open(); await reason(dialog); fireEvent.click(confirm);
  await screen.findByRole('button', { name: '同じ内容で再試行' });
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'published', tab: 'versions' } }));
  await screen.findByRole('heading', { name: '別の合成文書', level: 1 });
  const event = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
});

test('authoringのWORKING限定readから公開版の取下げcapabilityを捏造しない', async () => {
  const { api, router } = setup();
  await screen.findByRole('button', { name: '選択版を取下げ' });
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'authoring', tab: 'versions' } }));
  await waitFor(() => expect(api.listDocumentVersions).toHaveBeenLastCalledWith(documentId, 'authoring'));
  expect(screen.queryByRole('button', { name: '選択版を取下げ' })).not.toBeInTheDocument();
});

test('操作も結果も無い概要画面へ空の公開操作パネルを追加しない', async () => {
  const { router } = setup(); await screen.findByRole('heading', { name: '合成文書', level: 1 });
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId }, search: { view: 'published', tab: 'overview' } }));
  expect(screen.queryByRole('region', { name: '公開状態の操作' })).not.toBeInTheDocument();
});
