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

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', capabilities: { nativeWorkspace: false, agent: true, search: false, fileUpload: false, return: false } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null };
const artifact = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' };
const detail = { ...task, inputResources: [{ kind: 'document', documentId: '00000000-0000-4000-8000-000000000010', label: '共有の入力文書' }], workingArtifacts: [artifact], history: [], agentExecutionIds: [] };
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
const returned = { kind: 'returned', task: { ...officeTask, revision: 2, state: 'completed', canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: instruction.id }, returnInstruction: instruction, nextTask: { ...task, attemptId: instruction.targetAttemptId, attemptNumber: 2, revision: 4, state: 'ready', canClaim: true, canEdit: false, canSubmit: false, returnInstructionId: instruction.id, handoffSnapshotId: snapshot.id } };
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
  const current = { ...officeTask, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null };
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

test('real Playwright exact label matching identifies source file and judgment selects without option text', async () => {
  const matches = require('./playwright-label-matcher.cjs')() as (element: Element, name: string) => boolean;
  setup(); mockRecords();
  jest.spyOn(documentApi, 'getDocument').mockResolvedValue(sourceDocument as never);
  jest.spyOn(documentApi, 'listVersionFiles').mockResolvedValue({ items: [sourceFile] } as never);
  await openEvidence();
  const source = screen.getByLabelText('根拠にする入力文書');
  await userEvent.selectOptions(source, sourceDocument.documentId);
  const file = await screen.findByLabelText('原本ファイル');
  const judgment = screen.getByLabelText('候補の判断 finding-1');
  expect([matches(source, '根拠にする入力文書'), matches(file, '原本ファイル'), matches(judgment, '候補の判断 finding-1')]).toEqual([true, true, true]);
  expect(matches(judgment, '候補の判断')).toBe(false);
  expect(matches(source, '共有の入力文書')).toBe(false);
});

test('real Playwright exact textarea labels remain stable after controlled input', async () => {
  const matches = require('./playwright-label-matcher.cjs')() as (element: Element, name: string) => boolean;
  setup(); mockRecords();
  await openEvidence();
  await userEvent.selectOptions(screen.getByLabelText('候補の判断 finding-1'), 'modified');
  const labels = ['該当箇所（人間の記載・未検証）', '候補の主張', '判断理由', '採用文'];
  const fields = labels.map((label) => screen.getByLabelText(label) as HTMLTextAreaElement);
  expect(fields.map((field, index) => matches(field, labels[index]!))).toEqual([true, true, true, true]);
  for (const field of fields) await userEvent.type(field, 'synthetic probe');
  for (const field of fields) {
    expect(field.value).toBe('synthetic probe');
    expect(field.defaultValue).toBe('synthetic probe');
    expect(field.textContent).toBe('synthetic probe');
  }
  expect(fields.map((field, index) => matches(field, labels[index]!))).toEqual([true, true, true, true]);
});


test('Agent module labels fixed-rule simulation and requires bounded purpose plus exact existing evidence', async () => {
  setup(); mockRecords();
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  const module = await screen.findByRole('region', { name: '合成Agent' });
  expect(module).toHaveTextContent('固定規則の模擬処理');
  expect(module).toHaveTextContent('原本本文を分析しません');
  expect(module).toHaveTextContent('実LLM・MCP通信は使用しません');
  const purpose = within(module).getByLabelText('Agentへの依頼目的', { exact: true });
  const request = within(module).getByRole('button', { name: '合成Agentに依頼' });
  expect(request).toBeDisabled();
  fireEvent.change(purpose, { target: { value: '参照の確認' } });
  expect(request).toBeDisabled();
  await userEvent.click(await within(module).findByLabelText('Agentの根拠 evidence-1', { exact: true }));
  expect(request).toBeEnabled();
  fireEvent.change(purpose, { target: { value: 'あ'.repeat(2731) } });
  expect(request).toBeDisabled();
  fireEvent.change(purpose, { target: { value: '   ' } });
  expect(request).toBeDisabled();
  fireEvent.change(purpose, { target: { value: '参照を確認する' } });
  expect(request).toBeEnabled();
  const matches = require('./playwright-label-matcher.cjs')() as (element: Element, name: string) => boolean;
  expect(matches(purpose, 'Agentへの依頼目的')).toBe(true);
});

test('generated Finding preserves visible execution provenance in the existing Evidence module', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.listFindings).mockResolvedValue({ items: [{ ...findingRecord, author: 'organization-synthetic/agent-01', originExecutionId: 'execution-1' }], nextCursor: null } as never);
  await openEvidence();
  const candidate = await screen.findByRole('region', { name: '候補 finding-1' });
  expect(candidate).toHaveTextContent('organization-synthetic/agent-01');
  expect(candidate).toHaveTextContent('生成元の実行 execution-1');
  expect(candidate).toHaveTextContent('合成実行');
});

test('Agent request authority is separate from office draft editing authority', async () => {
  setup(); mockRecords();
  const officeTask = { ...task, canEdit: false, canSubmit: false, canRequestAgent: true };
  jest.mocked(workApi.getSession).mockResolvedValue({ ...session, principalId: 'office-01', actingAssignmentId: 'assignment-office' } as never);
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [officeTask], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...officeTask, workingArtifacts: [] } as never);
  await screen.findByText(/現在のタスクは読み取り専用/);
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  fireEvent.change(screen.getByLabelText('Agentへの依頼目的'), { target: { value: '受領した根拠の参照を確認' } });
  await userEvent.click(await screen.findByLabelText('Agentの根拠 evidence-1'));
  expect(screen.getByRole('button', { name: '合成Agentに依頼' })).toBeEnabled();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
});

test('switching task clears Agent input and selection while preserving the Work draft', async () => {
  const { router } = setup(); mockRecords();
  const other = { ...task, id: 'task-other', attemptId: 'attempt-other' };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [task, other], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockImplementation(async (id) => ({ ...detail, ...(id === task.id ? task : other), workingArtifacts: id === task.id ? [artifact] : [] }) as never);
  await screen.findByLabelText('作業中の文案');
  fireEvent.change(screen.getByLabelText('作業中の文案'), { target: { value: '保存を待つ文案' } });
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  fireEvent.change(screen.getByLabelText('Agentへの依頼目的'), { target: { value: '他タスクに持ち越さない目的' } });
  await userEvent.click(await screen.findByLabelText('Agentの根拠 evidence-1'));
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: other.id } } as never); });
  await screen.findByText(/タスク task-other/);
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: task.id } } as never); });
  await screen.findByText(/タスク task-1/);
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  expect(await screen.findByLabelText('Agentへの依頼目的')).toHaveValue('');
  expect(await screen.findByLabelText('Agentの根拠 evidence-1')).not.toBeChecked();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('保存を待つ文案');
});

test('Agent selection is bounded to sixteen exact references', async () => {
  setup();
  jest.mocked(workApi.listEvidence).mockResolvedValue({ items: Array.from({ length: 17 }, (_, index) => ({ ...evidenceRecord, id: `evidence-${index + 1}` })), nextCursor: null } as never);
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  fireEvent.change(screen.getByLabelText('Agentへの依頼目的'), { target: { value: '参照を確認する' } });
  for (const checkbox of await screen.findAllByRole('checkbox', { name: /^Agentの根拠 / })) await userEvent.click(checkbox);
  expect(screen.getByRole('button', { name: '合成Agentに依頼' })).toBeDisabled();
  await userEvent.click(screen.getByLabelText('Agentの根拠 evidence-17'));
  expect(screen.getByRole('button', { name: '合成Agentに依頼' })).toBeEnabled();
});

test('changing acting responsibility clears Agent private input before restoring the same responsibility', async () => {
  const { client } = setup(); mockRecords();
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  fireEvent.change(screen.getByLabelText('Agentへの依頼目的'), { target: { value: '担当が変わる前の目的' } });
  jest.mocked(workApi.getSession).mockResolvedValue({ ...session, actingAssignmentId: 'another-assignment' } as never);
  await act(async () => { await client.refetchQueries({ queryKey: ['organization-session'] }); });
  await screen.findByText(/担当: another-assignment/);
  jest.mocked(workApi.getSession).mockResolvedValue(session as never);
  await act(async () => { await client.refetchQueries({ queryKey: ['organization-session'] }); });
  await screen.findByText(/担当: assignment-sales/);
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  expect(await screen.findByLabelText('Agentへの依頼目的')).toHaveValue('');
});

const syntheticResult = { summary: '固定規則の合成候補を記録しました', findingRevisionRefs: [{ id: 'synthetic-finding', revision: 1 }], evidenceRevisionRefs: [{ id: 'evidence-1', revision: 1 }], uncertainty: ['原本本文は未検証'], simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false };
const syntheticExecution = { id: 'execution-1', contextId: task.contextId, workItemId: task.id, attemptId: task.attemptId, requestedBy: session.principalId, requesterResponsibility: session.actingAssignmentId, executedBy: 'organization-synthetic/agent-01', executorInvocationKind: 'agent', providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' }], effectiveContextRevision: 2, taskRevision: 2, purpose: '参照を確認', evidenceRevisionRefs: syntheticResult.evidenceRevisionRefs, status: 'queued', startedAt: '2026-10-04T13:00:00Z', endedAt: null, result: null, failureCode: null };
async function openAgent() {
  await screen.findByRole('heading', { name: '内容確認' });
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  await screen.findByRole('region', { name: '合成Agent' });
}
async function fillAgent() {
  await openAgent();
  fireEvent.change(screen.getByLabelText('Agentへの依頼目的'), { target: { value: '参照を確認' } });
  await userEvent.click(await screen.findByLabelText('Agentの根拠 evidence-1'));
}

test('synthetic request becomes a persisted candidate and opens existing Human judgment without automatic actions', async () => {
  setup(); mockRecords();
  const generated = { ...findingRecord, id: 'synthetic-finding', claim: '合成候補の主張', author: 'organization-synthetic/agent-01', originExecutionId: syntheticExecution.id };
  const request = jest.spyOn(workApi, 'requestAgentExecution').mockImplementation(async () => {
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 3, agentExecutionIds: [syntheticExecution.id] } as never);
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [{ ...task, revision: 3 }], nextCursor: null } as never);
    jest.mocked(workApi.listFindings).mockResolvedValue({ items: [generated], nextCursor: null } as never);
    return { kind: 'agent_execution_requested', task: { ...task, revision: 2, canRequestAgent: false }, execution: syntheticExecution } as never;
  });
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'succeeded', taskRevision: 3, result: syntheticResult } as never);
  jest.spyOn(workApi, 'getAgentResult').mockResolvedValue(syntheticResult as never);
  const decide = jest.spyOn(workApi, 'recordDecision');
  const submit = jest.spyOn(workApi, 'submit');
  const returning = jest.spyOn(workApi, 'returnTask');
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  const result = await screen.findByRole('region', { name: 'Agent実行 execution-1' });
  expect(result).toHaveTextContent('成功');
  expect(result).toHaveTextContent('固定規則の合成候補を記録しました');
  expect(result).toHaveTextContent('organization-synthetic/agent-01');
  expect(result).toHaveTextContent('poc/poc-agent');
  expect(request).toHaveBeenCalledTimes(1);
  expect(request.mock.calls[0]).toEqual([task.id, expect.objectContaining({ expectedRevision: 1, expectedAttemptId: task.attemptId, actingAssignmentId: session.actingAssignmentId, purpose: '参照を確認', evidenceRevisionRefs: syntheticResult.evidenceRevisionRefs })]);
  await waitFor(() => expect(screen.getByRole('button', { name: '候補を根拠モジュールで確認' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: '候補を根拠モジュールで確認' }));
  expect(await screen.findByRole('region', { name: '候補 synthetic-finding' })).toHaveTextContent('合成候補の主張');
  expect(decide).not.toHaveBeenCalled(); expect(submit).not.toHaveBeenCalled(); expect(returning).not.toHaveBeenCalled();
});

test('repeated Agent request stays one operation and unknown recovery reuses its exact payload', async () => {
  setup(); mockRecords();
  const request = jest.spyOn(workApi, 'requestAgentExecution').mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true)).mockResolvedValue({ kind: 'agent_execution_requested', task: { ...task, revision: 2 }, execution: syntheticExecution } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue(syntheticExecution as never);
  const recover = jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await fillAgent();
  const button = screen.getByRole('button', { name: '合成Agentに依頼' });
  fireEvent.click(button); fireEvent.click(button);
  await screen.findByText(/結果は未確認です/);
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText('Agentへの依頼目的')).toBeDisabled();
  await userEvent.click(screen.getByRole('button', { name: '文書・比較' }));
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await userEvent.click(await screen.findByRole('button', { name: '同じ操作を再送' }));
  await screen.findByRole('region', { name: 'Agent実行 execution-1' });
  expect(request).toHaveBeenCalledTimes(2);
  expect(request.mock.calls[1]).toEqual(request.mock.calls[0]);
  expect(recover).toHaveBeenCalledWith(request.mock.calls[0]?.[1].operationId);
});

test('an interrupted persisted execution stays explicitly unknown and never auto-dispatches or reads a result', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, agentExecutionIds: [syntheticExecution.id] } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'outcome_unknown', failureCode: 'interrupted' } as never);
  const request = jest.spyOn(workApi, 'requestAgentExecution');
  const readResult = jest.spyOn(workApi, 'getAgentResult');
  await openAgent();
  const execution = await screen.findByRole('region', { name: 'Agent実行 execution-1' });
  expect(execution).toHaveTextContent('結果不明');
  expect(execution).toHaveTextContent('自動再実行しません');
  expect(request).not.toHaveBeenCalled(); expect(readResult).not.toHaveBeenCalled();
  expect(within(execution).queryByRole('button', { name: '実行を取消' })).not.toBeInTheDocument();
});

test('cancelling a running execution uses a distinct Human command and confirms current persisted state', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 2, canRequestAgent: false, agentExecutionIds: [syntheticExecution.id] } as never);
  const read = jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'running' } as never);
  const cancel = jest.spyOn(workApi, 'cancelAgentExecution').mockImplementation(async () => {
    read.mockResolvedValue({ ...syntheticExecution, status: 'cancelled', taskRevision: 3 } as never);
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [{ ...task, revision: 3 }], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 3, agentExecutionIds: [syntheticExecution.id] } as never);
    return { kind: 'agent_execution_cancelled', task: { ...task, revision: 3 }, execution: { ...syntheticExecution, status: 'cancelled', taskRevision: 3 } } as never;
  });
  await openAgent();
  await userEvent.click(await screen.findByRole('button', { name: '実行を取消' }));
  await waitFor(() => expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('取消済み'));
  expect(cancel.mock.calls[0]).toEqual([syntheticExecution.id, expect.objectContaining({ taskId: task.id, expectedRevision: 2, expectedAttemptId: task.attemptId, actingAssignmentId: session.actingAssignmentId })]);
});

test('denial of current Agent result removes private context rather than showing stale execution content', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, agentExecutionIds: [syntheticExecution.id] } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'succeeded', result: syntheticResult } as never);
  jest.spyOn(workApi, 'getAgentResult').mockRejectedValue(new WorkApiError(404, 'EVIDENCE_NOT_FOUND'));
  await openAgent();
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByText(syntheticResult.summary)).not.toBeInTheDocument();
  expect(screen.queryByLabelText('Agentへの依頼目的')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
});

test('a late Agent request response cannot restore a replaced attempt or its private state', async () => {
  const { client } = setup(); mockRecords();
  let resolve!: (value: never) => void;
  jest.spyOn(workApi, 'requestAgentExecution').mockImplementation(() => new Promise((done) => { resolve = done; }));
  const read = jest.spyOn(workApi, 'getAgentExecution');
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  const replacement = { ...task, attemptId: 'attempt-new', attemptNumber: 2, revision: 4 };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [replacement], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...replacement, workingArtifacts: [], agentExecutionIds: [] } as never);
  jest.mocked(workApi.listEvidence).mockResolvedValue({ items: [], nextCursor: null });
  jest.mocked(workApi.listFindings).mockResolvedValue({ items: [], nextCursor: null });
  await userEvent.click(screen.getByRole('button', { name: '再読込' }));
  await screen.findByText(/試行 2（attempt-new）/);
  await act(async () => { resolve({ kind: 'agent_execution_requested', task: { ...task, revision: 2 }, execution: syntheticExecution } as never); });
  expect(screen.queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
  expect(read).not.toHaveBeenCalled();
  expect(screen.getByLabelText('Agentへの依頼目的')).toHaveValue('');
  expect(client.getQueryData(['organization', session.principalId, session.actingAssignmentId, 'task', task.id, 'attempt-new'])).toMatchObject({ attemptId: 'attempt-new', revision: 4, agentExecutionIds: [] });
});

test('recovered cancellation for a different execution stays unknown instead of changing the current execution', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 2, canRequestAgent: false, agentExecutionIds: [syntheticExecution.id] } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'running' } as never);
  jest.spyOn(workApi, 'cancelAgentExecution').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  jest.spyOn(workApi, 'getOperation').mockResolvedValue({ kind: 'agent_execution_cancelled', task: { ...task, revision: 3 }, execution: { ...syntheticExecution, id: 'other-execution', status: 'cancelled' } } as never);
  await openAgent();
  await userEvent.click(await screen.findByRole('button', { name: '実行を取消' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  expect(await screen.findByText('応答と操作が一致しません。同じ操作IDで結果を確認してください。')).toBeVisible();
  expect(screen.getByRole('button', { name: '同じ操作の結果を確認' })).toBeEnabled();
  expect(screen.queryByRole('region', { name: 'Agent実行 other-execution' })).not.toBeInTheDocument();
});

test('current execution read denial drops previously displayed result and cached Agent data', async () => {
  const { client } = setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 3, agentExecutionIds: [syntheticExecution.id] } as never);
  const read = jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'succeeded', taskRevision: 3, result: syntheticResult } as never);
  jest.spyOn(workApi, 'getAgentResult').mockResolvedValue(syntheticResult as never);
  await openAgent();
  await screen.findByText(syntheticResult.summary);
  read.mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await userEvent.click(screen.getByRole('button', { name: '実行状態を再読込' }));
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByText(syntheticResult.summary)).not.toBeInTheDocument();
  expect(client.getQueryCache().findAll({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'agent-context'] })).toHaveLength(0);
});

test('failed execution refreshes the task request hint so an explicit new request is possible', async () => {
  setup(); mockRecords();
  jest.spyOn(workApi, 'requestAgentExecution').mockImplementation(async () => {
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [{ ...task, revision: 2 }], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 2, agentExecutionIds: [syntheticExecution.id] } as never);
    return { kind: 'agent_execution_requested', task: { ...task, revision: 2, canRequestAgent: false }, execution: syntheticExecution } as never;
  });
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'failed', failureCode: 'dependency_unavailable' } as never);
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  await screen.findByRole('region', { name: 'Agent実行 execution-1' });
  await waitFor(() => expect(screen.getByLabelText('Agentへの依頼目的')).toBeEnabled());
  expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('失敗');
  expect(workApi.requestAgentExecution).toHaveBeenCalledTimes(1);
});

test('a recovered nonterminal request stays uncertain with refresh and cancel available, without redispatch', async () => {
  setup(); mockRecords();
  const request = jest.spyOn(workApi, 'requestAgentExecution').mockRejectedValue(new WorkApiError(500, 'COMMIT_OUTCOME_UNKNOWN', true));
  const recover = jest.spyOn(workApi, 'getOperation').mockResolvedValue({ kind: 'agent_execution_requested', task: { ...task, revision: 2, canRequestAgent: false }, execution: syntheticExecution } as never);
  const read = jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue(syntheticExecution as never);
  const cancel = jest.spyOn(workApi, 'cancelAgentExecution').mockImplementation(async () => {
    const cancelled = { ...syntheticExecution, status: 'cancelled', taskRevision: 3 };
    read.mockResolvedValue(cancelled as never);
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [{ ...task, revision: 3 }], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 3, agentExecutionIds: [syntheticExecution.id] } as never);
    return { kind: 'agent_execution_cancelled', task: { ...task, revision: 3 }, execution: cancelled } as never;
  });
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByRole('region', { name: 'Agent実行 execution-1' });
  expect(screen.getByText(/結果は未確認です/)).toBeVisible();
  expect(screen.getByText(/実行の継続は未確認です/)).toBeVisible();
  expect(screen.getByRole('button', { name: '合成Agentに依頼' })).toBeDisabled();
  await waitFor(() => expect(screen.getByRole('button', { name: '実行状態を再読込' })).toBeEnabled());
  expect(screen.getByRole('button', { name: '実行を取消' })).toBeEnabled();
  read.mockResolvedValue({ ...syntheticExecution, status: 'running' } as never);
  await userEvent.click(screen.getByRole('button', { name: '実行状態を再読込' }));
  await waitFor(() => expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('実行中'));
  expect(screen.getByText(/結果は未確認です/)).toBeVisible();
  expect(screen.getByText(/実行の継続は未確認です/)).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: '実行を取消' }));
  await waitFor(() => expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('取消済み'));
  expect(screen.queryByText(/結果は未確認です/)).not.toBeInTheDocument();
  expect(screen.queryByText(/実行の継続は未確認です/)).not.toBeInTheDocument();
  expect(request).toHaveBeenCalledTimes(1);
  expect(recover).toHaveBeenCalledWith(request.mock.calls[0]?.[1].operationId);
  expect(cancel).toHaveBeenCalledTimes(1);
});

test('a fresh terminal read resolves recovered request uncertainty without interpreting its old queued receipt as progress', async () => {
  setup(); mockRecords();
  jest.spyOn(workApi, 'requestAgentExecution').mockRejectedValue(new WorkApiError(500, 'COMMIT_OUTCOME_UNKNOWN', true));
  jest.spyOn(workApi, 'getOperation').mockResolvedValue({ kind: 'agent_execution_requested', task: { ...task, revision: 2, canRequestAgent: false }, execution: syntheticExecution } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockResolvedValue({ ...syntheticExecution, status: 'outcome_unknown', failureCode: 'commit_outcome_unknown' } as never);
  const result = jest.spyOn(workApi, 'getAgentResult');
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await waitFor(() => expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('結果不明'));
  await waitFor(() => expect(screen.queryByText(/結果は未確認です/)).not.toBeInTheDocument());
  expect(screen.queryByText(/実行の継続は未確認です/)).not.toBeInTheDocument();
  expect(screen.getByText(/自動再実行しません/)).toBeVisible();
  expect(workApi.requestAgentExecution).toHaveBeenCalledTimes(1);
  expect(result).not.toHaveBeenCalled();
});

test('accepted execution survives a transient current-read context race without another mutation', async () => {
  setup(); mockRecords();
  const request = jest.spyOn(workApi, 'requestAgentExecution').mockImplementation(async () => {
    jest.mocked(workApi.listTasks).mockResolvedValue({ items: [{ ...task, revision: 3 }], nextCursor: null } as never);
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, revision: 3, agentExecutionIds: [syntheticExecution.id] } as never);
    return { kind: 'agent_execution_requested', task: { ...task, revision: 2, canRequestAgent: false }, execution: syntheticExecution } as never;
  });
  const read = jest.spyOn(workApi, 'getAgentExecution').mockRejectedValueOnce(new WorkApiError(409, 'WORK_CONTEXT_STALE')).mockResolvedValue({ ...syntheticExecution, status: 'succeeded', taskRevision: 3, result: syntheticResult } as never);
  jest.spyOn(workApi, 'getAgentResult').mockResolvedValue(syntheticResult as never);
  const cancel = jest.spyOn(workApi, 'cancelAgentExecution');
  const recover = jest.spyOn(workApi, 'getOperation');
  await fillAgent();
  await userEvent.click(screen.getByRole('button', { name: '合成Agentに依頼' }));
  await waitFor(() => expect(read).toHaveBeenCalled());
  expect(screen.queryByText(syntheticResult.summary)).not.toBeInTheDocument();
  expect(await screen.findByText(syntheticResult.summary)).toBeVisible();
  expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('実行状態：成功');
  expect(request).toHaveBeenCalledTimes(1);
  expect(cancel).not.toHaveBeenCalled(); expect(recover).not.toHaveBeenCalled();
});

test('current execution context-race retries stop after two additional reads', async () => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, agentExecutionIds: [syntheticExecution.id] } as never);
  const read = jest.spyOn(workApi, 'getAgentExecution').mockRejectedValue(new WorkApiError(409, 'WORK_CONTEXT_STALE'));
  const request = jest.spyOn(workApi, 'requestAgentExecution');
  await openAgent();
  await screen.findByText(/競合が発生しました/);
  expect(read).toHaveBeenCalledTimes(3);
  expect(screen.queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
  read.mockResolvedValue({ ...syntheticExecution, status: 'cancelled' } as never);
  await userEvent.click(screen.getByRole('button', { name: '実行状態を再読込' }));
  await waitFor(() => expect(screen.getByRole('region', { name: 'Agent実行 execution-1' })).toHaveTextContent('取消済み'));
  expect(read).toHaveBeenCalledTimes(4);
  expect(request).not.toHaveBeenCalled();
});

test.each([401, 403, 404])('current execution denial %s is never retried and clears private context', async (status) => {
  setup(); mockRecords();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, agentExecutionIds: [syntheticExecution.id] } as never);
  const read = jest.spyOn(workApi, 'getAgentExecution').mockRejectedValue(new WorkApiError(status, 'WORK_ITEM_NOT_FOUND'));
  await screen.findByRole('heading', { name: '内容確認' });
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  await screen.findByText(/内容を非表示にしました/);
  expect(read).toHaveBeenCalledTimes(1);
  expect(screen.queryByLabelText('Agentへの依頼目的')).not.toBeInTheDocument();
  expect(screen.queryByText(syntheticResult.summary)).not.toBeInTheDocument();
});

test('a pending read-only retry cannot reveal an old execution after task context changes', async () => {
  const { router, client } = setup(); mockRecords();
  const other = { ...task, id: 'task-other', attemptId: 'attempt-other' };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [task, other], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockImplementation(async (id) => ({ ...detail, ...(id === task.id ? task : other), workingArtifacts: id === task.id ? [artifact] : [], agentExecutionIds: id === task.id ? [syntheticExecution.id] : [] }) as never);
  let resolve!: (value: never) => void;
  const read = jest.spyOn(workApi, 'getAgentExecution').mockRejectedValueOnce(new WorkApiError(409, 'WORK_CONTEXT_STALE')).mockImplementation(() => new Promise((done) => { resolve = done; }));
  const readResult = jest.spyOn(workApi, 'getAgentResult');
  await openAgent();
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  jest.mocked(workApi.listEvidence).mockResolvedValue({ items: [], nextCursor: null });
  jest.mocked(workApi.listFindings).mockResolvedValue({ items: [], nextCursor: null });
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: other.id } } as never); });
  await screen.findByText(/タスク task-other/);
  await act(async () => { resolve({ ...syntheticExecution, status: 'succeeded', result: syntheticResult } as never); });
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  expect(await screen.findByLabelText('Agentへの依頼目的')).toHaveValue('');
  expect(screen.queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
  expect(screen.queryByText(syntheticResult.summary)).not.toBeInTheDocument();
  expect(readResult).not.toHaveBeenCalled();
  expect(client.getQueryCache().findAll({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'agent-context', task.id] })).toHaveLength(0);
});


const completableOffice = { ...officeTask, canComplete: true, completionActionId: 'office-complete-action' };
const completedOffice = { ...completableOffice, revision: 2, state: 'completed', canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, returnTransition: null, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false };
function setupCompletion() {
  const rendered = setupOffice();
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [completableOffice], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...completableOffice, workingArtifacts: [] } as never);
  return rendered;
}

test('office completion requires a cancellable confirmation with current task, attempt and responsibility', async () => {
  setupCompletion();
  const trigger = await screen.findByRole('button', { name: '完了内容を確認' });
  await userEvent.click(trigger);
  const dialog = screen.getByRole('dialog', { name: 'タスク完了の確認' });
  expect(dialog).toHaveTextContent(completableOffice.id);
  expect(dialog).toHaveTextContent(completableOffice.attemptId);
  expect(dialog).toHaveTextContent('assignment-office');
  expect(dialog).toHaveTextContent('完了後は読み取り専用');
  await waitFor(() => expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toHaveFocus());
  await userEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  await userEvent.click(trigger);
  await userEvent.keyboard('{Escape}');
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
});

function completionReply() {
  const result = { kind: 'completed', task: completedOffice };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [completedOffice], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...completedOffice, workingArtifacts: [], history: [{ kind: 'completed', occurredAt: '2026-10-04T17:00:00Z' }] } as never);
  return result as never;
}
async function confirmCompletion() {
  await userEvent.click(await screen.findByRole('button', { name: '完了内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: '完了を確定' }));
}

test('completion sends the exact server action and becomes read-only while preserving received content and current history', async () => {
  const { router } = setupCompletion();
  const complete = jest.spyOn(workApi, 'completeTask').mockImplementation(async () => completionReply());
  await confirmCompletion();
  await screen.findByText('タスクの完了が確定しました');
  expect(complete).toHaveBeenCalledTimes(1);
  expect(complete.mock.calls[0]).toEqual([completableOffice.id, { operationId: expect.any(String), expectedRevision: completableOffice.revision, actingAssignmentId: 'assignment-office', expectedAttemptId: completableOffice.attemptId, action: 'complete', definitionActionId: completableOffice.completionActionId }]);
  expect(router.state.location.search).toMatchObject({ taskId: completableOffice.id });
  expect(screen.queryByRole('button', { name: '完了内容を確認' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '差戻内容を確認' })).not.toBeInTheDocument();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  expect(screen.getByRole('region', { name: '受領したスナップショット' })).toHaveTextContent('提出する文案');
  await userEvent.click(screen.getByRole('button', { name: '履歴' }));
  expect(await screen.findByText('タスクを完了')).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  expect(await screen.findByLabelText('Agentへの依頼目的')).toBeDisabled();
});

test('real Playwright exact text finds the completed history label independently of its timestamp', async () => {
  const matches = require('./playwright-text-matcher.cjs')() as (element: Element, text: string) => boolean;
  setupCompletion();
  jest.spyOn(workApi, 'completeTask').mockImplementation(async () => completionReply());
  await confirmCompletion();
  await screen.findByText('タスクの完了が確定しました');
  await userEvent.click(screen.getByRole('button', { name: '履歴' }));
  const entry = (await screen.findByText('タスクを完了')).closest('li')!;
  const time = entry.querySelector('time')!;
  expect(time).toBeVisible();
  expect(time).toHaveAttribute('datetime', '2026-10-04T17:00:00Z');
  expect(time.textContent).not.toBe('');
  // Exact text must identify one visible label, not the combined label/time
  // entry or a substring. This is the same matcher used by the hosted journey.
  expect(matches(entry, 'タスクを完了')).toBe(false);
  const labels = Array.from(entry.querySelectorAll('*')).filter((element) => matches(element, 'タスクを完了'));
  expect(labels).toHaveLength(1);
  expect(labels[0]).toBeVisible();
  expect(matches(labels[0]!, '完了')).toBe(false);
});

test('completion Cancel and Escape never execute a write', async () => {
  setupCompletion();
  const complete = jest.spyOn(workApi, 'completeTask');
  for (const escape of [false, true]) {
    await userEvent.click(await screen.findByRole('button', { name: '完了内容を確認' }));
    if (escape) await userEvent.keyboard('{Escape}');
    else await userEvent.click(screen.getByRole('button', { name: 'キャンセル' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  }
  expect(complete).not.toHaveBeenCalled();
});

test.each([{ canComplete: false, completionActionId: 'office-complete-action' }, { canComplete: true, completionActionId: null }])('completion requires both server capability and action ID: %j', async (capability) => {
  setupCompletion();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...completableOffice, ...capability, workingArtifacts: [] } as never);
  await screen.findByLabelText('差戻理由');
  expect(screen.queryByRole('button', { name: '完了内容を確認' })).not.toBeInTheDocument();
});

test('unknown completion recovers the original operation without a second completion', async () => {
  setupCompletion();
  const complete = jest.spyOn(workApi, 'completeTask').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  const recovery = jest.spyOn(workApi, 'getOperation').mockImplementation(async () => completionReply());
  await confirmCompletion();
  await screen.findByText(/結果は未確認です/);
  expect(screen.getByRole('button', { name: '完了内容を確認' })).toBeDisabled();
  expect(screen.queryByText('タスクの完了が確定しました')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText('タスクの完了が確定しました');
  expect(recovery).toHaveBeenCalledWith(complete.mock.calls[0]?.[1].operationId);
  expect(complete).toHaveBeenCalledTimes(1);
});

test('unreceived completion retries the exact original operation after an inconclusive lookup', async () => {
  setupCompletion();
  const complete = jest.spyOn(workApi, 'completeTask').mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true)).mockImplementationOnce(async () => completionReply());
  jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await confirmCompletion();
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await userEvent.click(await screen.findByRole('button', { name: '同じ操作を再送' }));
  await screen.findByText('タスクの完了が確定しました');
  expect(complete.mock.calls[1]).toEqual(complete.mock.calls[0]);
});

test('completion conflict refreshes current authority without falsely claiming completion', async () => {
  setupCompletion();
  jest.spyOn(workApi, 'completeTask').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  await confirmCompletion();
  expect(await screen.findByRole('alert')).toHaveTextContent('競合が発生しました');
  expect(screen.queryByText('タスクの完了が確定しました')).not.toBeInTheDocument();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...completableOffice, revision: 3, canComplete: false, completionActionId: null, workingArtifacts: [] } as never);
  await userEvent.click(screen.getByRole('button', { name: '現在の状態を再読込' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: '完了内容を確認' })).not.toBeInTheDocument());
});

test.each([401, 403, 404])('completion denial %i clears private content and confirmation', async (status) => {
  setupCompletion();
  jest.spyOn(workApi, 'completeTask').mockRejectedValue(new WorkApiError(status, 'WORK_ITEM_NOT_FOUND'));
  await confirmCompletion();
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: '受領したスナップショット' })).not.toBeInTheDocument();
  expect(screen.queryByLabelText('差戻理由')).not.toBeInTheDocument();
  expect(screen.queryByRole('link', { name: '共有の入力文書' })).not.toBeInTheDocument();
});

test('an unresolved completion remains scoped to its original task across navigation', async () => {
  setupCompletion();
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [completableOffice, task], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockImplementation(async (id) => (id === task.id ? detail : { ...detail, ...completableOffice, workingArtifacts: [] }) as never);
  const complete = jest.spyOn(workApi, 'completeTask').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  await confirmCompletion();
  await screen.findByText(/結果は未確認です/);
  await userEvent.click(screen.getByRole('button', { name: /内容確認 作業中/ }));
  await screen.findByLabelText('作業中の文案');
  expect(screen.queryByText(/結果は未確認です/)).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /事務確認 作業中/ }));
  await screen.findByRole('button', { name: '同じ操作の結果を確認' });
  expect(screen.getByRole('button', { name: '完了内容を確認' })).toBeDisabled();
  expect(complete).toHaveBeenCalledTimes(1);
});


const holdableTask = { ...task, canHold: true, holdActionId: 'sales-hold-action' };
const heldTask = { ...holdableTask, revision: 2, state: 'held', canEdit: false, canSubmit: false, canHold: false, holdActionId: null, canResume: true, resumeActionId: 'sales-resume-action', canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false };
const resumedTask = { ...holdableTask, revision: 3 };
function setupHoldResume(current: typeof holdableTask | typeof heldTask = holdableTask) {
  const rendered = setup();
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [current], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current } as never);
  return rendered;
}
async function confirmHoldResume(action: 'hold' | 'resume') {
  await userEvent.click(await screen.findByRole('button', { name: action === 'hold' ? '保留内容を確認' : '再開内容を確認' }));
  await userEvent.click(screen.getByRole('button', { name: action === 'hold' ? '保留を確定' : '再開を確定' }));
}
function holdResumeReply(kind: 'held' | 'resumed') {
  const current = kind === 'held' ? heldTask : resumedTask;
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [current], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current, history: [{ kind: 'held', occurredAt: '2026-10-04T21:00:00Z' }, ...(kind === 'resumed' ? [{ kind: 'resumed', occurredAt: '2026-10-04T21:01:00Z' }] : [])] } as never);
  return { kind, task: current };
}
describe('hold/resume workflow', () => {
  const originalFetch = globalThis.fetch;
  let transport: jest.Mock;
  beforeEach(() => { transport = jest.fn(); globalThis.fetch = transport; });
  afterEach(() => { globalThis.fetch = originalFetch; });

  test('hold and resume show current identity in cancellable confirmations without a write', async () => {
    setupHoldResume();
    await userEvent.click(await screen.findByRole('button', { name: '保留内容を確認' }));
    const dialog = screen.getByRole('dialog', { name: '保留の確認' });
    expect(dialog).toHaveTextContent(task.id);
    expect(dialog).toHaveTextContent(task.attemptId);
    expect(dialog).toHaveTextContent(session.actingAssignmentId);
    expect(dialog).toHaveTextContent('未保存');
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'キャンセル' })).toHaveFocus());
    await userEvent.click(within(dialog).getByRole('button', { name: 'キャンセル' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: '保留内容を確認' }));
    await userEvent.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(transport).not.toHaveBeenCalled();
  });

  test('hold preserves saved private content and tab-only unsaved text, then explicit resume restores editing without Agent rerun', async () => {
    setupHoldResume();
    transport.mockImplementation(async (_url, init) => ({ ok: true, status: 200, json: async () => holdResumeReply(JSON.parse(init.body).action === 'hold' ? 'held' : 'resumed') }));
    const save = jest.spyOn(workApi, 'saveDraft');
    const submit = jest.spyOn(workApi, 'submit');
    const requestAgent = jest.spyOn(workApi, 'requestAgentExecution');
    fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: 'タブ内だけの未保存文案' } });
    await confirmHoldResume('hold');
    await screen.findByText('タスクを保留しました');
    expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: '保存済みの作業文案' })).toHaveTextContent(artifact.value.text);
    expect(screen.getByText(/未保存の入力はこのタブ内だけに保持しています/)).toBeVisible();
    expect(screen.queryByText('タブ内だけの未保存文案')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '文案を保存' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
    expect(await screen.findByLabelText('Agentへの依頼目的')).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: '履歴' }));
    expect(await screen.findByText('タスクを保留')).toBeVisible();
    await confirmHoldResume('resume');
    expect(await screen.findByLabelText('作業中の文案')).toHaveValue('タブ内だけの未保存文案');
    expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
    expect(await screen.findByText('タスクを再開')).toBeVisible();
    expect(transport.mock.calls.map(([url, init]) => [url, JSON.parse(init.body)])).toEqual([
      ['/v1/organization/tasks/task-1/actions', { operationId: expect.any(String), expectedRevision: 1, actingAssignmentId: session.actingAssignmentId, expectedAttemptId: task.attemptId, action: 'hold', definitionActionId: holdableTask.holdActionId }],
      ['/v1/organization/tasks/task-1/actions', { operationId: expect.any(String), expectedRevision: 2, actingAssignmentId: session.actingAssignmentId, expectedAttemptId: task.attemptId, action: 'resume', definitionActionId: heldTask.resumeActionId }],
    ]);
    expect(save).not.toHaveBeenCalled();
    expect(submit).not.toHaveBeenCalled();
    expect(requestAgent).not.toHaveBeenCalled();
  });
});

test('holding re-reads the revision-bound Agent execution and replaces queued cancellation with the terminal fence result', async () => {
  setupHoldResume();
  const queued = { ...syntheticExecution, taskRevision: task.revision };
  const fenced = { ...queued, status: 'failed', failureCode: 'context_stale', endedAt: '2026-10-04T21:00:00Z' };
  const matches = require('./playwright-text-matcher.cjs')() as (element: Element, text: string) => boolean;
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...holdableTask, agentExecutionIds: [queued.id] } as never);
  let held = false;
  let resolveCurrent!: (value: never) => void;
  const currentRead = new Promise<never>((resolve) => { resolveCurrent = resolve; });
  jest.spyOn(workApi, 'getAgentExecution').mockImplementation(async () => held ? currentRead : queued as never);
  const request = jest.spyOn(workApi, 'requestAgentExecution');
  jest.spyOn(workApi, 'holdTask').mockImplementation(async () => {
    held = true;
    const receipt = holdResumeReply('held');
    jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...heldTask, agentExecutionIds: [queued.id] } as never);
    return receipt as never;
  });
  await openAgent();
  expect(matches(await screen.findByRole('heading', { name: '実行状態：待機中' }), '実行状態：待機中')).toBe(true);
  expect(screen.getByRole('button', { name: '実行を取消' })).toBeEnabled();
  await confirmHoldResume('hold');
  await screen.findByText('タスクを保留しました');
  // A new revision has no placeholder execution. The old queued read cannot keep its cancel action visible.
  await screen.findByText('現在の実行状態を確認中…');
  expect(screen.queryByRole('heading', { name: '実行状態：待機中' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '実行を取消' })).not.toBeInTheDocument();
  await act(async () => { resolveCurrent(fenced as never); });
  const terminal = await screen.findByRole('heading', { name: '実行状態：失敗' });
  expect(matches(terminal, '実行状態：失敗')).toBe(true);
  expect(screen.getByLabelText('Agentへの依頼目的')).toBeDisabled();
  expect(screen.queryByRole('button', { name: '実行を取消' })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole('button', { name: '実行状態を再読込' })).toBeEnabled());
  expect(request).not.toHaveBeenCalled();
});

const holdResumeCases = [
  { action: 'hold' as const, current: holdableTask, kind: 'held' as const, trigger: '保留内容を確認', notice: 'タスクを保留しました' },
  { action: 'resume' as const, current: heldTask, kind: 'resumed' as const, trigger: '再開内容を確認', notice: 'タスクを再開しました' },
];

test.each(holdResumeCases)('$action cancellation and Escape preserve local input without a transition', async ({ action, current, trigger }) => {
  setupHoldResume(current);
  const execute = jest.spyOn(workApi, action === 'hold' ? 'holdTask' : 'resumeTask');
  for (const escape of [false, true]) {
    await userEvent.click(await screen.findByRole('button', { name: trigger }));
    if (escape) await userEvent.keyboard('{Escape}');
    else await userEvent.click(screen.getByRole('button', { name: 'キャンセル' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  }
  expect(execute).not.toHaveBeenCalled();
});

test.each([
  { current: { ...holdableTask, canHold: false }, trigger: '保留内容を確認' },
  { current: { ...holdableTask, holdActionId: null }, trigger: '保留内容を確認' },
  { current: { ...heldTask, canResume: false }, trigger: '再開内容を確認' },
  { current: { ...heldTask, resumeActionId: null }, trigger: '再開内容を確認' },
])('hold/resume requires both server capability and action ID, independent of the state label: $trigger $current', async ({ current, trigger }) => {
  setupHoldResume(current as typeof holdableTask);
  await screen.findByRole('link', { name: '共有の入力文書' });
  expect(screen.queryByRole('button', { name: trigger })).not.toBeInTheDocument();
});

test.each(holdResumeCases)('unknown $action recovers the same operation without replaying or discarding local input', async ({ action, current, kind, trigger, notice }) => {
  setupHoldResume(current);
  const execute = jest.spyOn(workApi, action === 'hold' ? 'holdTask' : 'resumeTask').mockRejectedValue(new WorkApiError(0, 'network_unavailable', true));
  const recovery = jest.spyOn(workApi, 'getOperation').mockImplementation(async () => holdResumeReply(kind) as never);
  if (action === 'hold') fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '回復中も保持する文案' } });
  await confirmHoldResume(action);
  await screen.findByText(/結果は未確認です/);
  expect(screen.queryByText(notice)).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: trigger })).toBeDisabled();
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await screen.findByText(notice);
  expect(recovery).toHaveBeenCalledWith(execute.mock.calls[0]?.[1].operationId);
  expect(execute).toHaveBeenCalledTimes(1);
  if (action === 'hold') {
    jest.spyOn(workApi, 'resumeTask').mockImplementation(async () => holdResumeReply('resumed') as never);
    await confirmHoldResume('resume');
    expect(await screen.findByLabelText('作業中の文案')).toHaveValue('回復中も保持する文案');
  }
});

test.each(holdResumeCases)('unreceived $action retries the identical command after an inconclusive operation lookup', async ({ action, current, kind, notice }) => {
  setupHoldResume(current);
  const execute = jest.spyOn(workApi, action === 'hold' ? 'holdTask' : 'resumeTask').mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true)).mockImplementationOnce(async () => holdResumeReply(kind) as never);
  jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await confirmHoldResume(action);
  await userEvent.click(await screen.findByRole('button', { name: '同じ操作の結果を確認' }));
  await userEvent.click(await screen.findByRole('button', { name: '同じ操作を再送' }));
  await screen.findByText(notice);
  expect(execute.mock.calls[1]).toEqual(execute.mock.calls[0]);
});

test.each(holdResumeCases)('$action conflict keeps input and requires refreshed authority without claiming success', async ({ action, current, trigger, notice }) => {
  setupHoldResume(current);
  jest.spyOn(workApi, action === 'hold' ? 'holdTask' : 'resumeTask').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  if (action === 'hold') fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '競合後も未保存' } });
  await confirmHoldResume(action);
  expect(await screen.findByRole('alert')).toHaveTextContent('競合が発生しました');
  expect(screen.queryByText(notice)).not.toBeInTheDocument();
  if (action === 'hold') expect(screen.getByLabelText('作業中の文案')).toHaveValue('競合後も未保存');
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, ...current, revision: 4, canHold: false, holdActionId: null, canResume: false, resumeActionId: null } as never);
  await userEvent.click(screen.getByRole('button', { name: '現在の状態を再読込' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: trigger })).not.toBeInTheDocument());
});

test.each([401, 403, 404])('hold denial %i hides the saved private text, local draft and confirmation', async (status) => {
  setupHoldResume();
  jest.spyOn(workApi, 'holdTask').mockRejectedValue(new WorkApiError(status, 'WORK_ITEM_NOT_FOUND'));
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '権限喪失時に消す文案' } });
  await confirmHoldResume('hold');
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(screen.queryByText('権限喪失時に消す文案')).not.toBeInTheDocument();
  expect(screen.queryByText(artifact.value.text)).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '再開内容を確認' })).not.toBeInTheDocument();
});

test('an unresolved hold and late receipt stay isolated from the newly selected task', async () => {
  const { router } = setupHoldResume();
  const other = { ...task, id: 'other-task', attemptId: 'other-attempt', title: '別のタスク' };
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [holdableTask, other], nextCursor: null } as never);
  jest.mocked(workApi.getTask).mockImplementation(async (id) => ({ ...detail, ...(id === task.id ? holdableTask : other), workingArtifacts: id === task.id ? [artifact] : [] }) as never);
  let resolve!: (value: never) => void;
  const hold = jest.spyOn(workApi, 'holdTask').mockImplementation(() => new Promise((done) => { resolve = done; }));
  fireEvent.change(await screen.findByLabelText('作業中の文案'), { target: { value: '元タスクの未保存文案' } });
  await confirmHoldResume('hold');
  await screen.findByText(/処理中です/);
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: other.id } } as never); });
  expect(await screen.findByLabelText('作業中の文案')).toHaveValue('');
  await act(async () => { resolve({ kind: 'held', task: heldTask } as never); });
  expect(screen.queryByText('タスクを保留しました')).not.toBeInTheDocument();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('');
  await act(async () => { await router.navigate({ to: '/tasks', search: { view: 'context', taskId: task.id } } as never); });
  await screen.findByRole('button', { name: '同じ操作の結果を確認' });
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('元タスクの未保存文案');
  expect(screen.getByRole('button', { name: '保留内容を確認' })).toBeDisabled();
  expect(hold).toHaveBeenCalledTimes(1);
});

test('holding after clearing a candidate does not claim there is unsaved input', async () => {
  setupHoldResume();
  jest.spyOn(workApi, 'holdTask').mockImplementation(async () => holdResumeReply('held') as never);
  await screen.findByLabelText('作業中の文案');
  await userEvent.click(screen.getByRole('button', { name: '根拠' }));
  fireEvent.change(await screen.findByLabelText('候補の主張'), { target: { value: '書きかけ' } });
  fireEvent.change(screen.getByLabelText('候補の主張'), { target: { value: '' } });
  await confirmHoldResume('hold');
  await screen.findByText('タスクを保留しました');
  expect(screen.queryByText(/未保存の入力はこのタブ内だけに保持しています/)).not.toBeInTheDocument();
});
