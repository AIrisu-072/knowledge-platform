/** @jest-environment ./test/fetch-dom-environment.cjs */
import { documentAccessPolicyOperations } from '../src/application/document-access-policy';
import { folderAccessPolicyOperations } from '../src/application/document-folder-access-policy';
import { client as wireClient } from '../../../packages/document-api-client/src/generated/client.gen';
import { documentApi as wireApi } from '../src/api/document-api';

jest.mock('@knowledge-platform/document-api-client', () => jest.requireActual('../../../packages/document-api-client/src/index'), { virtual: true });

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
  patchDocumentMetadata: jest.fn(), getDocumentAccessPolicy: jest.fn(), setDocumentAccessPolicy: jest.fn(),
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
    capabilities: { updateMetadata: capability, createVersion: denied, manageAccess: capability, compareVersions: denied, endPublication: denied, moveDocument: denied } };
}
const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach(client => { [documentAccessPolicyOperations(client), folderAccessPolicyOperations(client)].forEach(store => { const operation = store.get(); if (operation) { store.put({ ...operation, status: 'succeeded' } as never); store.clearSettled(store.get()! as never); } }); client.clear(); }));
function setup(values: Record<string, unknown> = metadata, capability: unknown = available, entry = `/documents/${documentId}?view=published&tab=access`) {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach(mock => mock.mockReset());
  api.getDocument.mockImplementation((id: string) => Promise.resolve(detail(id, values, capability)));
  api.getRootFolder.mockResolvedValue({ folderId: '00000000-0000-4000-8000-000000000099', name: 'ルート', capabilities: {} });
  api.listFolderChildren.mockResolvedValue({ items: [], nextCursor: null, capabilities: {} });
  api.listDocuments.mockResolvedValue({ view: 'published', items: [{ ...detail(), folderName: null }], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue({ currentPublicationScheduleId: null, capabilities: { download: denied, withdraw: denied } });
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.listDocumentVersions.mockResolvedValue({ items: [], nextCursor: null });
  api.listDocumentRevisions.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.getDocumentAccessPolicy.mockResolvedValue(policy());
  api.setDocumentAccessPolicy.mockImplementation(wireApi.setDocumentAccessPolicy);
  wireClient.setConfig({ baseUrl: 'https://synthetic.invalid', fetch: async () => { throw new TypeError('response lost'); } });
  api.patchDocumentMetadata.mockImplementation((id: string, body: { operationId: string }) => Promise.resolve({ operationId: body.operationId, resourceId: id, changed: true, resultingRevision: 8, occurredAt: '2026-10-05T00:00:00Z' }));
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const page = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const router = createRouter({ routeTree: root.addChildren([list, page]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  clients.push(client);
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  const view = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { api, router, client, invalidate, ...view };
}

function policy(revision = 3) { return { target: { kind: 'document', id: documentId }, bindingMode: 'explicit', policyId: 'local-policy', policyRevision: revision, effectivePolicyId: 'local-policy', effectiveSource: { kind: 'document', id: documentId }, effectiveGrants: [{ subjectKind: 'group', identityProvider: 'directory', subjectId: 'reviewers', actions: ['read', 'write'], presentation: { ref: { kind: 'group', provider: 'directory', subjectId: 'reviewers' }, displayName: '確認者', secondaryText: null, resolution: 'resolved' } }] }; }
async function fill() { await screen.findByRole('heading', { name: 'アクセス設定' }); await screen.findByLabelText('変更理由'); fireEvent.change(screen.getByLabelText('変更理由'), { target: { value: '最初の理由' } }); }
function save() { fireEvent.click(screen.getByRole('button', { name: 'アクセス設定を保存' })); }

// The real route, real operation ID generator, application API and generated SDK
// feed the fetch boundary. A constant operationId mock would hide this regression.
test('実AccessTabの応答喪失と背景policy再読取後もPUTの固定IDとJSON本文を保持する', async () => {
  const { api, client } = setup();
  const requests: { url: string; method: string; body: string }[] = [];
  wireClient.setConfig({ fetch: async input => { const request = input as Request; requests.push({ url: request.url, method: request.method, body: await request.text() }); throw new TypeError('response lost'); } });
  await fill(); save();
  await screen.findByRole('button', { name: '同じ内容で再試行' });
  api.getDocumentAccessPolicy.mockResolvedValue({ ...policy(4), effectiveGrants: policy(4).effectiveGrants.map(grant => ({ ...grant, actions: ['read'] })) });
  await act(async () => { await client.refetchQueries({ queryKey: ['document-access-policy', documentId] }); await new Promise(resolve => setTimeout(resolve, 0)); });
  fireEvent.click(screen.getByRole('button', { name: '同じ内容で再試行' }));
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(requests[0]!.method).toBe('PUT');
  expect(requests[0]!.url).toBe(`https://synthetic.invalid/v1/documents/${documentId}/access-policy`);
  expect(JSON.parse(requests[0]!.body)).toEqual({ operationId: expect.stringMatching(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/), expectedPolicyRevision: 3, mode: 'explicit', reason: '最初の理由', grants: [{ subjectKind: 'group', identityProvider: 'directory', subjectId: 'reviewers', actions: ['read', 'write'] }] });
  expect(requests[1]).toEqual(requests[0]);
});


test.each(['input', 'tab', 'home'])('UNKNOWN後%sを経ても同一wire要求だけ再送できる', async path => {
  const { router } = setup(); const bodies: string[] = [];
  wireClient.setConfig({ fetch: async input => { const request = input as Request; bodies.push(await request.text()); throw new TypeError('lost'); } });
  await fill(); save(); await screen.findByRole('button', { name: '同じ内容で再試行' });
  if (path === 'input') { expect(screen.getByLabelText('変更理由')).toBeDisabled(); expect(screen.getByLabelText('編集')).toBeDisabled(); }
  else if (path === 'tab') { fireEvent.click(screen.getByRole('tab', { name: '概要' })); await screen.findByRole('heading', { name: '基本情報' }); fireEvent.click(screen.getByRole('tab', { name: 'アクセス' })); }
  else { await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'published' }) }); }); fireEvent.click(await screen.findByRole('button', { name: '文書アクセス設定の保存結果' })); }
  fireEvent.click(await screen.findByRole('button', { name: '同じ内容で再試行' })); await waitFor(() => expect(bodies).toHaveLength(2)); expect(bodies[1]).toBe(bodies[0]);
});

test('同tick二重保存と保存中の編集を抑止する', async () => {
  setup(); const bodies: string[] = []; wireClient.setConfig({ fetch: async input => { const request = input as Request; bodies.push(await request.text()); return new Promise(() => {}); } });
  await fill(); const button = screen.getByRole('button', { name: 'アクセス設定を保存' });
  act(() => { fireEvent.submit(button.closest('form')!); fireEvent.submit(button.closest('form')!); });
  await waitFor(() => expect(bodies).toHaveLength(1)); expect(screen.getByLabelText('変更理由')).toBeDisabled(); expect(screen.getByLabelText('編集')).toBeDisabled();
});

test('異なるtargetのGET policyを保存権限の根拠にしない', async () => {
  const { api } = setup(); api.getDocumentAccessPolicy.mockResolvedValue({ ...policy(), target: { kind: 'folder', id: documentId } });
  await screen.findByRole('heading', { name: 'アクセス設定' }); await screen.findByText(/最新の文書とアクセス設定を確認できません/);
  expect(screen.queryByRole('button', { name: 'アクセス設定を保存' })).not.toBeInTheDocument(); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

function problem(code: string, status: number) { return { type: 'about:blank', title: 'Synthetic', code, status, traceId: 'synthetic', retryable: false }; }
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { resolve, promise }; }
async function flushReads(client: QueryClient, key: readonly unknown[]) { await act(async () => { await client.refetchQueries({ queryKey: key }); await new Promise(resolve => setTimeout(resolve, 0)); }); }

test.each([403, 404, 409])('自己失権でAccess tab消失後の%sもUNKNOWN要求をHome/Detailに保持する', async status => {
  const { api, client, router } = setup(); const requests: { url: string; body: string }[] = [];
  wireClient.setConfig({ fetch: async input => { const request = input as Request; requests.push({ url: request.url, body: await request.text() }); if (requests.length === 1) throw new TypeError('lost'); return Response.json(problem(status === 403 ? 'FORBIDDEN' : status === 404 ? 'DOCUMENT_NOT_FOUND' : 'REVISION_CONFLICT', status), { status }); } });
  await fill(); save(); await screen.findByRole('button', { name: '同じ内容で再試行' });
  api.getDocument.mockRejectedValue(problem('FORBIDDEN', 403)); api.getDocumentAccessPolicy.mockRejectedValue(problem('FORBIDDEN', 403));
  await flushReads(client, ['document', documentId]); expect(screen.queryByRole('tab', { name: 'アクセス' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '文書アクセス設定の保存結果' }));
  const dialog = await screen.findByRole('dialog', { name: '文書アクセス設定の保存結果' });
  fireEvent.click(within(dialog).getByRole('button', { name: '同じ内容で再試行' })); await within(dialog).findByText(/初回結果は未確定のまま/);
  expect(requests).toHaveLength(2); expect(requests[1]).toEqual(requests[0]); expect(within(dialog).queryByRole('button', { name: '確認して閉じる' })).not.toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' }));
  await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'published' }) }); });
  fireEvent.click(await screen.findByRole('button', { name: '文書アクセス設定の保存結果' }));
  expect(await screen.findByRole('dialog', { name: '文書アクセス設定の保存結果' })).toHaveTextContent(JSON.parse(requests[0]!.body).operationId);
});

test('同値no-op receiptは正規GET一致と分離して保持し往復後も成功結果を確認できる', async () => {
  const { api, client, router } = setup(); let body: Record<string, unknown> = {};
  wireClient.setConfig({ fetch: async input => { body = await (input as Request).json(); api.getDocument.mockRejectedValue(problem('FORBIDDEN', 403)); return Response.json({ operationId: body.operationId, resourceId: documentId, resultingRevision: 3, changed: false, occurredAt: '2026-10-07T00:00:00Z' }); } });
  client.setQueryData(['document-version-files', documentId, versionId, 'published'], { items: ['old'] });
  await fill(); save(); await waitFor(() => expect(screen.queryByRole('tab', { name: 'アクセス' })).not.toBeInTheDocument());
  fireEvent.click(await screen.findByRole('button', { name: '文書アクセス設定の保存結果' }));
  const dialog = await screen.findByRole('dialog', { name: '文書アクセス設定の保存結果' });
  expect(await within(dialog).findByText('アクセス設定の変更はありませんでした。')).toBeVisible();
  expect(dialog).toHaveTextContent(String(body.operationId)); expect(client.getQueryData(['document-version-files', documentId, versionId, 'published'])).toBeUndefined();
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); await act(async () => { await router.navigate({ to: '/documents', search: validateListSearch({ view: 'published' }) }); });
  fireEvent.click(await screen.findByRole('button', { name: '文書アクセス設定の保存結果' })); const held = await screen.findByRole('dialog', { name: '文書アクセス設定の保存結果' });
  expect(held).toHaveTextContent(String(body.operationId)); fireEvent.click(within(held).getByRole('button', { name: '確認して閉じる' })); await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
});

test('未送信draftは同revisionの実効権限変更を無言置換せずfresh見直し後だけ送信する', async () => {
  const { api, client } = setup(); await fill(); fireEvent.click(screen.getByLabelText('編集'));
  api.getDocumentAccessPolicy.mockResolvedValue({ ...policy(), effectiveSource: { kind: 'folder', id: 'new-parent' }, effectiveGrants: policy().effectiveGrants.map(grant => ({ ...grant, actions: ['read', 'write', 'publish'] })) });
  await flushReads(client, ['document-access-policy', documentId]); expect(screen.getByLabelText('変更理由')).toHaveValue('最初の理由'); expect(screen.queryByLabelText('編集')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'アクセス設定を保存' })).toBeDisabled(); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '最新の状態を取得して見直す' })); await waitFor(() => expect(screen.getByRole('button', { name: 'アクセス設定を保存' })).toBeEnabled());
  expect(screen.getByLabelText('編集')).not.toBeChecked(); expect(screen.getByLabelText('公開')).not.toBeChecked(); save(); await screen.findByRole('button', { name: '同じ内容で再試行' });
  expect(api.setDocumentAccessPolicy.mock.calls[0]![1].grants[0].actions).toEqual(['read']);
});

test('送信直前fresh read中に入力/二重送信を止め背景変更を送らない', async () => {
  const { api } = setup(); await fill(); const read = deferred<ReturnType<typeof policy>>(); api.getDocumentAccessPolicy.mockReturnValue(read.promise); save();
  await waitFor(() => expect(api.getDocumentAccessPolicy).toHaveBeenCalledTimes(2)); expect(screen.getByLabelText('変更理由')).toBeDisabled();
  fireEvent.submit(screen.getByRole('button', { name: 'アクセス設定を保存' }).closest('form')!);
  await act(async () => read.resolve(policy(4))); await screen.findByText(/変更案を保持しています/); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('送信前read中に別文書へ移動したら旧対象へ送らない', async () => {
  const { api, router } = setup(); await fill(); const read = deferred<ReturnType<typeof policy>>(); api.getDocumentAccessPolicy.mockReturnValue(read.promise); save();
  await waitFor(() => expect(api.getDocumentAccessPolicy).toHaveBeenCalledTimes(2));
  await act(async () => { await router.navigate({ to: '/documents/$documentId', params: { documentId: otherId }, search: validateDetailSearch({ view: 'published', tab: 'overview' }) }); });
  await act(async () => read.resolve(policy())); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('policy読取拒否後は旧主体の表示を止め、変更案は見直し後に戻す', async () => {
  const { api, client } = setup(); await fill(); fireEvent.click(screen.getByLabelText('編集'));
  api.getDocumentAccessPolicy.mockRejectedValue(problem('FORBIDDEN', 403)); await flushReads(client, ['document-access-policy', documentId]);
  expect(screen.queryByRole('group', { name: 'group / directory / reviewers' })).not.toBeInTheDocument();
  expect(screen.queryByText('確認者')).not.toBeInTheDocument(); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
  api.getDocumentAccessPolicy.mockResolvedValue(policy()); fireEvent.click(screen.getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'アクセス設定を保存' })).toBeEnabled()); expect(screen.getByLabelText('編集')).not.toBeChecked(); expect(screen.getByLabelText('変更理由')).toHaveValue('最初の理由');
});

test('保持結果を確認して閉じた後の新しい保存でdialogを勝手に再表示しない', async () => {
  setup(); wireClient.setConfig({ fetch: async input => { const body = await (input as Request).json(); return Response.json({ operationId: body.operationId, resourceId: documentId, resultingRevision: 3, changed: false, occurredAt: '2026-10-07T00:00:00Z' }); } });
  await fill(); save(); await screen.findByText('アクセス設定の変更はありませんでした。'); fireEvent.click(screen.getByRole('button', { name: '文書アクセス設定の保存結果' }));
  const dialog = await screen.findByRole('dialog', { name: '文書アクセス設定の保存結果' }); fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument()); await fill(); save();
  await waitFor(() => expect(screen.getAllByText('アクセス設定の変更はありませんでした。')).toHaveLength(1)); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});


test('Document送信前read中のFolder ACL UNKNOWNは両方の固定要求を壊さず新送信を止める', async () => {
  const { api, client } = setup(); await fill(); const read = deferred<ReturnType<typeof policy>>(); api.getDocumentAccessPolicy.mockReturnValue(read.promise); save(); await waitFor(() => expect(api.getDocumentAccessPolicy).toHaveBeenCalledTimes(2));
  const saved = { targetFolderId: 'folder', context: { kind: 'selected' as const, folderId: 'folder', sourceParentId: 'root', name: '資料', pageLimit: 1 }, request: { operationId: 'folder-policy', expectedPolicyRevision: 2, mode: 'inherit' as const, reason: '理由' }, expectedChanged: true, status: 'unknown' as const };
  act(() => folderAccessPolicyOperations(client).put(saved)); await act(async () => read.resolve(policy()));
  expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled(); expect(folderAccessPolicyOperations(client).get()).toBe(saved); expect(documentAccessPolicyOperations(client).get()).toBeUndefined();
});

test.each([-1, 1.5, Number.MAX_SAFE_INTEGER + 1])('policy local revision %pを送信権限の根拠にしない', async revision => {
  const { api } = setup(); api.getDocumentAccessPolicy.mockResolvedValue(policy(revision)); await screen.findByRole('heading', { name: 'アクセス設定' }); await screen.findByText(/最新の文書とアクセス設定を確認できません/);
  expect(screen.queryByRole('button', { name: 'アクセス設定を保存' })).not.toBeInTheDocument(); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('送信前に管理権限だけを失っても正規Document閲覧成功をerrorに変換しない', async () => {
  const { api, client } = setup(); await fill();
  api.getDocument.mockResolvedValue({ ...detail(), capabilities: { ...detail().capabilities, manageAccess: denied } }); save();
  await waitFor(() => expect(screen.queryByRole('tab', { name: 'アクセス' })).not.toBeInTheDocument());
  expect(client.getQueryState(['document', documentId, 'published'])?.status).toBe('success');
  expect(screen.getByRole('heading', { name: '合成文書', level: 1 })).toBeVisible(); expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('送信前の通常Document GET拒否も既存history/revision拒否barrierへ伝える', async () => {
  const { api, client } = setup();
  fireEvent.click(await screen.findByRole('tab', { name: '版・改訂' }));
  fireEvent.click(await screen.findByRole('button', { name: 'コンテンツ版の履歴を開く' }));
  await screen.findByRole('heading', { name: 'コンテンツ版の履歴（閲覧専用）' });
  fireEvent.click(screen.getByRole('tab', { name: 'アクセス' })); await fill(); const deniedRead = problem('FORBIDDEN', 403);
  api.getDocument.mockRejectedValue(deniedRead); save(); await waitFor(() => expect(client.getQueryState(['document', documentId, 'published'])?.status).toBe('error'));
  expect(client.getQueryData(['document-history-refusal', documentId])).toEqual({ error: deniedRead });
  expect(client.getQueryData(['document-content-history-refusal', documentId])).toEqual({ error: deniedRead });
  expect(client.getQueryData(['document-revisions', documentId, 'pages'])).toMatchObject({ pages: [], denial: deniedRead });
  expect(api.setDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('保存receipt日時は既存詳細と同じAsia/Tokyoの翌日・zone/offsetで表示する', async () => {
  setup(); wireClient.setConfig({ fetch: async input => { const body = await (input as Request).json(); return Response.json({ operationId: body.operationId, resourceId: documentId, resultingRevision: 3, changed: false, occurredAt: '2026-10-07T22:30:00Z' }); } });
  await fill(); save(); await screen.findByText('アクセス設定の変更はありませんでした。'); expect(screen.getByRole('region', { name: '文書アクセス設定の保存結果' })).toHaveTextContent('2026/10/08 7:30 (Asia/Tokyo, UTC+09:00)');
});
