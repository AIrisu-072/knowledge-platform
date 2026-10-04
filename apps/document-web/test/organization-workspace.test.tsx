import { TextEncoder } from "node:util";
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { documentApi } from '../src/application/document-workspace';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { AppShell } from '../src/components/app-shell/AppShell';
import { validateDetailSearch } from '../src/application/search-state';
import { TaskHomePage } from '../src/routes/TaskHomePage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn() } }));

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: false } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null };
const artifact = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' };
const detail = { ...task, inputResources: [{ kind: 'document', documentId: '00000000-0000-4000-8000-000000000010', label: '共有の入力文書' }], workingArtifacts: [artifact], history: [] };
const nextTask = { ...task, id: 'task-2', attemptId: 'attempt-2', title: '事務確認', stepLabel: '事務確認', state: 'ready', canClaim: true, canEdit: false, canSubmit: false };
const snapshot = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: nextTask.id, createdAt: '2026-10-04T05:00:00Z', evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [], artifacts: [{ artifactId: artifact.id, revision: 2, schemaId: artifact.schemaId, value: { text: '提出する文案' } }] };

function setup(entry = '/tasks?view=context&taskId=task-1') {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(session as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: [task], nextCursor: null } as never);
  jest.spyOn(workApi, 'getTask').mockResolvedValue(detail as never);
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'getSnapshot').mockResolvedValue(snapshot as never);
  const root = createRootRoute({ component: Outlet });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const document = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: () => <AppShell><h1>入力文書</h1></AppShell> });
  const router = createRouter({ routeTree: root.addChildren([tasks, document]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { router, client };
}

afterEach(() => jest.restoreAllMocks());

test('task selection stays in URL across the two projections; shared input opens existing Document route', async () => {
  const { router } = setup();
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('保存済みの文案');
  const navigation = screen.getByRole('navigation', { name: 'メインナビゲーション' });
  expect(within(navigation).getAllByRole('link').map((link) => link.textContent?.trim())).toEqual(['タスク', '文書', '検索']);
  expect(screen.getByRole('link', { name: '共有の入力文書' })).toHaveAttribute('href', expect.stringContaining('/documents/00000000-0000-4000-8000-000000000010'));
  await userEvent.click(screen.getByRole('button', { name: '事務型・キュー' }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ view: 'queue', taskId: 'task-1' }));
  expect(screen.getByText(/ブラウザーではネイティブWorkspaceを利用できません/)).toBeVisible();
});

test('save then confirmation submits the exact saved revision and shows immutable receipt', async () => {
  setup();
  const save = jest.spyOn(workApi, 'saveDraft').mockResolvedValue({ kind: 'draft_saved', task: { ...task, revision: 2 }, artifact: { ...artifact, revision: 2, value: { text: '提出する文案' } } } as never);
  const submit = jest.spyOn(workApi, 'submit').mockResolvedValue({ kind: 'submitted', task: { ...task, revision: 3, state: 'completed', canEdit: false, canSubmit: false, handoffSnapshotId: snapshot.id }, snapshot, nextTask } as never);
  const editor = await screen.findByLabelText('作業中の文案');
  fireEvent.change(editor, { target: { value: '提出する文案' } });
  expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
  await userEvent.click(screen.getByRole('button', { name: '文案を保存' }));
  await screen.findByText('文案を保存しました');
  expect(save.mock.calls[0]?.[0]).toMatchObject({ taskId: task.id, artifactId: artifact.id, expectedRevision: 1, actingAssignmentId: session.actingAssignmentId, value: { text: '提出する文案' } });
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  const dialog = screen.getByRole('dialog', { name: '提出の確認' });
  expect(submit).not.toHaveBeenCalled();
  expect(within(dialog).getByText('提出する文案')).toBeVisible();
  await userEvent.click(within(dialog).getByRole('button', { name: '提出を確定' }));
  await screen.findByText('提出が確定しました');
  expect(submit.mock.calls[0]?.[1]).toMatchObject({ expectedRevision: 2, artifacts: [{ artifactId: artifact.id, revision: 2 }] });
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  expect(screen.getByRole('region', { name: '提出済みスナップショット' })).toHaveTextContent('提出する文案');
  expect(screen.getByText(/次のタスク：事務確認/)).toBeVisible();
});

test('eligible queue row exposes no private detail before a successful explicit claim', async () => {
  setup('/tasks?view=queue&taskId=task-2');
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [nextTask], nextCursor: null } as never);
  const claim = jest.spyOn(workApi, 'claim').mockResolvedValue({ kind: 'claimed', task: { ...nextTask, state: 'active', canClaim: false, canEdit: true } } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...nextTask, state: 'active', canClaim: false, canEdit: true, handoffSnapshotId: snapshot.id } as never);
  await screen.findByRole('button', { name: '担当を引き受ける' });
  expect(workApi.getTask).not.toHaveBeenCalled();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: '担当を引き受ける' }));
  await screen.findByLabelText('作業中の文案');
  expect(claim).toHaveBeenCalledTimes(1);
  expect(await screen.findByRole('region', { name: '受領したスナップショット' })).toHaveTextContent('提出する文案');
});

test('changing projection preserves an unsaved draft for the same task', async () => {
  setup();
  const editor = await screen.findByLabelText('作業中の文案');
  fireEvent.change(editor, { target: { value: 'まだ保存していない文案' } });
  await userEvent.click(screen.getByRole('button', { name: '事務型・キュー' }));
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('まだ保存していない文案');
});

test('conflict preserves input and never displays saved success', async () => {
  setup();
  jest.spyOn(workApi, 'saveDraft').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '競合しても残す文案' } });
  await userEvent.click(screen.getByRole('button', { name: '文案を保存' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('競合が発生しました');
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('競合しても残す文案');
  expect(screen.queryByText('文案を保存しました')).not.toBeInTheDocument();
});

test('unknown submit stays unresolved and recovers the same operation without sending a duplicate', async () => {
  setup();
  const submit = jest.spyOn(workApi, 'submit').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  const recovery = jest.spyOn(workApi, 'getOperation').mockResolvedValue({ kind: 'submitted', task: { ...task, state: 'completed', revision: 2, canEdit: false, canSubmit: false, handoffSnapshotId: snapshot.id }, snapshot, nextTask } as never);
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '提出を確定' }));
  await screen.findByText(/結果は未確認です/);
  expect(screen.queryByText('提出が確定しました')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText('提出が確定しました');
  expect(recovery).toHaveBeenCalledWith(submit.mock.calls[0]?.[1].operationId);
  expect(submit).toHaveBeenCalledTimes(1);
});

test('denied mutation removes all now-inaccessible context and private draft', async () => {
  setup();
  jest.spyOn(workApi, 'saveDraft').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '失効した文案' } });
  await userEvent.click(screen.getByRole('button', { name: '文案を保存' }));
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  expect(screen.queryByRole('link', { name: '共有の入力文書' })).not.toBeInTheDocument();
});

test('unsaved task draft stays bound to its identity when selecting another task and returning', async () => {
  setup();
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [task, nextTask], nextCursor: null } as never);
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '元のタスクだけの未保存文案' } });
  await userEvent.click(screen.getByRole('button', { name: /事務確認 担当待ち/ }));
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /内容確認 作業中/ }));
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('元のタスクだけの未保存文案');
});

test('unknown outcome stays bound to the original task across task navigation', async () => {
  setup();
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [task, nextTask], nextCursor: null } as never);
  jest.spyOn(workApi, 'submit').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '提出を確定' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: /事務確認 担当待ち/ }));
  expect(screen.queryByText(/結果は未確認です/)).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /内容確認 作業中/ }));
  await screen.findByRole('button', { name: '同じ操作の結果を確認' });
  expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
});

test('Document round trip restores task URL and its unsaved draft through the task navigation', async () => {
  const { router } = setup('/tasks?view=queue&taskId=task-1');
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '文書を確認中の未保存文案' } });
  await userEvent.click(screen.getByRole('link', { name: '共有の入力文書' }));
  await screen.findByRole('heading', { name: '入力文書' });
  await userEvent.click(within(screen.getByRole('navigation', { name: 'メインナビゲーション' })).getByRole('link', { name: 'タスク' }));
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('文書を確認中の未保存文案');
  expect(router.state.location.search).toMatchObject({ view: 'queue', taskId: 'task-1' });
});

test('submit confirmation focuses cancel and Escape sends no command', async () => {
  setup();
  const submit = jest.spyOn(workApi, 'submit');
  await screen.findByLabelText('作業中の文案');
  const trigger = screen.getByRole('button', { name: '提出内容を確認' });
  await userEvent.click(trigger);
  await waitFor(() => expect(screen.getByRole('button', { name: 'キャンセル' })).toHaveFocus());
  await userEvent.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(submit).not.toHaveBeenCalled();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('保存済みの文案');
  await waitFor(() => expect(trigger).toHaveFocus());
});

test('an unreceived draft write remains unknown after operation 404 and retries the identical command', async () => {
  setup();
  const save = jest.spyOn(workApi, 'saveDraft')
    .mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true))
    .mockResolvedValueOnce({ kind: 'draft_saved', task: { ...task, revision: 2 }, artifact: { ...artifact, revision: 2, value: { text: '未到達でも保持する文案' } } } as never);
  const recovery = jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '未到達でも保持する文案' } });
  await userEvent.click(screen.getByRole('button', { name: '文案を保存' }));
  await screen.findByText(/結果は未確認です/);
  const originalCommand = save.mock.calls[0]![0];
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await waitFor(() => expect(recovery).toHaveBeenCalledWith(originalCommand.operationId));
  expect(await screen.findByText(/操作の確定記録はまだ見つかりません/)).toBeVisible();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('未到達でも保持する文案');
  expect(screen.getByText(/結果は未確認です/)).toHaveTextContent(originalCommand.operationId);
  expect(screen.queryByText('文案を保存しました')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: '同じ操作を再送' }));
  await screen.findByText('文案を保存しました');
  expect(save).toHaveBeenCalledTimes(2);
  expect(save.mock.calls[1]![0]).toEqual(originalCommand);
});

test('a cached old-attempt detail is never rendered under the same task current attempt', async () => {
  const { client } = setup();
  await screen.findByLabelText('作業中の文案');
  const current = { ...task, attemptId: 'sales-attempt-2', attemptNumber: 2, revision: 7 };
  jest.mocked(workApi.getTask).mockReturnValue(new Promise(() => {}));
  client.setQueryData(['organization', session.principalId, session.actingAssignmentId, 'tasks', 'context'], { items: [current], nextCursor: null });
  await screen.findByText(/sales-attempt-2/);
  expect(screen.queryByDisplayValue('保存済みの文案')).not.toBeInTheDocument();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
});

test('late old-attempt operation recovery cannot roll a current ready attempt backward', async () => {
  const { client } = setup();
  let resolveRecovery!: (value: unknown) => void;
  jest.spyOn(workApi, 'submit').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  jest.spyOn(workApi, 'getOperation').mockImplementation(() => new Promise((resolve) => { resolveRecovery = resolve; }) as never);
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '提出を確定' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  const current = { ...task, attemptId: 'sales-attempt-2', attemptNumber: 2, revision: 7, state: 'ready', canClaim: true, canEdit: false, canSubmit: false };
  client.setQueryData(['organization', session.principalId, session.actingAssignmentId, 'tasks', 'context'], { items: [current], nextCursor: null });
  await screen.findByRole('button', { name: '担当を引き受ける' });
  resolveRecovery({ kind: 'submitted', task: { ...task, revision: 3, state: 'completed', canEdit: false, canSubmit: false, handoffSnapshotId: snapshot.id }, snapshot, nextTask });
  await waitFor(() => expect(screen.queryByText('処理中です。サーバーの確定を待っています…')).not.toBeInTheDocument());
  await waitFor(() => expect(client.getQueryData(['organization', session.principalId, session.actingAssignmentId, 'tasks', 'context'])).toEqual({ items: [current], nextCursor: null }));
  expect(screen.getByRole('button', { name: '担当を引き受ける' })).toBeVisible();
  expect(screen.getByText(/sales-attempt-2/)).toBeVisible();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
});

const officeTask = { ...nextTask, attemptId: 'office-attempt-1', state: 'active', canClaim: false, canReturn: true, handoffSnapshotId: snapshot.id, returnTransition: { transitionId: 'office-return', targetTaskId: task.id, previousSubmissionId: snapshot.id } };
const reason = '数量を追記して再提出してください';
const instruction = { id: 'return-1', workflowId: 'workflow-1', contextId: task.contextId, sourceTaskId: officeTask.id, sourceAttemptId: officeTask.attemptId, targetTaskId: task.id, targetAttemptId: 'sales-attempt-2', previousSubmissionId: snapshot.id, transitionId: 'office-return', reason, returnedBy: 'office-01', actingAssignmentId: 'assignment-office', createdAt: '2026-10-04T07:00:00Z' };
const returned = { kind: 'returned', task: { ...officeTask, revision: 2, state: 'completed', canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, returnTransition: null, returnInstructionId: instruction.id }, returnInstruction: instruction, nextTask: { ...task, attemptId: instruction.targetAttemptId, attemptNumber: 2, revision: 4, state: 'ready', canClaim: true, canEdit: false, canSubmit: false, returnInstructionId: instruction.id, handoffSnapshotId: snapshot.id } };
function setupOffice() {
  const context = setup('/tasks?view=queue&taskId=task-2');
  jest.mocked(workApi.getSession).mockResolvedValue({ ...session, principalId: 'office-01', displayName: '事務担当（模擬）', actingAssignmentId: 'assignment-office' } as never);
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [officeTask], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...officeTask, workingArtifacts: [] } as never);
  return context;
}

test('return confirmation preserves the bounded required reason on Cancel and Escape', async () => {
  setupOffice();
  const returnTask = jest.spyOn(workApi, 'returnTask');
  const input = await screen.findByLabelText('差戻理由');
  const trigger = screen.getByRole('button', { name: '差戻内容を確認' });
  expect(trigger).toBeDisabled();
  fireEvent.change(input, { target: { value: ' \n\t' } });
  expect(trigger).toBeDisabled();
  fireEvent.change(input, { target: { value: 'あ'.repeat(2730) + 'ab' } });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.change(input, { target: { value: 'あ'.repeat(2731) } });
  expect(trigger).toBeDisabled();
  expect(screen.getByRole('alert')).toHaveTextContent('8192バイト');
  fireEvent.change(input, { target: { value: reason } });
  await userEvent.click(trigger);
  const dialog = screen.getByRole('dialog', { name: '差戻の確認' });
  expect(within(dialog).getByText(reason)).toBeVisible();
  expect(within(dialog).getByText(/snapshot-1/)).toBeVisible();
  await waitFor(() => expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toHaveFocus());
  await userEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  expect(input).toHaveValue(reason);
  await userEvent.click(trigger);
  await userEvent.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(input).toHaveValue(reason);
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(returnTask).not.toHaveBeenCalled();
});

test('return commits the server transition and leaves office on its own completed read-only task', async () => {
  const { router } = setupOffice();
  jest.spyOn(workApi, 'getReturnInstruction').mockResolvedValue(instruction as never);
  const returnTask = jest.spyOn(workApi, 'returnTask').mockResolvedValue(returned as never);
  fireEvent.change(await screen.findByLabelText('差戻理由'), { target: { value: reason } });
  await waitFor(() => expect(screen.getByRole('button', { name: '差戻内容を確認' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: '差戻内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '差戻を確定' }));
  await screen.findByText('差戻が確定しました');
  expect(returnTask).toHaveBeenCalledTimes(1);
  expect(returnTask.mock.calls[0]).toEqual([officeTask.id, expect.objectContaining({ expectedRevision: officeTask.revision, expectedAttemptId: officeTask.attemptId, actingAssignmentId: 'assignment-office', ...officeTask.returnTransition, reason })]);
  expect(router.state.location.search).toMatchObject({ taskId: officeTask.id });
  expect(screen.getByRole('region', { name: '確定した差戻指示' })).toHaveTextContent(reason);
  expect(screen.getByRole('region', { name: '受領したスナップショット' })).toHaveTextContent('提出する文案');
  expect(screen.queryByLabelText('差戻理由')).not.toBeInTheDocument();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '担当を引き受ける' })).not.toBeInTheDocument();
});

test('unknown return keeps its reason and replays exactly the original operation after missing recovery', async () => {
  setupOffice();
  jest.spyOn(workApi, 'getReturnInstruction').mockResolvedValue(instruction as never);
  const returnTask = jest.spyOn(workApi, 'returnTask').mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true)).mockResolvedValueOnce(returned as never);
  const recovery = jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  fireEvent.change(await screen.findByLabelText('差戻理由'), { target: { value: reason } });
  await waitFor(() => expect(screen.getByRole('button', { name: '差戻内容を確認' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: '差戻内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '差戻を確定' }));
  await screen.findByText(/結果は未確認です/);
  expect(screen.queryByText('差戻が確定しました')).not.toBeInTheDocument();
  expect(screen.getByLabelText('差戻理由')).toHaveValue(reason);
  expect(screen.getByLabelText('差戻理由')).toBeDisabled();
  const command = returnTask.mock.calls[0]![1];
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText(/操作の確定記録はまだ見つかりません/);
  expect(recovery).toHaveBeenCalledWith(command.operationId);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作を再送' }));
  await screen.findByText('差戻が確定しました');
  expect(returnTask.mock.calls[1]).toEqual(returnTask.mock.calls[0]);
});

test('return conflict preserves the reason and never claims completion', async () => {
  setupOffice();
  jest.spyOn(workApi, 'returnTask').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  fireEvent.change(await screen.findByLabelText('差戻理由'), { target: { value: reason } });
  await waitFor(() => expect(screen.getByRole('button', { name: '差戻内容を確認' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: '差戻内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '差戻を確定' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('競合が発生しました');
  expect(screen.getByLabelText('差戻理由')).toHaveValue(reason);
  expect(screen.queryByText('差戻が確定しました')).not.toBeInTheDocument();
});

test('returned sales attempt shows the fixed reason and old submission with a fresh empty private editor', async () => {
  setup();
  const current = { ...returned.nextTask, state: 'active', canClaim: false, canEdit: true, canSubmit: true };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [current], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current, workingArtifacts: [] } as never);
  jest.spyOn(workApi, 'getReturnInstruction').mockResolvedValue(instruction as never);
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('');
  expect(await screen.findByRole('region', { name: '確定した差戻指示' })).toHaveTextContent(reason);
  expect(await screen.findByRole('region', { name: '提出済みスナップショット' })).toHaveTextContent('提出する文案');
  expect(screen.queryByLabelText('差戻理由')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
});

test('a task without a current return hint never offers a return even if the session supports it', async () => {
  setupOffice();
  jest.mocked(workApi.getSession).mockResolvedValue({ ...session, capabilities: { ...session.capabilities, return: true } } as never);
  const current = { ...officeTask, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, returnTransition: null };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [current], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current, workingArtifacts: [] } as never);
  await screen.findByRole('region', { name: '受領したスナップショット' });
  expect(screen.queryByLabelText('差戻理由')).not.toBeInTheDocument();
});

const evidenceRecord = { id: 'evidence-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, sourceRef: { providerId: 'document', resourceId: detail.inputResources[0]!.documentId, revisionId: 'revision-1', versionId: 'version-1' }, authoritativeLocator: { kind: 'contentItem', contentItemId: 'content-1', representationId: 'representation-1' }, relevantLocation: '第1節', origin: 'human', fragmentOmissionReason: 'not_retained', coverage: 'unknown', relevantLocationVerified: false, policyDisposition: 'reference_only', uncertainty: [], conflictReferences: [], createdBy: 'sales-01', actingAssignmentId: 'assignment-sales', recordedAt: '2026-10-04T08:00:00Z', retrievedAt: '2026-10-04T08:00:00Z', providerCheckedAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private' };
const findingRecord = { id: 'finding-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, claim: '検討中の候補', evidenceRevisionRefs: [{ id: evidenceRecord.id, revision: 1 }], author: 'sales-01', actingAssignmentId: 'assignment-sales', createdAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private', uncertainty: [], conflicts: [], supersedesFindingId: null };
const sourceDocument = { documentId: detail.inputResources[0]!.documentId, documentVersionId: 'version-1', displayRevision: { revisionId: 'revision-1', documentVersionId: 'version-1', label: '1.0' } };
const sourceFile = { contentItemId: 'content-1', representationId: 'representation-1', role: 'AUTHORITATIVE', displayName: '原本.txt' };
async function openEvidence() { await screen.findByLabelText('作業中の文案'); await userEvent.click(screen.getByRole('button', { name: '根拠' })); }
function mockRecords() { jest.mocked(workApi.listEvidence).mockResolvedValue({ items: [evidenceRecord], nextCursor: null } as never); jest.mocked(workApi.listFindings).mockResolvedValue({ items: [findingRecord], nextCursor: null } as never); }

test('actual source selection registers only exact published revision and authoritative file references', async () => {
  setup();
  jest.spyOn(documentApi, 'getDocument').mockResolvedValue(sourceDocument as never);
  jest.spyOn(documentApi, 'listVersionFiles').mockResolvedValue({ items: [sourceFile] } as never);
  const register = jest.spyOn(workApi, 'registerEvidence').mockResolvedValue({ kind: 'evidence_registered', task: { ...task, revision: 2 }, evidence: evidenceRecord } as never);
  await openEvidence();
  await userEvent.selectOptions(screen.getByLabelText('根拠にする入力文書'), sourceDocument.documentId);
  await userEvent.selectOptions(await screen.findByLabelText('原本ファイル'), 'content-1:representation-1');
  fireEvent.change(screen.getByLabelText('該当箇所（人間の記載・未検証）'), { target: { value: '第1節' } });
  expect(screen.getByText(/原本本文は保持しません/)).toBeVisible();
  expect(screen.getByText('根拠・候補・各候補の判断はこのPoCでは可視16件までです')).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: '根拠を登録' }));
  await waitFor(() => expect(register).toHaveBeenCalledTimes(1));
  expect(register.mock.calls[0]).toEqual([task.id, expect.objectContaining({ expectedAttemptId: task.attemptId, sourceRef: evidenceRecord.sourceRef, authoritativeLocator: evidenceRecord.authoritativeLocator, relevantLocation: '第1節' })]);
  expect(JSON.stringify(register.mock.calls[0])).not.toContain('原本.txt');
});
test('human judgment has separate candidate/support, modified validation, cancel focus and no workflow action', async () => {
  setup(); mockRecords();
  const record = jest.spyOn(workApi, 'recordDecision').mockResolvedValue({ kind: 'decision_recorded', task: { ...task, revision: 2 }, decision: { id: 'decision-1', taskId: task.id, attemptId: task.attemptId } } as never);
  const submit = jest.spyOn(workApi, 'submit'); const returning = jest.spyOn(workApi, 'returnTask');
  await openEvidence();
  const candidate = await screen.findByRole('region', { name: '候補 finding-1' });
  expect(candidate).toHaveTextContent('検討中の候補'); expect(candidate).toHaveTextContent('evidence-1');
  await userEvent.selectOptions(within(candidate).getByLabelText('候補の判断 finding-1'), 'modified');
  expect(within(candidate).getByRole('button', { name: '判断内容を確認' })).toBeDisabled();
  fireEvent.change(within(candidate).getByLabelText('採用文'), { target: { value: '修正した採用文' } });
  await userEvent.click(within(candidate).getByRole('button', { name: '判断内容を確認' }));
  const dialog = screen.getByRole('dialog', { name: '人間判断の確認' });
  await waitFor(() => expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toHaveFocus());
  await userEvent.keyboard('{Escape}');
  expect(record).not.toHaveBeenCalled();
  expect(within(candidate).getByLabelText('採用文')).toHaveValue('修正した採用文');
  await userEvent.click(within(candidate).getByRole('button', { name: '判断内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '判断を確定' }));
  await waitFor(() => expect(record).toHaveBeenCalledTimes(1));
  expect(record.mock.calls[0]).toEqual([findingRecord.id, expect.objectContaining({ taskId: task.id, expectedAttemptId: task.attemptId, findingRevision: 1, decision: 'modified', adoptedClaim: '修正した採用文', evidenceRevisionRefs: findingRecord.evidenceRevisionRefs })]);
  expect(submit).not.toHaveBeenCalled(); expect(returning).not.toHaveBeenCalled();
});
test('submission exposes explicit selection, preserves unselected privacy and requires support closure', async () => {
  setup(); mockRecords();
  const submit = jest.spyOn(workApi, 'submit').mockResolvedValue({ kind: 'submitted', task: { ...task, revision: 2, state: 'completed', canEdit: false, canSubmit: false }, snapshot, nextTask } as never);
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  const dialog = screen.getByRole('dialog', { name: '提出の確認' });
  const candidate = await within(dialog).findByLabelText('共有する候補 finding-1');
  expect(candidate).not.toBeChecked(); expect(within(dialog).getByLabelText('共有する根拠 evidence-1')).not.toBeChecked();
  await userEvent.click(candidate);
  expect(within(dialog).getByRole('button', { name: '提出を確定' })).toBeDisabled();
  await userEvent.click(within(dialog).getByLabelText('共有する根拠 evidence-1'));
  await userEvent.click(within(dialog).getByRole('button', { name: '提出を確定' }));
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
  expect(submit.mock.calls[0]?.[1]).toMatchObject({ evidenceRevisionRefs: [{ id: 'evidence-1', revision: 1 }], findingRevisionRefs: [{ id: 'finding-1', revision: 1 }], decisionRevisionRefs: [] });
});

test('office can record an independent judgment on received candidate without draft editing authority', async () => {
  setup('/tasks?view=queue&taskId=task-1'); mockRecords();
  const officeTask = { ...task, canEdit: false, canSubmit: false, canRecordDecision: true, attemptId: 'office-attempt', handoffSnapshotId: snapshot.id };
  jest.mocked(workApi.getSnapshot).mockResolvedValue({ ...snapshot, evidenceRevisionRefs: [{ id: evidenceRecord.id, revision: 1 }], findingRevisionRefs: [{ id: findingRecord.id, revision: 1 }] } as never);
  jest.mocked(workApi.getSession).mockResolvedValue({ ...session, principalId: 'office-01', actingAssignmentId: 'assignment-office' } as never);
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [officeTask], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...officeTask, workingArtifacts: [] } as never);
  const record = jest.spyOn(workApi, 'recordDecision').mockResolvedValue({ kind: 'decision_recorded', task: { ...officeTask, revision: 2 }, decision: { id: 'office-decision', taskId: task.id, attemptId: 'office-attempt' } } as never);
  await screen.findByText(/現在のタスクは読み取り専用/);
  await userEvent.click(screen.getByRole('button', { name: '根拠' }));
  const candidate = await screen.findByRole('region', { name: '候補 finding-1' });
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  await userEvent.click(within(candidate).getByRole('button', { name: '判断内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '判断を確定' }));
  await waitFor(() => expect(record).toHaveBeenCalledTimes(1));
  expect(record.mock.calls[0]?.[1]).toMatchObject({ taskId: task.id, expectedAttemptId: 'office-attempt', actingAssignmentId: 'assignment-office', decision: 'accepted', findingRevision: 1 });
});
test('unknown evidence operation keeps the original payload and recovers without duplicate registration', async () => {
  setup(); mockRecords();
  const register = jest.spyOn(workApi, 'registerFinding').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  const recover = jest.spyOn(workApi, 'getOperation').mockResolvedValue({ kind: 'finding_registered', task: { ...task, revision: 2 }, finding: findingRecord } as never);
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '未確定の候補' } });
  await userEvent.click(await screen.findByLabelText('候補の根拠 evidence-1'));
  await userEvent.click(screen.getByRole('button', { name: '候補を登録' }));
  await screen.findByText(/結果は未確認です/);
  expect(screen.getByRole('button', { name: '候補を登録' })).toBeDisabled();
  await userEvent.click(screen.getByRole('button', { name: '文書・比較' }));
  await userEvent.click(screen.getByRole('button', { name: '根拠' }));
  expect(screen.getByLabelText('候補の主張')).toHaveValue('未確定の候補');
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText('候補を登録しました');
  expect(register).toHaveBeenCalledTimes(1);
  expect(recover).toHaveBeenCalledWith(register.mock.calls[0]?.[1].operationId);
});
test('source denial clears source metadata, candidate inputs, selections and registered context', async () => {
  setup(); mockRecords();
  jest.spyOn(documentApi, 'getDocument').mockRejectedValue({ status: 403, code: 'DOCUMENT_FORBIDDEN' });
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '失効した候補' } });
  await userEvent.selectOptions(screen.getByLabelText('根拠にする入力文書'), sourceDocument.documentId);
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByLabelText('候補の主張')).not.toBeInTheDocument();
  expect(screen.queryByText('検討中の候補')).not.toBeInTheDocument();
  expect(screen.queryByText('失効した候補')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: '根拠 evidence-1' })).not.toBeInTheDocument();
});
test('stale attempt conflict retains the candidate and never changes its target to a new operation', async () => {
  setup(); mockRecords();
  const register = jest.spyOn(workApi, 'registerFinding').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '競合した候補' } });
  await userEvent.click(await screen.findByLabelText('候補の根拠 evidence-1'));
  await userEvent.click(screen.getByRole('button', { name: '候補を登録' }));
  await screen.findByText(/競合が発生しました/);
  expect(screen.getByLabelText('候補の主張')).toHaveValue('競合した候補');
  expect(register.mock.calls[0]?.[1]).toMatchObject({ expectedAttemptId: task.attemptId });
  expect(screen.queryByText('候補を登録しました')).not.toBeInTheDocument();
});
test('source selection and candidate text survive module and Document navigation in the same attempt', async () => {
  setup(); mockRecords();
  jest.spyOn(documentApi, 'getDocument').mockResolvedValue(sourceDocument as never);
  jest.spyOn(documentApi, 'listVersionFiles').mockResolvedValue({ items: [sourceFile] } as never);
  await openEvidence();
  await userEvent.selectOptions(screen.getByLabelText('根拠にする入力文書'), sourceDocument.documentId);
  await userEvent.selectOptions(await screen.findByLabelText('原本ファイル'), 'content-1:representation-1');
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '戻った時の候補' } });
  await userEvent.click(screen.getAllByRole('link', { name: '版・改訂を確認' })[0]!);
  await screen.findByRole('heading', { name: '入力文書' });
  await userEvent.click(screen.getByRole('link', { name: 'タスク' }));
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '根拠' }));
  expect(await screen.findByLabelText('候補の主張')).toHaveValue('戻った時の候補');
  expect(screen.getByLabelText('根拠にする入力文書')).toHaveValue(sourceDocument.documentId);
  expect(await screen.findByLabelText('原本ファイル')).toHaveValue('content-1:representation-1');
});
test('the exact evidence original action requests its recorded version and file, never the first current file', async () => {
  setup(); mockRecords();
  jest.spyOn(workApi, 'getEvidence').mockResolvedValue(evidenceRecord as never);
  const download = jest.spyOn(documentApi, 'downloadVersionFile').mockRejectedValue({ status: 503 });
  await openEvidence();
  const evidence = await screen.findByRole('region', { name: '根拠 evidence-1' });
  await userEvent.click(within(evidence).getByRole('button', { name: 'この根拠の原本を取得' }));
  expect(download).toHaveBeenCalledWith({ documentId: evidenceRecord.sourceRef.resourceId, versionId: 'version-1', contentItemId: 'content-1', representationId: 'representation-1', purpose: 'history' });
  expect(documentApi.listVersionFiles).not.toHaveBeenCalled();
});

test('candidate support and UTF-8 limits prevent unsupported or oversized registration', async () => {
  setup(); mockRecords();
  const register = jest.spyOn(workApi, 'registerFinding');
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '候補' } });
  expect(screen.getByRole('button', { name: '候補を登録' })).toBeDisabled();
  await userEvent.click(await screen.findByLabelText('候補の根拠 evidence-1'));
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: 'あ'.repeat(2731) } });
  expect(screen.getByRole('button', { name: '候補を登録' })).toBeDisabled();
  expect(register).not.toHaveBeenCalled();
});
test('a late response from a replaced attempt cannot roll back the current task or restore private inputs', async () => {
  const { client } = setup(); mockRecords();
  let resolve!: (value: never) => void;
  jest.spyOn(workApi, 'registerFinding').mockImplementation(() => new Promise((done) => { resolve = done; }));
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '古い試行の候補' } });
  await userEvent.click(await screen.findByLabelText('候補の根拠 evidence-1'));
  await userEvent.click(screen.getByRole('button', { name: '候補を登録' }));
  const replacement = { ...task, attemptId: 'attempt-new', attemptNumber: 2, revision: 4 };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [replacement], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...replacement, workingArtifacts: [] } as never);
  jest.mocked(workApi.listEvidence).mockResolvedValue({ items: [], nextCursor: null });
  jest.mocked(workApi.listFindings).mockResolvedValue({ items: [], nextCursor: null });
  await userEvent.click(screen.getByRole('button', { name: '再読込' }));
  await screen.findByText(/試行 2（attempt-new）/);
  resolve({ kind: 'finding_registered', task: { ...task, revision: 2 }, finding: findingRecord } as never);
  await waitFor(() => expect(screen.queryByText('古い試行の候補')).not.toBeInTheDocument());
  expect(screen.queryByText('検討中の候補')).not.toBeInTheDocument();
  expect(client.getQueryData(['organization', session.principalId, session.actingAssignmentId, 'task', task.id, 'attempt-new'])).toMatchObject({ attemptId: 'attempt-new', revision: 4 });
  expect(screen.getByLabelText('候補の主張')).toHaveValue('');
});

test('loss of authenticated session discards private evidence inputs before the same actor signs in again', async () => {
  const { client } = setup(); mockRecords();
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '失効時に消す候補' } });
  jest.mocked(workApi.getSession).mockRejectedValue(new WorkApiError(401, 'UNAUTHENTICATED'));
  await act(async () => { await client.refetchQueries({ queryKey: ['organization-session'] }); });
  await screen.findByRole('heading', { name: 'タスクを開けません' });
  jest.mocked(workApi.getSession).mockResolvedValue(session as never);
  await act(async () => { await client.refetchQueries({ queryKey: ['organization-session'] }); });
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '根拠' }));
  expect(await screen.findByLabelText('候補の主張')).toHaveValue('');
});

test('source-specific denial during outcome recovery clears evidence instead of treating it as an absent operation', async () => {
  setup(); mockRecords();
  jest.spyOn(workApi, 'registerFinding').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'EVIDENCE_NOT_FOUND'));
  await openEvidence();
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '回復時に消す候補' } });
  await userEvent.click(await screen.findByLabelText('候補の根拠 evidence-1'));
  await userEvent.click(screen.getByRole('button', { name: '候補を登録' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByLabelText('候補の主張')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: '根拠 evidence-1' })).not.toBeInTheDocument();
});

test('exact original download rechecks Work visibility before requesting the provider bytes', async () => {
  setup(); mockRecords();
  jest.spyOn(workApi, 'getEvidence').mockRejectedValue(new WorkApiError(404, 'EVIDENCE_NOT_FOUND'));
  const download = jest.spyOn(documentApi, 'downloadVersionFile').mockRejectedValue({ status: 503 });
  await openEvidence();
  const evidence = await screen.findByRole('region', { name: '根拠 evidence-1' });
  await userEvent.click(within(evidence).getByRole('button', { name: 'この根拠の原本を取得' }));
  await screen.findByText(/内容を非表示にしました/);
  expect(download).not.toHaveBeenCalled();
});

test('immutable handoff rendering exposes the selected evidence candidate and judgment revisions', async () => {
  setup();
  const completed = { ...task, state: 'completed', canEdit: false, canSubmit: false, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, handoffSnapshotId: snapshot.id };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [completed], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...completed } as never);
  jest.mocked(workApi.getSnapshot).mockResolvedValue({ ...snapshot, evidenceRevisionRefs: [{ id: 'evidence-shared', revision: 1 }], findingRevisionRefs: [{ id: 'finding-shared', revision: 1 }], decisionRevisionRefs: [{ id: 'decision-shared', revision: 1 }] } as never);
  const receipt = await screen.findByRole('region', { name: '提出済みスナップショット' });
  expect(receipt).toHaveTextContent('共有された根拠：evidence-shared（版 1）');
  expect(receipt).toHaveTextContent('共有された候補：finding-shared（版 1）');
  expect(receipt).toHaveTextContent('共有された判断：decision-shared（版 1）');
});

test('reload retries unavailable evidence collections without requiring a changed task revision', async () => {
  setup();
  jest.mocked(workApi.listEvidence).mockRejectedValue(new WorkApiError(503, 'SOURCE_UNAVAILABLE'));
  await openEvidence();
  await screen.findByText(/サーバーから結果を取得できませんでした/);
  mockRecords();
  await userEvent.click(screen.getByRole('button', { name: '再読込' }));
  expect(await screen.findByRole('region', { name: '根拠 evidence-1' })).toHaveTextContent('第1節');
  expect(await screen.findByRole('region', { name: '候補 finding-1' })).toHaveTextContent('検討中の候補');
});
test('reload retries unavailable Document source files for the same revision', async () => {
  setup();
  jest.spyOn(documentApi, 'getDocument').mockResolvedValue(sourceDocument as never);
  jest.spyOn(documentApi, 'listVersionFiles').mockRejectedValue({ status: 503 });
  await openEvidence();
  await userEvent.selectOptions(screen.getByLabelText('根拠にする入力文書'), sourceDocument.documentId);
  await screen.findByText('原本情報を取得できません。再読込して確認してください。');
  jest.mocked(documentApi.listVersionFiles).mockResolvedValue({ items: [sourceFile] } as never);
  await userEvent.click(screen.getByRole('button', { name: '再読込' }));
  expect(await screen.findByLabelText('原本ファイル')).toBeVisible();
});
