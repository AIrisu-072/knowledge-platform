import { TextEncoder } from 'node:util';
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { TaskHomePage } from '../src/routes/TaskHomePage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(), getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn() } }));

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', responsibilities: undefined, capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: true, return: true } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '営業内容整理', stepLabel: '営業内容整理', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null, requiredRoleId: null, claimAssignmentId: null, canAssign: false, assignment: null, workTypeId: 'type-sales', workTypeLabel: '営業内容整理', dueAt: null, attention: [], contextTitle: null };
const memo = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 0, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' };
const generation = { id: 'generation-1', sizeBytes: 6, sha256: 'a'.repeat(64), storedAt: '2026-10-07T09:00:00Z', providerId: 'organization.work-artifacts' };
const fileRecord = (extra: Record<string, unknown> = {}) => ({ id: 'file-1', taskId: task.id, attemptId: task.attemptId, revision: 0, schemaId: 'organization.work-file.v1', visibility: 'work_item_private', file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation: null }, ...extra });
const detail = (workingArtifacts: unknown[], extra: Record<string, unknown> = {}) => ({ ...task, ...extra, inputResources: [], workingArtifacts, history: [], agentExecutionIds: [] });

function setup(taskDetail: unknown, summary: Record<string, unknown> = task, actor: unknown = session) {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(actor as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: [summary], nextCursor: null } as never);
  jest.spyOn(workApi, 'listWorkContexts').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listWorkViewProfiles').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'getTask').mockResolvedValue(taskDetail as never);
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const router = createRouter({ routeTree: root.addChildren([route]), history: createMemoryHistory({ initialEntries: [`/tasks?view=context&taskId=${(summary as { id: string }).id}`] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
}
afterEach(() => jest.restoreAllMocks());

test('a chosen file is recorded, saved to the work store and submitted with the draft', async () => {
  setup(detail([memo]));
  const created = jest.spyOn(workApi, 'createFileArtifact').mockResolvedValue({ kind: 'artifact_created', task: { ...task, revision: 2 }, artifact: fileRecord() } as never);
  const written = jest.spyOn(workApi, 'writeArtifactContent').mockResolvedValue({ kind: 'artifact_content_written', task: { ...task, revision: 3 }, artifact: fileRecord({ revision: 1, file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation } }) } as never);
  const files = await screen.findByRole('region', { name: '作業ファイル' });
  expect(within(files).getByText(/端末上の場所（パス）は送信しません/)).toBeVisible();
  const chosen = new File(['合成データ'], '合成_見積.txt', { type: 'text/plain' });
  await userEvent.upload(within(files).getByLabelText('作業ファイルを追加'), chosen);
  await waitFor(() => expect(written).toHaveBeenCalled());
  expect(created).toHaveBeenCalledWith(task.id, { operationId: expect.any(String), expectedRevision: 1, actingAssignmentId: 'assignment-sales', file: { fileName: '合成_見積.txt', mediaType: 'text/plain' } });
  // Content follows the new record with the receipt's revisions, under its own operation.
  const [artifactId, command, content] = written.mock.calls[0]!;
  expect(artifactId).toBe('file-1');
  expect(command).toEqual({ operationId: expect.any(String), expectedRevision: 2, actingAssignmentId: 'assignment-sales', expectedArtifactRevision: 0 });
  expect(command.operationId).not.toBe(created.mock.calls[0]![1].operationId);
  expect(content).toBe(chosen);
  expect(await screen.findByText(/作業用保存領域へ保存しました/)).toBeVisible();
  const list = within(files).getByRole('list', { name: '作業ファイルの一覧' });
  expect(list).toHaveTextContent('合成_見積.txt');
  expect(list).toHaveTextContent('6 B · 保存済み');
  const submit = jest.spyOn(workApi, 'submit').mockRejectedValue(new WorkApiError(409, 'REVISION_CONFLICT'));
  await userEvent.click(screen.getByRole('button', { name: '提出内容を確認' }));
  const dialog = screen.getByRole('dialog', { name: '提出の確認' });
  expect(within(dialog).getByRole('list', { name: '提出するファイル' })).toHaveTextContent('合成_見積.txt · 6 B');
  await userEvent.click(within(dialog).getByRole('button', { name: '提出を確定' }));
  await waitFor(() => expect(submit).toHaveBeenCalled());
  expect(submit.mock.calls[0]![1].artifacts).toEqual([{ artifactId: memo.id, revision: 0 }, { artifactId: 'file-1', revision: 1 }]);
});

test('empty, oversized or path-like files are refused before any request', async () => {
  setup(detail([memo]));
  const created = jest.spyOn(workApi, 'createFileArtifact');
  const input = await screen.findByLabelText('作業ファイルを追加');
  await userEvent.upload(input, new File([], '空.txt'));
  expect(await screen.findByText('空のファイルは添付できません。')).toBeVisible();
  await userEvent.upload(input, new File([new Uint8Array(8 * 1024 * 1024 + 1)], '大きい.bin'));
  expect(await screen.findByText('ファイルは8MiB以内にしてください。')).toBeVisible();
  await userEvent.upload(input, new File(['x'], 'a\nb.txt'));
  expect(await screen.findByText(/ファイル名に使えない文字/)).toBeVisible();
  await userEvent.upload(input, new File(['x'], '請求書\u202Efdp.exe'));
  expect(await screen.findByText(/ファイル名に使えない文字/)).toBeVisible();
  expect(created).not.toHaveBeenCalled();
});

test('a record removed meanwhile is stale state: the task refreshes and is never hidden as denied', async () => {
  setup(detail([memo, fileRecord({ revision: 1, file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation } })]));
  const files = await screen.findByRole('region', { name: '作業ファイル' });
  const discarded = jest.spyOn(workApi, 'discardArtifact').mockRejectedValue(new WorkApiError(404, 'WORK_ARTIFACT_NOT_FOUND'));
  const reads = jest.mocked(workApi.getTask).mock.calls.length;
  jest.mocked(workApi.getTask).mockResolvedValue(detail([memo]) as never);
  await userEvent.click(within(files).getByRole('button', { name: '合成_見積.txt を外す' }));
  await waitFor(() => expect(discarded).toHaveBeenCalled());
  expect(await screen.findByText(/既に外されたか変更されています/)).toBeVisible();
  await waitFor(() => expect(jest.mocked(workApi.getTask).mock.calls.length).toBeGreaterThan(reads));
  expect(screen.queryByText(/現在の担当では利用できません/)).not.toBeInTheDocument();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('保存済みの文案');
});

test('without a composed work store, files can be listed and removed but not added', async () => {
  setup(detail([memo, fileRecord({ revision: 2 })]), task, { ...session, capabilities: { ...session.capabilities, fileUpload: false } });
  const files = await screen.findByRole('region', { name: '作業ファイル' });
  expect(within(files).getByText('このサーバーでは作業ファイルを保存できません。')).toBeVisible();
  expect(within(files).queryByLabelText('作業ファイルを追加')).not.toBeInTheDocument();
  expect(within(files).queryByLabelText('合成_見積.txt の内容を登録')).not.toBeInTheDocument();
  expect(within(files).getByRole('button', { name: '合成_見積.txt を外す' })).toBeEnabled();
});

test('a file without content blocks submit; it is registered or discarded explicitly', async () => {
  setup(detail([memo, fileRecord({ revision: 2 })]));
  const files = await screen.findByRole('region', { name: '作業ファイル' });
  expect(await screen.findByText(/内容が未登録のファイルがあります/)).toBeVisible();
  expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeDisabled();
  expect(within(files).getByRole('list', { name: '作業ファイルの一覧' })).toHaveTextContent('内容未登録');
  const written = jest.spyOn(workApi, 'writeArtifactContent').mockRejectedValue(new WorkApiError(503, 'WORK_ARTIFACT_UNAVAILABLE', true));
  const chosen = new File(['合成'], '別名.txt');
  await userEvent.upload(within(files).getByLabelText('合成_見積.txt の内容を登録'), chosen);
  await waitFor(() => expect(written).toHaveBeenCalledWith('file-1', { operationId: expect.any(String), expectedRevision: 1, actingAssignmentId: 'assignment-sales', expectedArtifactRevision: 2 }, chosen));
  // A 503 on the write is resolved by the same operation ID, never claimed as saved or not.
  expect(await screen.findByText(/結果は未確認です/)).toBeVisible();
  const recovered = jest.spyOn(workApi, 'getOperation').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await userEvent.click(screen.getByRole('button', { name: '同じ操作の結果を確認' }));
  await waitFor(() => expect(recovered).toHaveBeenCalledWith(written.mock.calls[0]![1].operationId));
  written.mockResolvedValue({ kind: 'artifact_content_written', task: { ...task, revision: 2 }, artifact: fileRecord({ revision: 3, file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation } }) } as never);
  await userEvent.click(await screen.findByRole('button', { name: '同じ操作を再送' }));
  await waitFor(() => expect(written).toHaveBeenCalledTimes(2));
  expect(written.mock.calls[1]![1].operationId).toBe(written.mock.calls[0]![1].operationId);
  expect(written.mock.calls[1]![2]).toBe(chosen);
  expect(await screen.findByText(/作業用保存領域へ保存しました/)).toBeVisible();
  const discarded = jest.spyOn(workApi, 'discardArtifact').mockResolvedValue({ kind: 'artifact_discarded', task: { ...task, revision: 2 }, artifactId: 'file-1' } as never);
  await userEvent.click(within(files).getByRole('button', { name: '合成_見積.txt を外す' }));
  await waitFor(() => expect(discarded).toHaveBeenCalledWith('file-1', { operationId: expect.any(String), expectedRevision: 2, actingAssignmentId: 'assignment-sales', expectedArtifactRevision: 3 }));
  expect(await screen.findByText(/保存済みの内容は削除していません/)).toBeVisible();
  await waitFor(() => expect(screen.getByRole('button', { name: '提出内容を確認' })).toBeEnabled());
});

test('a received submission file downloads as an attachment through the Work API', async () => {
  const office = { ...task, id: 'office-1', attemptId: 'office-attempt', title: '事務内容確認', stepLabel: '事務内容確認', canEdit: false, canSubmit: false, handoffSnapshotId: 'snapshot-1' };
  const snapshot = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: office.id, createdAt: '2026-10-07T09:00:00Z', evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [], artifacts: [{ artifactId: 'file-1', revision: 1, schemaId: 'organization.work-file.v1', file: { fileName: '合成_見積.txt', mediaType: 'text/html', generation } }] };
  jest.spyOn(workApi, 'getSnapshot').mockResolvedValue(snapshot as never);
  const blob = new Blob(['合成']);
  const read = jest.spyOn(workApi, 'readSnapshotContent').mockResolvedValue(blob);
  const create = jest.fn(() => 'blob:synthetic'); const revoke = jest.fn();
  Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke });
  const click = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  setup({ ...detail([], office), id: office.id }, office);
  const received = await screen.findByRole('region', { name: '受領したスナップショット' });
  expect(received).toHaveTextContent('合成_見積.txt · 6 B');
  await userEvent.click(within(received).getByRole('button', { name: '合成_見積.txt を取得' }));
  await waitFor(() => expect(click).toHaveBeenCalled());
  expect(read).toHaveBeenCalledWith('snapshot-1', 'file-1', generation, expect.any(AbortSignal));
  expect(create).toHaveBeenCalledWith(blob);
  expect((click.mock.contexts[0] as HTMLAnchorElement).download).toBe('合成_見積.txt');
  // The content itself is never rendered into the page.
  expect(received).not.toHaveTextContent('合成データ');
});

test('a returned attempt imports the prior submission only by an explicit action', async () => {
  const returned = { ...task, attemptNumber: 2, attemptId: 'attempt-2', returnInstructionId: 'instruction-1', handoffSnapshotId: 'snapshot-1' };
  const prior = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: 'office-1', createdAt: '2026-10-07T09:00:00Z', evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [], artifacts: [{ artifactId: memo.id, revision: 0, schemaId: memo.schemaId, value: { text: '前回の文案' } }, { artifactId: 'file-1', revision: 1, schemaId: 'organization.work-file.v1', file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation } }] };
  jest.spyOn(workApi, 'getSnapshot').mockResolvedValue(prior as never);
  jest.spyOn(workApi, 'getReturnInstruction').mockResolvedValue({ id: 'instruction-1', workflowId: 'w', contextId: 'context-1', sourceTaskId: 'office-1', sourceAttemptId: 'office-attempt', targetTaskId: task.id, targetAttemptId: 'attempt-2', previousSubmissionId: 'snapshot-1', transitionId: 't', reason: '合成の差戻理由', returnedBy: 'office-01', actingAssignmentId: 'a', createdAt: '2026-10-07T09:10:00Z' } as never);
  setup(detail([], returned), returned);
  const section = await screen.findByRole('region', { name: '差戻し後の作業' });
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('');
  const derived = (artifact: Record<string, unknown>, original: string) => ({ ...artifact, attemptId: 'attempt-2', derivedFrom: { snapshotId: 'snapshot-1', artifactId: original } });
  const imported = jest.spyOn(workApi, 'importSubmission').mockResolvedValue({ kind: 'submission_imported', task: { ...returned, revision: 2 }, artifacts: [derived({ ...memo, id: 'draft-2', value: { text: '前回の文案' } }, memo.id), derived(fileRecord({ id: 'file-2', file: { fileName: '合成_見積.txt', mediaType: 'text/plain', generation } }), 'file-1')] } as never);
  await userEvent.click(within(section).getByRole('button', { name: '前回の提出内容を取り込む' }));
  await waitFor(() => expect(imported).toHaveBeenCalledWith(task.id, { operationId: expect.any(String), expectedRevision: 1, actingAssignmentId: 'assignment-sales', expectedAttemptId: 'attempt-2', snapshotId: 'snapshot-1' }));
  expect(await screen.findByText(/前回の提出は変更されません/)).toBeVisible();
  await waitFor(() => expect(screen.getByLabelText('作業中の文案')).toHaveValue('前回の文案'));
  expect(screen.getByRole('list', { name: '作業ファイルの一覧' })).toHaveTextContent('前回の提出から取込み');
  expect(screen.queryByRole('region', { name: '差戻し後の作業' })).not.toBeInTheDocument();
});
