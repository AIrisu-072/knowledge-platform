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
  getVersionEditManifest: jest.fn(), downloadVersionFile: jest.fn(), prepareVersionUpload: jest.fn(), createVersion: jest.fn(),
  updateWorkingVersion: jest.fn(), rebaseWorkingVersion: jest.fn(), publishVersion: jest.fn(), schedulePublication: jest.fn(),
} }));
const documentId = '00000000-0000-4000-8000-000000000001';
const otherId = '00000000-0000-4000-8000-000000000002';
const versionId = '00000000-0000-4000-8000-000000000003';
const baseId = '00000000-0000-4000-8000-000000000004';
const available = { status: 'available' as const };
const denied = { status: 'disabled' as const, reason: 'permission' as const };
const version: VersionDetail = { versionId, versionNo: 2, baseVersionId: baseId, lifecycleState: 'working', isCurrent: false,
  createdAt: '2026-10-05T01:00:00Z', approvedAt: null, scheduledPublishAt: null, currentPublicationScheduleId: null, publishedAt: null, withdrawnAt: null,
  updatedAt: '2026-10-05T01:00:00Z', fileSummary: { authoritativeItemCount: 2, totalSizeBytes: 28, primary: { displayName: '原本A.txt', mediaType: 'text/plain', sizeBytes: 7 } },
  firstReadAt: null, title: '合成文書', metadata: {}, capabilities: { edit: available, rebase: denied, publish: available, withdraw: denied,
    schedulePublication: available, cancelPublicationSchedule: denied, download: available } };
function detail(id = documentId): DocumentDetail { return { documentId: id, documentVersionId: versionId, title: id === documentId ? '合成文書' : '別の合成文書', folderId: null,
  revision: 7, currentVersionId: baseId, displayVersion: { ...version, lifecycleState: 'WORKING' }, displayRevision: null,
  readState: { isRead: false, firstReadAt: null }, displayTimestamp: { kind: 'workingUpdatedAt', value: version.updatedAt },
  capabilities: { createVersion: denied, updateMetadata: denied, moveDocument: denied, endPublication: denied, manageAccess: denied, compareVersions: denied } }; }
const representation = (id: string, filename: string, role: 'authoritative' | 'rendition' = 'authoritative') => ({ role, representationId: `representation-${id}`, fileId: `file-${id}`, originalFilename: filename, mediaType: 'text/plain', sizeBytes: 7 });
function manifest() { return { documentId, sourceVersionId: versionId, documentRevision: 7, purpose: 'authoring' as const, title: '合成文書', items: [
  { contentItemId: 'item-a', logicalPath: 'folder/original-a', ordinal: 2, representations: [representation('a', '正確な 原本A.txt'), representation('a-r', '表示A.txt', 'rendition')] },
  { contentItemId: 'item-b', logicalPath: 'original-b', ordinal: 9, representations: [representation('b', '原本B-正確.txt'), representation('b-r', '表示B.txt', 'rendition')] },
] }; }
const problem = (status: number, code: string) => ({ type: 'about:blank', title: '拒否', status, code, traceId: 'synthetic', retryable: status >= 500 });
const clients: QueryClient[] = [];
afterEach(() => { clients.splice(0).forEach(client => client.clear()); });
function setup(input: { published?: boolean; view?: 'published' | 'authoring' } = {}) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  const current = { ...version, versionId: baseId, lifecycleState: 'published' as const, isCurrent: true, capabilities: { ...version.capabilities, edit: denied } };
  const doc = input.published ? { ...detail(), documentVersionId: baseId, displayVersion: { ...current, lifecycleState: 'PUBLISHED' as const }, capabilities: { ...detail().capabilities, createVersion: available } } : detail();
  api.getDocument.mockImplementation((id: string) => Promise.resolve({ ...doc, documentId: id, title: id === documentId ? doc.title : '別の合成文書' }));
  api.listDocumentVersions.mockResolvedValue({ items: [input.published ? current : version], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue(input.published ? current : version);
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null }); api.listVersionFiles.mockResolvedValue({ items: [] });
  api.getVersionEditManifest.mockResolvedValue({ ...manifest(), sourceVersionId: input.published ? baseId : versionId, purpose: input.published ? 'published' : 'authoring' });
  api.downloadVersionFile.mockResolvedValue(new Blob(['content']));
  api.prepareVersionUpload.mockImplementation(() => ({ body: new Blob(['wire']), contentType: 'multipart/form-data; boundary=fixed' }));
  const success = (id: string, body: { operationId: string; targetVersionId: string; expectedRevision: number }) => ({ operationId: body.operationId, documentId: id, targetVersionId: body.targetVersionId, versionNo: 2, baseVersionId: baseId, resultingRevision: body.expectedRevision + 1 });
  api.updateWorkingVersion.mockImplementation((id, _target, body) => Promise.resolve(success(id, body)));
  api.createVersion.mockImplementation((id, body) => Promise.resolve(success(id, body)));
  api.rebaseWorkingVersion.mockImplementation((id, target, body) => Promise.resolve(success(id, { ...body, targetVersionId: target })));
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const history = createMemoryHistory({ initialEntries: [`/documents/${documentId}?view=${input.view ?? (input.published ? 'published' : 'authoring')}&tab=versions`] });
  const router = createRouter({ routeTree: root.addChildren([route]), history });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }); clients.push(client);
  render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, client, router, history };
}
async function open(published = false) { fireEvent.click(await screen.findByRole('button', { name: published ? '新しい版を作成' : '作業版を編集' })); return screen.findByRole('form', { name: '作業版の原本を編集' }); }
async function replace(form: HTMLElement, item = '正確な 原本A.txt') {
  const file = new File(['changed'], '差替.txt', { type: 'text/plain' });
  await userEvent.setup().upload(within(form).getByLabelText(`差替ファイル: ${item}（固定パス: folder/original-a、順序: 2）`), file);
  return file;
}
test('複数原本の固定path/ordinalと選択原本だけのrendition除外を明示し、他原本を完全保持する', async () => {
  const { api } = setup(); const form = await open();
  expect(within(form).getByText('folder/original-a')).toBeVisible();
  expect(within(form).getByText('original-b')).toBeVisible();
  expect(within(form).getByRole('button', { name: '作業版を保存' })).toBeEnabled();
  const replacement = await replace(form);
  expect(within(form).getByText(/除外する補助ファイル: 1件/)).toBeVisible();
  expect(within(form).getByText('表示A.txt')).toBeVisible();
  fireEvent.submit(form);
  await screen.findByText('作業版を保存しました。');
  expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1); expect(api.createVersion).not.toHaveBeenCalled();
  const [id, target, body, files] = api.updateWorkingVersion.mock.calls[0];
  expect([id, target]).toEqual([documentId, versionId]);
  expect(body).toMatchObject({ targetVersionId: versionId, expectedRevision: 7, title: '合成文書', items: [
    { logicalPath: 'folder/original-a', ordinal: 2, originalFilename: '差替.txt', renditions: [] },
    { logicalPath: 'original-b', ordinal: 9, fileId: 'file-b', originalFilename: '原本B-正確.txt', renditions: [{ fileId: 'file-b-r', originalFilename: '表示B.txt' }] },
  ] });
  expect(body.items[0].fileId).not.toBe('file-a'); expect(files.get(body.items[0].partId)).toBe(replacement);
  expect(api.downloadVersionFile.mock.calls.map(([request]) => request)).toEqual([
    { documentId, versionId, contentItemId: 'item-b', representationId: 'representation-b', purpose: 'authoring' },
    { documentId, versionId, contentItemId: 'item-b', representationId: 'representation-b-r', purpose: 'authoring' },
  ]);
});
test('公開版からは全manifest読込だけでは複製せず、変更保存時だけPOSTする', async () => {
  const { api } = setup({ published: true });
  // Header and version-list actions have the same title; select the list action.
  fireEvent.click((await screen.findAllByRole('button', { name: '新しい版を作成' }))[0]!);
  const form = await screen.findByRole('form', { name: '作業版の原本を編集' });
  expect(api.createVersion).not.toHaveBeenCalled(); expect(api.downloadVersionFile).not.toHaveBeenCalled();
  await replace(form); fireEvent.submit(form); await screen.findByText('新しい作業版を作成しました。');
  expect(api.createVersion).toHaveBeenCalledTimes(1); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
  expect(api.getVersionEditManifest).toHaveBeenCalledWith(documentId, baseId, 'published');
  expect(api.downloadVersionFile.mock.calls.every(([request]) => request.purpose === 'published' && request.versionId === baseId)).toBe(true);
});
test.each(['FORBIDDEN', 'COMMIT_OUTCOME_UNKNOWN'])('保持ファイル取得が%sなら書込を一切送らない', async code => {
  const { api } = setup(); api.downloadVersionFile.mockRejectedValue(problem(code === 'FORBIDDEN' ? 403 : 503, code));
  const form = await open(); await replace(form); fireEvent.submit(form);
  expect(await screen.findByRole('alert')).toHaveTextContent('保存は送信していません');
  expect(api.updateWorkingVersion).not.toHaveBeenCalled(); expect(api.createVersion).not.toHaveBeenCalled();
});
test('結果不明後はnavigation・revision変更・再拒否でもID/全payload/bytesを固定し再downloadしない', async () => {
  const { api, client, router, history } = setup();
  api.updateWorkingVersion.mockRejectedValueOnce(problem(503, 'COMMIT_OUTCOME_UNKNOWN')).mockRejectedValueOnce(problem(403, 'FORBIDDEN'));
  const form = await open(); await replace(form); fireEvent.submit(form); fireEvent.submit(form);
  await screen.findByRole('button', { name: '同じ内容で再試行' });
  expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1);
  act(() => client.setQueryData(['document', documentId, 'authoring'], { ...detail(), revision: 88 }));
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'authoring', tab: 'versions' } }));
  expect(await screen.findByRole('heading', { name: '別の合成文書', level: 1 })).toBeVisible();
  expect(screen.queryByText('保存結果を確認できません')).not.toBeInTheDocument();
  await act(async () => history.back());
  fireEvent.click(await screen.findByRole('button', { name: '同じ内容で再試行' }));
  await waitFor(() => expect(api.updateWorkingVersion).toHaveBeenCalledTimes(2));
  expect(api.updateWorkingVersion.mock.calls[1]).toEqual(api.updateWorkingVersion.mock.calls[0]);
  expect(api.downloadVersionFile).toHaveBeenCalledTimes(2);
  expect(screen.getByText('保存結果を確認できません')).toBeVisible();
  expect(screen.queryByRole('button', { name: '最新状態を確認' })).not.toBeInTheDocument();
});
test('準備中のキャンセルは遅いdownload応答後も書込を送信しない', async () => {
  const { api } = setup(); let resolve!: (value: Blob) => void;
  api.downloadVersionFile.mockImplementation(() => new Promise(done => { resolve = done; }));
  const form = await open(); await replace(form); fireEvent.submit(form);
  await waitFor(() => expect(api.downloadVersionFile).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole('button', { name: '準備をキャンセル' }));
  await act(async () => resolve(new Blob(['content'])));
  expect(api.updateWorkingVersion).not.toHaveBeenCalled(); expect(api.downloadVersionFile).toHaveBeenCalledTimes(1);
});
test('編集中の権限失効で保存を禁止する', async () => {
  const { api, client } = setup(); const form = await open(); await replace(form);
  act(() => client.setQueryData(['document-version', documentId, versionId, 'authoring'], { ...version, capabilities: { ...version.capabilities, edit: denied } }));
  await waitFor(() => expect(within(form).getByRole('button', { name: '作業版を保存' })).toBeDisabled());
  fireEvent.submit(form); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
});

test('authoringの一覧が公開版だけの場合もcreate意図はpublished manifestからPOSTする', async () => {
  const { api } = setup({ published: true, view: 'authoring' });
  fireEvent.click((await screen.findAllByRole('button', { name: '新しい版を作成' }))[0]!);
  const form = await screen.findByRole('form', { name: '作業版の原本を編集' }); await replace(form); fireEvent.submit(form);
  await screen.findByText('新しい作業版を作成しました。');
  expect(api.getVersionEditManifest).toHaveBeenCalledWith(documentId, baseId, 'published');
  expect(api.createVersion).toHaveBeenCalledTimes(1); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
});
test('確定済み成功後の次の新版作成では新しいフォームを開く', async () => {
  const { api } = setup({ published: true });
  fireEvent.click((await screen.findAllByRole('button', { name: '新しい版を作成' }))[0]!);
  const form = await screen.findByRole('form', { name: '作業版の原本を編集' }); await replace(form); fireEvent.submit(form);
  await screen.findByText('新しい作業版を作成しました。');
  fireEvent.click(screen.getAllByRole('button', { name: '版の一覧へ戻る' })[0]!);
  fireEvent.click((await screen.findAllByRole('button', { name: '新しい版を作成' }))[0]!);
  expect(await screen.findByRole('form', { name: '作業版の原本を編集' })).toBeVisible();
  expect(screen.queryByText('新しい作業版を作成しました。')).not.toBeInTheDocument();
  expect(api.createVersion).toHaveBeenCalledTimes(1);
});
test('確定した拒否では選択ファイルをreadonlyで保持し、確認前に再送しない', async () => {
  const { api } = setup(); api.updateWorkingVersion.mockRejectedValueOnce(problem(403, 'FORBIDDEN'));
  const form = await open(); const file = await replace(form); const input = within(form).getByLabelText('差替ファイル: 正確な 原本A.txt（固定パス: folder/original-a、順序: 2）') as HTMLInputElement;
  fireEvent.submit(form); await screen.findByRole('button', { name: '最新状態を確認' });
  expect(input).toBeInTheDocument(); expect(input).toBeDisabled(); expect(input.files?.[0]).toBe(file);
  expect(screen.queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
  fireEvent.submit(form); expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1);
});

test('取消後すぐ再準備しても古い応答が新準備を完了表示しない', async () => {
  const { api } = setup(); const resolves: Array<(value: Blob) => void> = [];
  api.downloadVersionFile.mockImplementation(() => new Promise(done => { resolves.push(done); }));
  const form = await open(); await replace(form); fireEvent.submit(form);
  await waitFor(() => expect(resolves).toHaveLength(1)); fireEvent.click(screen.getByRole('button', { name: '準備をキャンセル' }));
  fireEvent.submit(form); await waitFor(() => expect(resolves).toHaveLength(2));
  await act(async () => resolves[0]!(new Blob(['content'])));
  expect(screen.getByRole('button', { name: '準備をキャンセル' })).toBeVisible(); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '準備をキャンセル' })); await act(async () => resolves[1]!(new Blob(['content'])));
});
test('準備中に別文書へ移動したら遅いdownloadから保存を送らない', async () => {
  const { api, router } = setup(); let resolve!: (value: Blob) => void;
  api.downloadVersionFile.mockImplementation(() => new Promise(done => { resolve = done; }));
  const form = await open(); await replace(form); fireEvent.submit(form);
  await waitFor(() => expect(api.downloadVersionFile).toHaveBeenCalledTimes(1));
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'authoring', tab: 'versions' } }));
  await act(async () => resolve(new Blob(['content'])));
  expect(api.updateWorkingVersion).not.toHaveBeenCalled(); expect(api.downloadVersionFile).toHaveBeenCalledTimes(1);
});
test('初回WORKINGはcurrent/base nullでも同じ版へPUTしnull baseの成功を受け取る', async () => {
  const { api } = setup();
  api.getDocument.mockResolvedValue({ ...detail(), currentVersionId: null, displayVersion: { ...detail().displayVersion, baseVersionId: null, versionNo: 1 } });
  api.getDocumentVersion.mockResolvedValue({ ...version, baseVersionId: null, versionNo: 1 });
  api.updateWorkingVersion.mockImplementation((id, target, body) => Promise.resolve({ operationId: body.operationId, documentId: id, targetVersionId: target, versionNo: 1, baseVersionId: null, resultingRevision: 8 }));
  const form = await open(); fireEvent.submit(form); await screen.findByText('作業版を保存しました。');
  expect(api.downloadVersionFile).toHaveBeenCalledTimes(4); expect(api.createVersion).not.toHaveBeenCalled();
  expect(api.updateWorkingVersion.mock.calls[0]![1]).toBe(versionId);
});
test.each([[409, 'REVISION_CONFLICT'], [403, 'FORBIDDEN'], [422, 'BUSINESS_RULE_REJECTED']] as const)('初回の%s拒否は同ID自動再送せず最新状態の確認を要求する', async (status, code) => {
  const { api } = setup(); api.updateWorkingVersion.mockRejectedValueOnce(problem(status, code));
  const form = await open(); await replace(form); fireEvent.submit(form);
  fireEvent.click(await screen.findByRole('button', { name: '最新状態を確認' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: '最新状態を確認' })).not.toBeInTheDocument());
  expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1);
});
test('manifest取得拒否ではhistoryへ切替えず原本編集も書込も出さない', async () => {
  const { api } = setup(); api.getVersionEditManifest.mockRejectedValue(problem(403, 'FORBIDDEN'));
  fireEvent.click(await screen.findByRole('button', { name: '作業版を編集' }));
  await screen.findByRole('alert'); expect(screen.queryByRole('form', { name: '作業版の原本を編集' })).not.toBeInTheDocument();
  expect(api.getVersionEditManifest).toHaveBeenCalledTimes(1); expect(api.getVersionEditManifest).toHaveBeenCalledWith(documentId, versionId, 'authoring');
  expect(api.downloadVersionFile).not.toHaveBeenCalled(); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
});
test('rebaseは明示確認のみで現行版へ更新し原本は読み直さない', async () => {
  const { api } = setup(); api.getDocumentVersion.mockResolvedValue({ ...version, capabilities: { ...version.capabilities, edit: denied, rebase: available } });
  fireEvent.click(await screen.findByRole('button', { name: '現行版へ基準を更新' }));
  const dialog = await screen.findByRole('dialog', { name: '作業版の基準更新を確認' });
  expect(dialog).toHaveTextContent(baseId); expect(api.rebaseWorkingVersion).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' })); expect(api.rebaseWorkingVersion).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '現行版へ基準を更新' })); fireEvent.click(screen.getByRole('button', { name: '基準更新を確定' }));
  await screen.findByText('作業版の基準を現行の公開版へ更新しました。');
  expect(api.rebaseWorkingVersion).toHaveBeenCalledWith(documentId, versionId, { operationId: expect.any(String), expectedRevision: 7 });
  expect(api.getVersionEditManifest).not.toHaveBeenCalled(); expect(api.downloadVersionFile).not.toHaveBeenCalled();
});
test('rebase確認後のcurrent変更と権限失効は古い基準で送信しない', async () => {
  const { api, client } = setup(); api.getDocumentVersion.mockResolvedValue({ ...version, capabilities: { ...version.capabilities, rebase: available } });
  fireEvent.click(await screen.findByRole('button', { name: '現行版へ基準を更新' }));
  act(() => client.setQueryData(['document', documentId, 'authoring'], { ...detail(), revision: 9, currentVersionId: otherId }));
  const confirm = screen.getByRole('button', { name: '基準更新を確定' }); await waitFor(() => expect(confirm).toBeDisabled()); fireEvent.click(confirm);
  expect(api.rebaseWorkingVersion).not.toHaveBeenCalled();
});
test('結果不明操作は全queryの再読込でも消えずブラウザー再読込に警告する', async () => {
  const { api, client } = setup(); api.updateWorkingVersion.mockRejectedValueOnce(new TypeError('lost'));
  const form = await open(); await replace(form); fireEvent.submit(form); await screen.findByText('保存結果を確認できません');
  await act(async () => client.refetchQueries());
  expect(screen.getByText('保存結果を確認できません')).toBeVisible(); expect(api.getVersionEditManifest).toHaveBeenCalledTimes(1);
  const unload = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(unload); expect(unload.defaultPrevented).toBe(true);
});

test('保存結果不明中は旧原本の公開と予約公開を開始できない', async () => {
  const { api } = setup(); api.updateWorkingVersion.mockRejectedValueOnce(new TypeError('lost'));
  const form = await open(); await replace(form); fireEvent.submit(form); await screen.findByText('保存結果を確認できません');
  fireEvent.click(screen.getAllByRole('button', { name: '版の一覧へ戻る' })[0]!);
  const publish = await screen.findByRole('button', { name: '公開する' }); const schedule = screen.getByRole('button', { name: '予約公開する' });
  expect(publish).toBeDisabled(); expect(schedule).toBeDisabled(); fireEvent.click(publish); fireEvent.click(schedule);
  expect(screen.queryByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' })).not.toBeInTheDocument();
  expect(api.publishVersion).not.toHaveBeenCalled(); expect(api.schedulePublication).not.toHaveBeenCalled();
});
test('manifestより古い文書revisionは明示再読込で編集可能な状態へ復帰する', async () => {
  const { api } = setup(); api.getVersionEditManifest.mockResolvedValue({ ...manifest(), documentRevision: 8 });
  const form = await open(); expect(within(form).getByRole('button', { name: '作業版を保存' })).toBeDisabled();
  api.getDocument.mockResolvedValue({ ...detail(), revision: 8 });
  fireEvent.click(within(form).getByRole('button', { name: '最新状態を確認' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '作業版を保存' })).toBeEnabled());
  expect(api.getDocument).toHaveBeenCalledTimes(2); expect(api.getDocumentVersion).toHaveBeenCalledTimes(2);
  expect(api.getVersionEditManifest).toHaveBeenCalledTimes(2); expect(api.updateWorkingVersion).not.toHaveBeenCalled();
});

test('rebase結果不明後のcurrent変更と再拒否でも元の対象・revision・operationを再送する', async () => {
  const { api, client } = setup(); api.getDocumentVersion.mockResolvedValue({ ...version, capabilities: { ...version.capabilities, edit: denied, rebase: available } });
  api.rebaseWorkingVersion.mockRejectedValueOnce(problem(503, 'COMMIT_OUTCOME_UNKNOWN')).mockRejectedValueOnce(problem(403, 'FORBIDDEN'));
  fireEvent.click(await screen.findByRole('button', { name: '現行版へ基準を更新' })); fireEvent.click(screen.getByRole('button', { name: '基準更新を確定' }));
  const retry = await screen.findByRole('button', { name: '同じ内容で再試行' });
  act(() => client.setQueryData(['document', documentId, 'authoring'], { ...detail(), revision: 99, currentVersionId: otherId }));
  fireEvent.click(retry); await waitFor(() => expect(api.rebaseWorkingVersion).toHaveBeenCalledTimes(2));
  expect(api.rebaseWorkingVersion.mock.calls[1]).toEqual(api.rebaseWorkingVersion.mock.calls[0]);
  expect(screen.getByText('保存結果を確認できません')).toBeVisible(); expect(screen.queryByRole('button', { name: '最新状態を確認' })).not.toBeInTheDocument();
  expect(api.getVersionEditManifest).not.toHaveBeenCalled(); expect(api.downloadVersionFile).not.toHaveBeenCalled();
});
test('既に開いた公開確認も保存結果不明の同期cache guardで送信を止める', async () => {
  const { api, client } = setup(); fireEvent.click(await screen.findByRole('button', { name: '公開する' }));
  fireEvent.click(await screen.findByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }));
  fireEvent.click(screen.getByRole('button', { name: '公開する' }));
  const dialog = await screen.findByRole('dialog', { name: '公開を確認' }); const confirm = within(dialog).getByRole('button', { name: '確定する' });
  act(() => {
    client.setQueryData(['document-working-operation', documentId], { status: 'unknown', intent: { kind: 'update', documentId, sourceVersionId: versionId,
      body: { operationId: 'fixed', targetVersionId: versionId, expectedRevision: 7, title: '合成文書', items: [] }, files: new Map(), prepared: { body: new Blob(), contentType: 'multipart/form-data' } } });
    fireEvent.click(confirm);
  });
  expect(api.publishVersion).not.toHaveBeenCalled(); await waitFor(() => expect(confirm).toBeDisabled());
});
test('送信中に別文書へ移動した後の成功は元文書だけへ記録し戻るまで誤表示しない', async () => {
  const { api, router, history } = setup(); let resolve!: (value: unknown) => void;
  api.updateWorkingVersion.mockImplementation(() => new Promise(done => { resolve = done; }));
  const form = await open(); await replace(form); fireEvent.submit(form); await screen.findByText('保存結果を確認しています…');
  await act(async () => router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: { view: 'authoring', tab: 'versions' } }));
  const [id, target, body] = api.updateWorkingVersion.mock.calls[0]!;
  await act(async () => resolve({ operationId: body.operationId, documentId: id, targetVersionId: target, versionNo: 2, baseVersionId: baseId, resultingRevision: 8 }));
  expect(screen.queryByText('作業版を保存しました。')).not.toBeInTheDocument(); expect(await screen.findByRole('heading', { name: '別の合成文書' })).toBeVisible();
  await act(async () => history.back());
  // Returning to the workflow starts a fresh draft only after confirmed success, never a replay.
  await screen.findByRole('form', { name: '作業版の原本を編集' }); expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1);
});

test('authoringで公開Version detailが404でもcurrent published manifestから新版作成できる', async () => {
  const { api } = setup({ published: true, view: 'authoring' });
  api.listDocumentVersions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentVersion.mockRejectedValue(problem(404, 'DOCUMENT_VERSION_NOT_FOUND'));
  fireEvent.click((await screen.findAllByRole('button', { name: '新しい版を作成' }))[0]!);
  const form = await screen.findByRole('form', { name: '作業版の原本を編集' }); await replace(form); fireEvent.submit(form);
  await screen.findByText('新しい作業版を作成しました。');
  expect(api.getVersionEditManifest).toHaveBeenCalledWith(documentId, baseId, 'published'); expect(api.createVersion).toHaveBeenCalledTimes(1);
});

test('同名原本は固定パスと順序で一意に選び、対象contentItemの原本と補助ファイルだけを差し替える', async () => {
  const { api } = setup();
  const duplicateNames = manifest();
  for (const item of duplicateNames.items) item.representations[0]!.originalFilename = '同名原本.txt';
  api.getVersionEditManifest.mockResolvedValue(duplicateNames);
  const form = await open();
  const first = within(form).getByLabelText('差替ファイル: 同名原本.txt（固定パス: folder/original-a、順序: 2）') as HTMLInputElement;
  const second = within(form).getByLabelText('差替ファイル: 同名原本.txt（固定パス: original-b、順序: 9）') as HTMLInputElement;
  const replacement = new File(['changed'], '差替.txt', { type: 'text/plain' });
  await userEvent.setup().upload(second, replacement);
  expect(first.files).toHaveLength(0);
  expect(second.files?.[0]).toBe(replacement);
  const firstRow = first.closest('li')!, secondRow = second.closest('li')!;
  expect(within(firstRow).getByText('保持する補助ファイル: 1件')).toBeVisible();
  expect(within(firstRow).getByText('表示A.txt')).toBeVisible();
  expect(within(secondRow).getByText('除外する補助ファイル: 1件')).toBeVisible();
  expect(within(secondRow).getByText('表示B.txt')).toBeVisible();
  fireEvent.submit(form);
  await screen.findByText('作業版を保存しました。');
  expect(api.updateWorkingVersion).toHaveBeenCalledTimes(1);
  const [id, target, body, files] = api.updateWorkingVersion.mock.calls[0];
  expect([id, target]).toEqual([documentId, versionId]);
  expect(body.items).toEqual([
    expect.objectContaining({ logicalPath: 'folder/original-a', ordinal: 2, fileId: 'file-a', originalFilename: '同名原本.txt',
      renditions: [expect.objectContaining({ fileId: 'file-a-r', originalFilename: '表示A.txt' })] }),
    expect.objectContaining({ logicalPath: 'original-b', ordinal: 9, originalFilename: '差替.txt', renditions: [] }),
  ]);
  expect(body.items[1].fileId).not.toBe('file-b');
  expect(files.get(body.items[1].partId)).toBe(replacement);
  expect(files.size).toBe(3);
  expect(api.downloadVersionFile.mock.calls.map(([request]) => request)).toEqual([
    { documentId, versionId, contentItemId: 'item-a', representationId: 'representation-a', purpose: 'authoring' },
    { documentId, versionId, contentItemId: 'item-a', representationId: 'representation-a-r', purpose: 'authoring' },
  ]);
  expect(api.createVersion).not.toHaveBeenCalled();
});
