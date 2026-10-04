import { TextEncoder } from "node:util";
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { AppShell } from '../src/components/app-shell/AppShell';
import { validateDetailSearch } from '../src/application/search-state';
import { TaskHomePage } from '../src/routes/TaskHomePage';

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: false } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canReturn: false, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null };
const artifact = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' };
const detail = { ...task, inputResources: [{ kind: 'document', documentId: '00000000-0000-4000-8000-000000000010', label: '共有の入力文書' }], workingArtifacts: [artifact], history: [] };
const nextTask = { ...task, id: 'task-2', attemptId: 'attempt-2', title: '事務確認', stepLabel: '事務確認', state: 'ready', canClaim: true, canEdit: false, canSubmit: false };
const snapshot = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: nextTask.id, createdAt: '2026-10-04T05:00:00Z', artifacts: [{ artifactId: artifact.id, revision: 2, schemaId: artifact.schemaId, value: { text: '提出する文案' } }] };

function setup(entry = '/tasks?view=context&taskId=task-1') {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(session as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: [task], nextCursor: null } as never);
  jest.spyOn(workApi, 'getTask').mockResolvedValue(detail as never);
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
const returned = { kind: 'returned', task: { ...officeTask, revision: 2, state: 'completed', canReturn: false, returnTransition: null, returnInstructionId: instruction.id }, returnInstruction: instruction, nextTask: { ...task, attemptId: instruction.targetAttemptId, attemptNumber: 2, revision: 4, state: 'ready', canClaim: true, canEdit: false, canSubmit: false, returnInstructionId: instruction.id, handoffSnapshotId: snapshot.id } };
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
  const current = { ...officeTask, canReturn: false, returnTransition: null };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [current], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current, workingArtifacts: [] } as never);
  await screen.findByRole('region', { name: '受領したスナップショット' });
  expect(screen.queryByLabelText('差戻理由')).not.toBeInTheDocument();
});
