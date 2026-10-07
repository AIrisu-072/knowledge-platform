import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentApi, type Folder, type FolderChildren } from '../src/application/document-workspace';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { folderAccessPolicyOperations } from '../src/application/document-folder-access-policy';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { folderMoveOperations } from '../src/application/document-folder-move';
import { documentMoveOperations } from '../src/application/document-move';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { validateListSearch } from '../src/application/search-state';

jest.mock('../src/application/document-workspace', () => ({ documentApi: {
  getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn(), createFolder: jest.fn(), renameFolder: jest.fn(), getDocument: jest.fn(), getDocumentVersion: jest.fn(), getFolderAccessPolicy: jest.fn(), setFolderAccessPolicy: jest.fn(),
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
  api.getDocument.mockResolvedValue({ metadata: {}, capabilities: { createVersion: denied, compareVersions: denied, manageAccess: denied } });
  api.getDocumentVersion.mockResolvedValue({ capabilities: { download: denied } });
  api.getFolderAccessPolicy.mockImplementation(id => Promise.resolve(policy(id)));
  api.setFolderAccessPolicy.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: body.expectedPolicyRevision + 1, changed: true, occurredAt: '2026-10-07T00:00:00Z' }));
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

const title = '選択したフォルダーのアクセス設定';
const actions = ['read', 'readHistory', 'write', 'publish', 'administer'];
const actionLabels: Record<string, string> = { read: '閲覧', readHistory: '履歴閲覧', write: '編集', publish: '公開', administer: 'アクセス管理' };
function policy(id = p.folderId) { return { target: { kind: 'folder', id }, bindingMode: 'explicit', policyId: 'policy', policyRevision: 7, effectivePolicyId: 'policy', effectiveSource: { kind: 'folder', id }, effectiveGrants: ['principal', 'group', 'role'].map((subjectKind, index) => ({ subjectKind, identityProvider: 'issuer', subjectId: `subject${index}`, actions: [...actions], presentation: { ref: { kind: subjectKind, provider: 'issuer', subjectId: `subject${index}` }, displayName: index === 0 ? '表示名' : null, secondaryText: null, resolution: index === 0 ? 'resolved' : 'unavailable' } })) }; }
async function choose(row = p) { fireEvent.click(await screen.findByRole('button', { name: row.name })); await waitFor(() => expect(screen.getByRole('button', { name: title })).toBeEnabled()); }
async function open() { fireEvent.click(screen.getByRole('button', { name: title })); const dialog = await screen.findByRole('dialog', { name: title }); await waitFor(() => expect(within(dialog).getByLabelText('設定方式')).toBeEnabled()); return dialog; }
function fill(dialog: HTMLElement) { fireEvent.change(within(dialog).getByLabelText('アクセス設定の変更理由'), { target: { value: '合成理由' } }); }
function confirm(dialog: HTMLElement) { fireEvent.click(within(dialog).getByLabelText('変更内容と影響範囲を確認しました')); }
function submit(dialog: HTMLElement) { fireEvent.submit(within(dialog).getByRole('button', { name: 'アクセス設定を保存' }).closest('form')!); }
function grant(dialog: HTMLElement, kind = 'group', index = 1) { return within(dialog).getByRole('group', { name: `${kind} / issuer / subject${index}` }); }
function change(dialog: HTMLElement) { fireEvent.click(within(grant(dialog)).getByLabelText('履歴閲覧')); fill(dialog); confirm(dialog); }
// Removing the actual Home entry, reading a URL target or rendering only document ACL must fail.
test('実Homeの選択行から非root設定を読み主体三つ組と5権限・表示名fallbackを示す', async () => {
  setup(); await choose(); const dialog = await open();
  expect(within(dialog).getByText('表示名')).toBeVisible();
  for (const [index, kind] of ['principal', 'group', 'role'].entries()) for (const action of actions) expect(within(grant(dialog, kind, index)).getByLabelText(actionLabels[action]!)).toBeChecked();
  expect(within(dialog).getByRole('heading', { name: '変更内容の確認' })).toBeVisible();
});
test('5権限は独立で三つ組だけを送り理由を正規化し影響確認を必須にする', async () => {
  const { api } = setup(); await choose(); const dialog = await open();
  fireEvent.click(within(grant(dialog)).getByLabelText('閲覧')); fill(dialog); submit(dialog); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
  expect(within(dialog).getByText(/自分の閲覧・管理権限を失う/)).toBeVisible(); expect(within(dialog).getByText(/独自の設定を持つ子孫/)).toBeVisible();
  confirm(dialog); submit(dialog); await within(dialog).findByText('アクセス設定を保存しました。');
  const [id, body] = api.setFolderAccessPolicy.mock.calls[0]!; expect(id).toBe(p.folderId); expect(body).toMatchObject({ expectedPolicyRevision: 7, mode: 'explicit', reason: '合成理由' });
  expect(body.grants[1]).toEqual({ subjectKind: 'group', identityProvider: 'issuer', subjectId: 'subject1', actions: ['readHistory', 'write', 'publish', 'administer'] });
  expect(body.grants.every((g: object) => !('presentation' in g))).toBe(true);
});
test('主体削除と空actionsは削除として確認し全grant削除は送信しない', async () => {
  const { api } = setup(); await choose(); const dialog = await open();
  fireEvent.click(within(grant(dialog, 'principal', 0)).getByRole('button', { name: 'この主体を削除' }));
  for (const action of actions) fireEvent.click(within(grant(dialog)).getByLabelText(actionLabels[action]!));
  fireEvent.click(within(grant(dialog, 'role', 2)).getByRole('button', { name: 'この主体を削除' }));
  fill(dialog); confirm(dialog); submit(dialog); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled(); expect(within(dialog).getByText(/少なくとも1主体/)).toBeVisible();
});
test.each(['inherit', 'explicit'] as const)('%sへ切り替えた正確なpayloadと確認内容', async mode => {
  const { api } = setup(); if (mode === 'explicit') api.getFolderAccessPolicy.mockImplementation(id => Promise.resolve({ ...policy(id), bindingMode: 'inherit', policyId: null, effectiveSource: { kind: 'folder', id: rootId } }));
  await choose(); const dialog = await open(); fireEvent.change(within(dialog).getByLabelText('設定方式'), { target: { value: mode } }); fill(dialog); confirm(dialog); submit(dialog);
  await within(dialog).findByText('アクセス設定を保存しました。'); const body = api.setFolderAccessPolicy.mock.calls[0]![1]; expect(body.mode).toBe(mode);
  expect(Object.hasOwn(body, 'grants')).toBe(mode === 'explicit');
});
test('同値もno-op receiptを受領して現在設定と区別する', async () => {
  const { api } = setup(); api.setFolderAccessPolicy.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: 7, changed: false, occurredAt: '2026-10-07T00:00:00Z' }));
  await choose(); const dialog = await open(); fill(dialog); confirm(dialog); submit(dialog); await within(dialog).findByText('アクセス設定の変更はありませんでした。'); expect(api.setFolderAccessPolicy).toHaveBeenCalledTimes(1);
});
test.each(['target', 'kind', 'revision', 'empty', 'duplicate', 'unknownAction'] as const)('不正GET %sは編集に採用しない', async field => {
  const { api } = setup(); const read = policy();
  if (field === 'target') read.target.id = q.folderId; if (field === 'kind') read.target.kind = 'document'; if (field === 'revision') read.policyRevision = -1;
  if (field === 'empty') read.effectiveGrants = []; if (field === 'duplicate') read.effectiveGrants.push(read.effectiveGrants[0]!); if (field === 'unknownAction') read.effectiveGrants[0]!.actions.push('unknown');
  api.getFolderAccessPolicy.mockResolvedValue(read); await choose(); fireEvent.click(screen.getByRole('button', { name: title })); const dialog = await screen.findByRole('dialog'); await within(dialog).findByRole('alert');
  expect(within(dialog).getByRole('button', { name: 'アクセス設定を保存' })).toBeDisabled(); expect(within(dialog).queryByRole('group', { name: /principal \/ issuer/ })).not.toBeInTheDocument();
});
test.each(['policyRevision', 'grants', 'source', 'mode', 'policyId', 'folderRevision', 'folderParent'] as const)('保存前fresh %s差異を検出し旧確認を再利用しない', async field => {
  const { api } = setup(); await choose(); const dialog = await open(); change(dialog); const changed = policy();
  if (field === 'policyRevision') changed.policyRevision++; if (field === 'grants') changed.effectiveGrants[1]!.actions = ['read']; if (field === 'source') changed.effectiveSource.id = rootId;
  if (field === 'mode') changed.bindingMode = 'inherit'; if (field === 'policyId') changed.effectivePolicyId = 'other';
  api.getFolderAccessPolicy.mockResolvedValue(changed);
  if (field === 'folderRevision' || field === 'folderParent') api.listFolderChildren.mockImplementation(id => Promise.resolve(page(id === rootId ? [{ ...p, revision: 9, parentFolderId: field === 'folderParent' ? q.folderId : rootId }, q] : [])));
  submit(dialog); await within(dialog).findByRole('alert'); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled(); expect(within(dialog).getByLabelText('変更内容と影響範囲を確認しました')).not.toBeChecked();
});
test.each(['permission', 'lifecycle', 'notHumanInteractive', 'unsupported'] as const)('fresh manageAccess %sを親やGET成功で補わない', async reason => {
  const { api } = setup(); await choose(); const dialog = await open(); change(dialog);
  api.listFolderChildren.mockImplementation(id => Promise.resolve(id === rootId ? page([p, q]) : { ...page([]), capabilities: { ...page([]).capabilities, manageAccess: { status: 'disabled', reason } } }));
  submit(dialog); await within(dialog).findByRole('alert'); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
});
test('rootと直URLは編集不可で実ツリー選択のみ入口を許可する', async () => {
  const { api } = setup(`/documents?view=published&folderId=${p.folderId}`); await screen.findByRole('button', { name: p.name }); expect(screen.queryByRole('button', { name: title })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'System Root' })); expect(screen.queryByRole('button', { name: title })).not.toBeInTheDocument(); expect(api.getFolderAccessPolicy).not.toHaveBeenCalled();
});
test.each(['cancel', 'same-selection', 'other-selection', 'back', 'navigation'] as const)('保存前read中%sは遅延応答から送信しない', async action => {
  const { api, router, history } = setup(); await choose(); const dialog = await open(); change(dialog); const read = deferred<ReturnType<typeof policy>>(); api.getFolderAccessPolicy.mockReturnValue(read.promise);
  act(() => { submit(dialog); submit(dialog); });
  if (action === 'cancel') fireEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  else if (action === 'same-selection' || action === 'other-selection') fireEvent.click(screen.getByRole('button', { name: action === 'same-selection' ? p.name : q.name, hidden: true }));
  else if (action === 'back') await act(async () => history.back()); else await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); });
  await act(async () => read.resolve(policy())); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
});
test('UNKNOWNは別選択・往復・GC後も同じ要求を再送し403/409後も保持する', async () => {
  const { api, router, history, client } = setup(); await choose(); const dialog = await open(); change(dialog);
  api.setFolderAccessPolicy.mockRejectedValueOnce(new Error('lost')).mockRejectedValueOnce(problem('FORBIDDEN', 403)).mockRejectedValueOnce(problem('REVISION_CONFLICT', 409));
  submit(dialog); await within(dialog).findByRole('button', { name: '同じ内容で再試行' }); const [id, body] = api.setFolderAccessPolicy.mock.calls[0]!;
  fireEvent.click(within(dialog).getByRole('button', { name: '閉じる' })); fireEvent.click(screen.getByRole('button', { name: q.name }));
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context' } }); client.removeQueries({ queryKey: ['folder-access-policy'] }); }); await act(async () => history.back());
  fireEvent.click(await screen.findByRole('button', { name: title })); const reopened = await screen.findByRole('dialog');
  for (let index = 0; index < 3; index++) { fireEvent.click(within(reopened).getByRole('button', { name: '同じ内容で再試行' })); await within(reopened).findByRole('button', { name: index === 2 ? '確認して閉じる' : '同じ内容で再試行' }); }
  expect(api.setFolderAccessPolicy.mock.calls.every(([target, request]) => target === id && request === body)).toBe(true);
  expect(within(reopened).getByText(/操作時点の記録/)).toBeVisible(); expect(router.state.location.search.folderId).toBe(q.folderId);
});
test('成功後読取拒否でもreceipt成功を保持し古いpolicy/文書/Organization contextはresetする', async () => {
  const { api, client } = setup(); await choose(); const dialog = await open(); change(dialog);
  const keys = [['document', 'secret'], ['document-history', 'secret'], ['revision-comparison', 'secret'], ['organization', 'actor', 'task', 'document-context', 'secret'], ['folder-access-policy', 'other']]; keys.forEach(key => client.setQueryData(key, { secret: true }));
  const organization = { selection: 'stable' }; client.setQueryData(['organization-session'], organization);
  api.setFolderAccessPolicy.mockImplementation((id, body) => { api.listFolderChildren.mockRejectedValue(problem('FORBIDDEN', 403)); api.listDocuments.mockRejectedValue(problem('FORBIDDEN', 403)); return Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: 8, changed: true, occurredAt: '2026-10-07T00:00:00Z' }); });
  submit(dialog); await within(dialog).findByText('アクセス設定を保存しました。'); await waitFor(() => keys.forEach(key => expect(client.getQueryData(key)).toBeUndefined()));
  expect(client.getQueryData(['organization-session'])).toBe(organization); expect(within(dialog).queryByRole('button', { name: '同じ内容で再試行' })).not.toBeInTheDocument();
});
test.each(['invalidate', 'reset', 'remove'] as const)('編集中policy cache %sは旧主体を隠し再確認を要求する', async kind => {
  const { client, api } = setup(); await choose(); const dialog = await open(); change(dialog);
  await act(async () => { if (kind === 'invalidate') await client.invalidateQueries({ queryKey: ['folder-access-policy'] }); else if (kind === 'reset') await client.resetQueries({ queryKey: ['folder-access-policy'] }); else client.removeQueries({ queryKey: ['folder-access-policy'] }); });
  expect(within(dialog).queryByRole('group', { name: /principal \/ issuer/ })).not.toBeInTheDocument(); submit(dialog); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
});

function policySaved(status: 'pending' | 'unknown') { return { targetFolderId: p.folderId, context: { kind: 'selected' as const, folderId: p.folderId, sourceParentId: rootId, pageLimit: 1, name: p.name }, request: { operationId: 'fixed-policy', expectedPolicyRevision: 7, mode: 'inherit' as const, reason: '理由' }, expectedChanged: true, status }; }
test.each(['pending', 'unknown'] as const)('policy %sは作成/改名/移動を止め保持結果入口を残す', async status => {
  const { client } = setup(); await choose(); act(() => folderAccessPolicyOperations(client).put(policySaved(status)));
  for (const label of ['System Rootにフォルダーを作成', '選択したフォルダーに子フォルダーを作成', '選択したフォルダー名を変更', '選択したフォルダーを移動']) expect(screen.getByRole('button', { name: label })).toBeDisabled();
  expect(screen.getByRole('button', { name: title })).toBeEnabled();
});
test.each(['create', 'rename', 'move', 'documentMove'] as const)('他%s未確定がfresh GET中に生じたら新policyを送信しない', async kind => {
  const { client, api } = setup(); await choose(); const dialog = await open(); change(dialog); const read = deferred<ReturnType<typeof policy>>(); api.getFolderAccessPolicy.mockReturnValue(read.promise); submit(dialog);
  act(() => {
    const context = policySaved('unknown').context;
    if (kind === 'create') rootFolderOperations(client).put({ status: 'unknown', request: { operationId: 'o', folderId: 'new', parentFolderId: rootId, expectedParentRevision: 17, name: '新', reason: '理由' } });
    else if (kind === 'rename') folderRenameOperations(client).put({ status: 'unknown', targetFolderId: p.folderId, context, currentName: p.name, expectedChanged: true, request: { operationId: 'o', expectedFolderRevision: 8, name: '新', reason: '理由' } });
    else if (kind === 'move') folderMoveOperations(client).put({ status: 'unknown', targetFolderId: p.folderId, context, currentName: p.name, expectedChanged: true, destination: { kind: 'root', folderId: rootId, name: 'System Root' }, request: { operationId: 'o', expectedFolderRevision: 8, fromParentId: rootId, toParentId: 'other', reason: '理由' } });
    else documentMoveOperations(client).put({ status: 'unknown' } as never);
  });
  await act(async () => read.resolve(policy())); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
});
test('close後同Folderを再表示したfresh readは古い未完了GETに合流しない', async () => {
  const { api } = setup(); await choose(); const old = deferred<ReturnType<typeof policy>>(); api.getFolderAccessPolicy.mockReturnValueOnce(old.promise);
  fireEvent.click(screen.getByRole('button', { name: title })); const first = await screen.findByRole('dialog'); await waitFor(() => expect(api.getFolderAccessPolicy).toHaveBeenCalledTimes(1));
  fireEvent.click(within(first).getByRole('button', { name: 'キャンセル' }));
  const fresh = policy(); fresh.effectiveGrants = [fresh.effectiveGrants[2]!]; api.getFolderAccessPolicy.mockResolvedValue(fresh);
  fireEvent.click(screen.getByRole('button', { name: title })); const reopened = await screen.findByRole('dialog');
  await waitFor(() => expect(within(reopened).getByLabelText('設定方式')).toBeEnabled());
  expect(api.getFolderAccessPolicy).toHaveBeenCalledTimes(2); await act(async () => old.resolve(policy()));
  expect(within(reopened).queryByRole('group', { name: /principal \/ issuer/ })).not.toBeInTheDocument(); expect(grant(reopened, 'role', 2)).toBeVisible();
});
test('保存で削除した主体は次回GETから復活させず新規主体入力を持たない', async () => {
  const { api } = setup(); await choose(); const dialog = await open(); fireEvent.click(within(grant(dialog)).getByRole('button', { name: 'この主体を削除' })); fill(dialog); confirm(dialog); submit(dialog);
  await within(dialog).findByText('アクセス設定を保存しました。'); const body = api.setFolderAccessPolicy.mock.calls[0]![1]; expect(body.grants).toHaveLength(2);
  const fresh = policy(); fresh.policyRevision++; fresh.effectiveGrants = fresh.effectiveGrants.filter(row => row.subjectKind !== 'group'); api.getFolderAccessPolicy.mockResolvedValue(fresh);
  fireEvent.click(within(dialog).getByRole('button', { name: '確認して閉じる' })); await choose(); const next = await open();
  expect(within(next).queryByRole('group', { name: /group \/ issuer/ })).not.toBeInTheDocument(); expect(within(next).queryByRole('textbox', { name: /主体|subject|provider/ })).not.toBeInTheDocument();
});
test('fresh差異の見直しで削除された主体を除き理由を保持し確認をやり直す', async () => {
  const { api } = setup(); await choose(); const dialog = await open(); change(dialog);
  const fresh = policy(); fresh.effectiveGrants = fresh.effectiveGrants.filter(row => row.subjectKind !== 'group'); api.getFolderAccessPolicy.mockResolvedValue(fresh);
  submit(dialog); await within(dialog).findByRole('button', { name: '最新の状態を取得して見直す' }); fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(dialog).getByLabelText('設定方式')).toBeEnabled());
  expect(within(dialog).getByLabelText('アクセス設定の変更理由')).toHaveValue('合成理由'); expect(within(dialog).getByLabelText('変更内容と影響範囲を確認しました')).not.toBeChecked(); expect(within(dialog).queryByRole('group', { name: /group \/ issuer/ })).not.toBeInTheDocument();
});
test('同名・同IDでもkind/providerを別主体として扱いpresentation refを送信しない', async () => {
  const { api } = setup(); const read = policy(); read.effectiveGrants = read.effectiveGrants.map((row, index) => ({ ...row, subjectId: 'same', identityProvider: index === 2 ? 'other' : 'issuer', presentation: { ...row.presentation, displayName: '同じ表示名', resolution: 'resolved', ref: { kind: 'role', provider: 'wrong', subjectId: 'wrong' } } })); api.getFolderAccessPolicy.mockResolvedValue(read);
  await choose(); const dialog = await open(); const group = within(dialog).getByRole('group', { name: 'group / issuer / same' }); fireEvent.click(within(group).getByLabelText('公開')); fill(dialog); confirm(dialog); submit(dialog);
  await within(dialog).findByText('アクセス設定を保存しました。'); const body = api.setFolderAccessPolicy.mock.calls[0]![1]; expect(body.grants.map((row: { subjectKind: string; identityProvider: string; subjectId: string }) => [row.subjectKind, row.identityProvider, row.subjectId])).toEqual([['principal', 'issuer', 'same'], ['group', 'issuer', 'same'], ['role', 'other', 'same']]);
});
test.each(['reason', 'mode', 'grant', 'delete'] as const)('確認後%s変更と同batch submitでは旧確認を流用しない', async field => {
  const { api } = setup(); await choose(); const dialog = await open(); change(dialog);
  act(() => {
    if (field === 'reason') fireEvent.change(within(dialog).getByLabelText('アクセス設定の変更理由'), { target: { value: '別理由' } });
    if (field === 'mode') fireEvent.change(within(dialog).getByLabelText('設定方式'), { target: { value: 'inherit' } });
    if (field === 'grant') fireEvent.click(within(grant(dialog)).getByLabelText('閲覧'));
    if (field === 'delete') fireEvent.click(within(grant(dialog)).getByRole('button', { name: 'この主体を削除' }));
    submit(dialog);
  }); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled();
});
test('初回409拒否は新fresh review後に新IDで保存できUNKNOWNと混ぜない', async () => {
  const { api } = setup(); await choose(); const dialog = await open(); change(dialog); api.setFolderAccessPolicy.mockRejectedValueOnce(problem('REVISION_CONFLICT', 409)); submit(dialog);
  await within(dialog).findByText('アクセス設定の保存は拒否されました。最新の状態を取得して見直してください。'); const first = api.setFolderAccessPolicy.mock.calls[0]![1];
  const fresh = policy(); fresh.policyRevision = 9; api.getFolderAccessPolicy.mockResolvedValue(fresh); fireEvent.click(within(dialog).getByRole('button', { name: '最新の状態を取得して見直す' }));
  await waitFor(() => expect(within(dialog).getByLabelText('設定方式')).toBeEnabled()); confirm(dialog); submit(dialog); await within(dialog).findByText('アクセス設定を保存しました。');
  expect(api.setFolderAccessPolicy.mock.calls[1]![1].expectedPolicyRevision).toBe(9); expect(api.setFolderAccessPolicy.mock.calls[1]![1].operationId).not.toBe(first.operationId);
});
test('保存前fresh GET完了直後の同期cache失効は送信根拠にしない', async () => {
  const { client, api } = setup(); await choose(); const dialog = await open(); change(dialog);
  const unsubscribe = client.getQueryCache().subscribe(event => {
    if (event.type === 'updated' && event.query.queryKey[0] === 'folder-access-policy' && event.action.type === 'success') {
      void client.invalidateQueries({ queryKey: ['folder-access-policy'], refetchType: 'none' });
    }
  });
  submit(dialog); await waitFor(() => expect(within(dialog).getByRole('button', { name: 'アクセス設定を保存' }).closest('form')).toHaveAttribute('aria-busy', 'false')); expect(api.setFolderAccessPolicy).not.toHaveBeenCalled(); expect(within(dialog).getByRole('alert')).toBeVisible(); unsubscribe();
});
test('継承へ戻す確認欄は未取得の祖先grantを旧設定から予測せず、設定解除と上位適用だけを示す', async () => {
  setup(); await choose(); const dialog = await open(); fireEvent.change(within(dialog).getByLabelText('設定方式'), { target: { value: 'inherit' } });
  const confirmation = within(dialog).getByRole('region', { name: '変更内容の確認' });
  expect(within(confirmation).getByText(/個別設定を解除し、その時点の上位の設定を適用/)).toBeVisible();
  expect(within(confirmation).queryAllByText(/→/)).toHaveLength(0);
  expect(within(grant(dialog)).getByLabelText('閲覧')).toBeDisabled();
});
test('編集から継承へ切替中は変更前の読取だけを示し個別設定へ戻すとdraftを保持する', async () => {
  setup(); await choose(); const dialog = await open(); fireEvent.click(within(grant(dialog)).getByLabelText('履歴閲覧'));
  expect(within(grant(dialog)).getByLabelText('履歴閲覧')).not.toBeChecked();
  fireEvent.change(within(dialog).getByLabelText('設定方式'), { target: { value: 'inherit' } });
  expect(within(grant(dialog)).getByLabelText('履歴閲覧')).toBeChecked(); expect(within(grant(dialog)).getByLabelText('履歴閲覧')).toBeDisabled();
  expect(within(dialog).getByText('変更前に確認した実効権限')).toBeVisible();
  fireEvent.change(within(dialog).getByLabelText('設定方式'), { target: { value: 'explicit' } });
  expect(within(grant(dialog)).getByLabelText('履歴閲覧')).not.toBeChecked(); expect(within(grant(dialog)).getByLabelText('履歴閲覧')).toBeEnabled();
  expect(within(dialog).getByText('保存する権限（編集内容）')).toBeVisible();
});
