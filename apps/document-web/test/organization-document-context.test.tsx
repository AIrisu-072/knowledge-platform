import { TextEncoder } from 'node:util';
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { documentApi } from '../src/application/document-workspace';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { TaskHomePage } from '../src/routes/TaskHomePage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn() } }));
const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', capabilities: { nativeWorkspace: false, agent: true, search: false, fileUpload: false, return: false } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null, workTypeId: 'work-type-1', workTypeLabel: '内容確認', dueAt: null, attention: [], contextTitle: null };
const documentId = '00000000-0000-4000-8000-000000000010';
const otherDocumentId = '00000000-0000-4000-8000-000000000011';
const detail = { ...task, inputResources: [{ kind: 'document', documentId, label: '共有入力文書' }, { kind: 'document', documentId: otherDocumentId, label: '別の共有文書' }], workingArtifacts: [{ id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' }], history: [], agentExecutionIds: [] };
const published = { documentId, title: '公開済み資料', documentVersionId: 'version-7', currentVersionId: 'version-7', revision: 99, displayRevision: { revisionId: 'revision-12', documentVersionId: 'version-7', label: '3.2' } };
const original = { contentItemId: 'content-1', representationId: 'representation-1', logicalPath: 'primary', ordinal: 1, role: 'AUTHORITATIVE', displayName: '参照資料.txt', mediaType: 'text/plain', sizeBytes: 42 };
const supplementary = { ...original, contentItemId: 'content-2', representationId: 'representation-2', displayName: '補足.html', mediaType: 'text/html', sizeBytes: 80 };
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function setup(view = 'context') {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(session as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: [task], nextCursor: null } as never);
  jest.spyOn(workApi, 'listWorkContexts').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'getTask').mockResolvedValue(detail as never);
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  const mutations = [jest.spyOn(workApi, 'saveDraft'), jest.spyOn(workApi, 'submit'), jest.spyOn(workApi, 'holdTask'), jest.spyOn(workApi, 'resumeTask'), jest.spyOn(workApi, 'completeTask'), jest.spyOn(workApi, 'recordDecision')];
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const router = createRouter({ routeTree: root.addChildren([route]), history: createMemoryHistory({ initialEntries: [`/tasks?view=${view}&taskId=task-1`] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  const rendered = render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { ...rendered, router, client, mutations };
}
beforeEach(() => {
  jest.mocked(documentApi.getDocument).mockReset().mockResolvedValue(published as never);
  jest.mocked(documentApi.listVersionFiles).mockReset().mockResolvedValue({ items: [original, supplementary] });
  jest.mocked(documentApi.downloadVersionFile).mockReset().mockResolvedValue(new Blob(['synthetic']));
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: jest.fn(() => 'blob:synthetic-original') });
  Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() });
});
afterEach(() => jest.restoreAllMocks());

test.each(['context', 'queue'])('%s shows formal published revision, distinct content Version and original metadata without saving Work', async (view) => {
  const { mutations } = setup(view);
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '未保存の文案を保持' } });
  const panel = await screen.findByRole('region', { name: '公開文書の内容' });
  expect(panel).toHaveTextContent('公開済み資料');
  expect(panel).toHaveTextContent('公開改訂 3.2');
  expect(panel).toHaveTextContent('revision-12');
  expect(panel).toHaveTextContent('内容の版（Version） version-7');
  expect(panel).not.toHaveTextContent('99');
  expect(panel).toHaveTextContent('参照資料.txt');
  expect(panel).toHaveTextContent('text/plain');
  expect(panel).toHaveTextContent('42 bytes');
  expect(panel).toHaveTextContent('補足.html');
  expect(within(panel).getAllByRole('button', { name: /原本を取得/ })).toHaveLength(2);
  expect(documentApi.getDocument).toHaveBeenCalledWith(documentId, 'published');
  expect(documentApi.listVersionFiles).toHaveBeenCalledWith(documentId, 'version-7', 'published');
  expect(documentApi.downloadVersionFile).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: '履歴' }));
  await userEvent.click(screen.getByRole('button', { name: '文書・比較' }));
  await screen.findByRole('region', { name: '公開文書の内容' });
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('未保存の文案を保持');
  expect(screen.getByRole('link', { name: '共有入力文書' })).toHaveAttribute('href', expect.stringContaining(`/documents/${documentId}`));
  for (const mutation of mutations) expect(mutation).not.toHaveBeenCalled();
});

test('no published revision and empty originals are distinct and never fall back to authoring or history', async () => {
  jest.mocked(documentApi.getDocument).mockResolvedValue({ ...published, title: '未公開の秘密', displayRevision: null } as never);
  setup();
  await screen.findByText('公開改訂はありません');
  expect(screen.queryByText('未公開の秘密')).not.toBeInTheDocument();
  expect(documentApi.listVersionFiles).not.toHaveBeenCalled();
  jest.mocked(documentApi.getDocument).mockResolvedValue(published as never);
  jest.mocked(documentApi.listVersionFiles).mockResolvedValue({ items: [] });
  await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  expect(await screen.findByText('原本ファイルはありません')).toBeVisible();
  expect(screen.getByText('公開済み資料')).toBeVisible();
  expect(documentApi.getDocument).toHaveBeenLastCalledWith(documentId, 'published');
});

test.each([401, 403, 404])('refresh %i hides previously loaded metadata and clears the source cache', async (status) => {
  const { client } = setup();
  await screen.findByText('公開済み資料');
  jest.mocked(documentApi.getDocument).mockRejectedValue({ status, detail: '非開示エラー本文' });
  await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  await screen.findByText(/公開文書を利用できません/);
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(screen.queryByText('参照資料.txt')).not.toBeInTheDocument();
  expect(screen.queryByText('非開示エラー本文')).not.toBeInTheDocument();
  expect(JSON.stringify(client.getQueryCache().getAll().filter((query) => query.queryKey.includes('document-context')).map((query) => query.state.data))).not.toContain('公開済み資料');
  expect(documentApi.listVersionFiles).toHaveBeenCalledTimes(1);
});

test('file-list denial hides the title and revision; successful retry restores only published data', async () => {
  jest.mocked(documentApi.listVersionFiles).mockRejectedValueOnce({ status: 403 });
  setup();
  await screen.findByText(/公開文書を利用できません/);
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(screen.queryByText(/revision-12/)).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  expect(await screen.findByText('公開済み資料')).toBeVisible();
});

test('mismatched Document or published Version is unavailable rather than displaying unbound bytes', async () => {
  jest.mocked(documentApi.getDocument).mockResolvedValue({ ...published, documentVersionId: 'wrong-version' } as never);
  setup();
  await screen.findByText(/公開文書を利用できません/);
  expect(documentApi.listVersionFiles).not.toHaveBeenCalled();
});

test('selection fences a late read and does not fetch original metadata for a deselected Document', async () => {
  const late = deferred<typeof published>();
  jest.mocked(documentApi.getDocument).mockImplementation((id) => id === documentId ? late.promise as never : Promise.resolve({ ...published, documentId: otherDocumentId, title: '別文書の公開資料' }) as never);
  setup();
  await userEvent.selectOptions(await screen.findByLabelText('確認する入力文書'), otherDocumentId);
  await screen.findByText('別文書の公開資料');
  await act(async () => late.resolve(published));
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(documentApi.listVersionFiles).not.toHaveBeenCalledWith(documentId, expect.anything(), expect.anything());
  expect(screen.getByText('別文書の公開資料')).toBeVisible();
});

test('original download is explicit, published-only and deduplicated while preserving the draft', async () => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  const clicks: string[] = [];
  jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) { clicks.push(this.download); });
  const { mutations } = setup();
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '取得中も未保存' } });
  const button = await screen.findByRole('button', { name: '原本を取得 参照資料.txt' });
  fireEvent.click(button); fireEvent.click(button);
  expect(button).toBeDisabled();
  expect(documentApi.downloadVersionFile).toHaveBeenCalledTimes(1);
  expect(documentApi.downloadVersionFile).toHaveBeenCalledWith({ documentId, versionId: 'version-7', contentItemId: original.contentItemId, representationId: original.representationId, purpose: 'published' });
  await act(async () => pending.resolve(new Blob(['synthetic'])));
  expect(clicks).toEqual(['参照資料.txt']);
  expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('取得中も未保存');
  for (const mutation of mutations) expect(mutation).not.toHaveBeenCalled();
});

test('download failure is retryable, but a later provider denial removes all loaded Document content', async () => {
  jest.mocked(documentApi.downloadVersionFile).mockRejectedValueOnce(new Error('network')).mockRejectedValueOnce({ status: 404 });
  setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  expect(await screen.findByText(/原本を取得できません。再度取得してください/)).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: '原本を取得 参照資料.txt' }));
  await screen.findByText(/公開文書を利用できません/);
  expect(screen.queryByText('参照資料.txt')).not.toBeInTheDocument();
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test.each(['module', 'document', 'refresh', 'unmount'])('%s change fences pending original download before creating a blob URL', async (change) => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  jest.mocked(documentApi.getDocument).mockImplementation((id) => Promise.resolve({ ...published, documentId: id }) as never);
  const { unmount } = setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  if (change === 'module') await userEvent.click(screen.getByRole('button', { name: '履歴' }));
  if (change === 'document') await userEvent.selectOptions(screen.getByLabelText('確認する入力文書'), otherDocumentId);
  if (change === 'refresh') await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  if (change === 'unmount') unmount();
  await act(async () => pending.resolve(new Blob(['obsolete'])));
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test.each(['principalId', 'actingAssignmentId', 'attemptId', 'id'])('%s changes isolate cached metadata and late original responses', async (field) => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  const { client, router } = setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  jest.mocked(documentApi.getDocument).mockRejectedValue({ status: 403 });
  if (field === 'principalId' || field === 'actingAssignmentId') {
    await act(async () => { client.setQueryData(['organization-session'], { ...session, [field]: 'different-actor' }); });
  } else {
    const next = { ...task, [field]: 'next-task-scope' };
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [next], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...next } as never);
    if (field === 'id') await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: next.id } } as never); });
    await act(async () => { await client.invalidateQueries({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'tasks'] }); });
  }
  await screen.findByText(/公開文書を利用できません/);
  await act(async () => pending.resolve(new Blob(['old-actor'])));
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
  expect(documentApi.getDocument).toHaveBeenCalledTimes(2);
});

test('Work denial hides Document context and fences in-flight bytes without relaxing private disclosure', async () => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  const { client } = setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  jest.mocked(workApi.getTask).mockRejectedValue(new WorkApiError(403, 'FORBIDDEN'));
  await act(async () => { await client.invalidateQueries({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'task'] }); });
  await screen.findByText(/内容を非表示にしました/);
  await act(async () => pending.resolve(new Blob(['old-task'])));
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test.each(['held', 'completed'])('%s task retains legitimate published Document reads', async (state) => {
  setup();
  const readonly = { ...task, state, canEdit: false, canSubmit: false };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [readonly], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...readonly } as never);
  expect(await screen.findByText('公開済み資料')).toBeVisible();
  expect(screen.getByRole('button', { name: '原本を取得 参照資料.txt' })).toBeEnabled();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
});

test('HTML original is download-only and an unsafe filename cannot create markup or a path', async () => {
  jest.mocked(documentApi.listVersionFiles).mockResolvedValue({ items: [{ ...original, displayName: '../folder/\u202E<script>.html\n', mediaType: 'text/html' }] });
  const clicks: string[] = [];
  jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) { clicks.push(this.download); });
  setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 <script>.html' }));
  expect(clicks).toEqual(['<script>.html']);
  expect(document.querySelector('iframe, object, embed, script')).toBeNull();
  expect(document.querySelector('a[href="blob:synthetic-original"]')).toBeNull();
});

test('reload permits a fresh download and an older response cannot clear its busy state or emit bytes', async () => {
  const old = deferred<Blob>(), current = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  const button = await screen.findByRole('button', { name: '原本を取得 参照資料.txt' });
  expect(button).toBeEnabled();
  await userEvent.click(button);
  await act(async () => old.resolve(new Blob(['obsolete'])));
  expect(button).toBeDisabled();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
  await act(async () => current.resolve(new Blob(['current'])));
  expect(button).toBeEnabled();
  expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
});

test('a removed shared input fences its pending original and offers only current task inputs', async () => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  const { client } = setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  await act(async () => { client.setQueryData(['organization', session.principalId, session.actingAssignmentId, 'task', task.id, task.attemptId], { ...detail, inputResources: [detail.inputResources[1]] }); });
  await waitFor(() => expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument());
  await act(async () => pending.resolve(new Blob(['removed'])));
  expect(screen.queryByText('公開済み資料')).not.toBeInTheDocument();
  expect(screen.queryByRole('link', { name: '共有入力文書' })).not.toBeInTheDocument();
  expect(screen.getByLabelText('確認する入力文書')).toHaveValue('');
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test('late original metadata and subsequent missing publication cannot restore previous Document content', async () => {
  const pending = deferred<{ items: typeof original[] }>();
  jest.mocked(documentApi.listVersionFiles).mockReturnValueOnce(pending.promise);
  jest.mocked(documentApi.getDocument).mockImplementation((id) => Promise.resolve({ ...published, documentId: id, title: id === documentId ? '古い文書' : '新しい文書' }) as never);
  setup();
  await waitFor(() => expect(documentApi.listVersionFiles).toHaveBeenCalledTimes(1));
  await userEvent.selectOptions(screen.getByLabelText('確認する入力文書'), otherDocumentId);
  await screen.findByText('新しい文書');
  await act(async () => pending.resolve({ items: [{ ...original, displayName: '古い原本.txt' }] }));
  expect(screen.queryByText('古い文書')).not.toBeInTheDocument();
  expect(screen.queryByText('古い原本.txt')).not.toBeInTheDocument();
  jest.mocked(documentApi.getDocument).mockResolvedValue({ ...published, displayRevision: null } as never);
  await userEvent.click(screen.getByRole('button', { name: '公開文書を再読込' }));
  await screen.findByText('公開改訂はありません');
  expect(screen.queryByText('新しい文書')).not.toBeInTheDocument();
  expect(screen.queryByText('参照資料.txt')).not.toBeInTheDocument();
});

test('no shared inputs means no Document request or upload/private attachment controls', async () => {
  setup();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, inputResources: [] } as never);
  await screen.findByText('入力文書はありません');
  expect(documentApi.getDocument).not.toHaveBeenCalled();
  expect(document.querySelector('input[type="file"]')).toBeNull();
});


test('derived and unknown representations are not labeled or downloaded as originals', async () => {
  jest.mocked(documentApi.listVersionFiles).mockResolvedValue({ items: [original, { ...supplementary, role: 'DERIVED', displayName: '派生.html' }, { ...supplementary, representationId: 'unknown', role: 'FUTURE', displayName: '未知表現.txt' }] });
  setup();
  const panel = await screen.findByRole('region', { name: '公開文書の内容' });
  expect(within(panel).getAllByRole('button', { name: /原本を取得/ })).toHaveLength(1);
  expect(panel).not.toHaveTextContent('派生.html');
  expect(panel).not.toHaveTextContent('未知表現.txt');
});

test.each(['履歴', '根拠'])('returning from %s retains the selected shared Document and draft while re-reading provider metadata', async (module) => {
  const oldDownload = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(oldDownload.promise);
  jest.mocked(documentApi.getDocument).mockImplementation((id) => Promise.resolve({ ...published, documentId: id, title: id === documentId ? '第一の文書' : '第二の文書' }) as never);
  const { mutations } = setup();
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '選択と一緒に保持する未保存文案' } });
  await userEvent.selectOptions(screen.getByLabelText('確認する入力文書'), otherDocumentId);
  await screen.findByText('第二の文書');
  await userEvent.click(screen.getByRole('button', { name: '原本を取得 参照資料.txt' }));
  await userEvent.click(screen.getByRole('button', { name: module }));
  jest.mocked(documentApi.getDocument).mockRejectedValue({ status: 403 });
  await userEvent.click(screen.getByRole('button', { name: '文書・比較' }));
  expect(await screen.findByLabelText('確認する入力文書')).toHaveValue(otherDocumentId);
  await screen.findByText(/公開文書を利用できません/);
  expect(documentApi.getDocument).toHaveBeenLastCalledWith(otherDocumentId, 'published');
  expect(screen.queryByText('第二の文書')).not.toBeInTheDocument();
  await act(async () => oldDownload.resolve(new Blob(['module離脱前のbytes'])));
  expect(URL.createObjectURL).not.toHaveBeenCalled();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('選択と一緒に保持する未保存文案');
  for (const mutation of mutations) expect(mutation).not.toHaveBeenCalled();
});

test.each(['principalId', 'actingAssignmentId', 'attemptId', 'id'])('the retained Document selection does not leak across a different %s', async (field) => {
  jest.mocked(documentApi.getDocument).mockImplementation((id) => Promise.resolve({ ...published, documentId: id }) as never);
  const { client, router } = setup();
  await userEvent.selectOptions(await screen.findByLabelText('確認する入力文書'), otherDocumentId);
  await screen.findByText('公開済み資料');
  if (field === 'principalId' || field === 'actingAssignmentId') {
    await act(async () => { client.setQueryData(['organization-session'], { ...session, [field]: 'different-selection-scope' }); });
  } else {
    const next = { ...task, [field]: 'different-selection-scope' };
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [next], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...next } as never);
    if (field === 'id') await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: next.id } } as never); });
    await act(async () => { await client.invalidateQueries({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'tasks'] }); });
  }
  await waitFor(() => expect(screen.getByLabelText('確認する入力文書')).toHaveValue(documentId));
  await userEvent.click(screen.getByRole('button', { name: '履歴' }));
  await userEvent.click(screen.getByRole('button', { name: '文書・比較' }));
  expect(await screen.findByLabelText('確認する入力文書')).toHaveValue(documentId);
  await waitFor(() => expect(documentApi.getDocument).toHaveBeenLastCalledWith(documentId, 'published'));
});

test.each([401, 403, 404])('original denial %i restores focus from the disappearing control to reload', async (status) => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  setup();
  const button = await screen.findByRole('button', { name: '原本を取得 参照資料.txt' });
  await userEvent.click(button);
  expect(button).toHaveFocus();
  await act(async () => pending.reject({ status }));
  await screen.findByText(/公開文書を利用できません/);
  expect(screen.getByRole('button', { name: '公開文書を再読込' })).toHaveFocus();
});

test('original denial does not steal focus moved to the unsaved Work draft while waiting', async () => {
  const pending = deferred<Blob>();
  jest.mocked(documentApi.downloadVersionFile).mockReturnValue(pending.promise);
  setup();
  await userEvent.click(await screen.findByRole('button', { name: '原本を取得 参照資料.txt' }));
  const editor = screen.getByLabelText('作業中の文案');
  await userEvent.click(editor);
  fireEvent.change(editor, { target: { value: '失効を待つ間も自分で作業' } });
  await act(async () => pending.reject({ status: 403 }));
  await screen.findByText(/公開文書を利用できません/);
  expect(editor).toHaveFocus();
  expect(editor).toHaveValue('失効を待つ間も自分で作業');
});
