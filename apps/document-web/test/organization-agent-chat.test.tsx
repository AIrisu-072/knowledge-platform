import { TextEncoder } from 'node:util';
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { documentApi } from '../src/application/document-workspace';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { AppShell } from '../src/components/app-shell/AppShell';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';
import { TaskHomePage } from '../src/routes/TaskHomePage';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(), getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn() } }));

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', capabilities: { nativeWorkspace: false, agent: true, search: false, fileUpload: false, return: false } };
const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null, requiredRoleId: null, claimAssignmentId: null, canAssign: false, assignment: null, workTypeId: 'work-type-1', workTypeLabel: '内容確認', dueAt: null, attention: [], contextTitle: null };
const artifact = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '保存済みの文案' }, visibility: 'work_item_private' };
const detail = { ...task, revision: 3, inputResources: [{ kind: 'document', documentId: '00000000-0000-4000-8000-000000000010', label: '共有の入力文書' }], workingArtifacts: [artifact], history: [], agentExecutionIds: ['execution-1'] };
const nextTask = { ...task, id: 'task-2', attemptId: 'attempt-2', title: '事務確認', stepLabel: '事務確認', state: 'ready', canClaim: true, canEdit: false, canSubmit: false };
const snapshot = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: nextTask.id, createdAt: '2026-10-04T05:00:00Z', evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [], artifacts: [] };

function setup(entry = '/tasks?view=context&taskId=task-1') {
  jest.mocked(documentApi.getRootFolder).mockResolvedValue({ folderId: '00000000-0000-7000-8000-000000000001', name: 'ルート', revision: 1, parentFolderId: null, capabilities: {} } as never);
  jest.mocked(documentApi.listFolderChildren).mockResolvedValue({ items: [], nextCursor: null, capabilities: {} } as never);
  jest.mocked(documentApi.listDocuments).mockImplementation(async query => ({ view: query.view, items: [], nextCursor: null } as never));
  jest.mocked(documentApi.getDocument).mockResolvedValue({ documentId: detail.inputResources[0]!.documentId, displayRevision: null } as never);
  jest.spyOn(workApi, 'getSession').mockResolvedValue(session as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: [task], nextCursor: null } as never);
  jest.spyOn(workApi, 'listWorkContexts').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listWorkViewProfiles').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'getTask').mockResolvedValue(detail as never);
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'getSnapshot').mockResolvedValue(snapshot as never);
  const root = createRootRoute({ component: Outlet });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const document = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: () => <AppShell><h1>入力文書</h1></AppShell> });
  const documents = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
  const router = createRouter({ routeTree: root.addChildren([tasks, document, documents]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { router, client };
}

afterEach(() => jest.restoreAllMocks());

const evidenceRecord = { id: 'evidence-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, sourceRef: { providerId: 'document', resourceId: detail.inputResources[0]!.documentId, revisionId: 'revision-1', versionId: 'version-1' }, authoritativeLocator: { kind: 'contentItem', contentItemId: 'content-1', representationId: 'representation-1' }, relevantLocation: '第1節', origin: 'human', fragmentOmissionReason: 'not_retained', coverage: 'unknown', relevantLocationVerified: false, policyDisposition: 'reference_only', uncertainty: [], conflictReferences: [], createdBy: 'sales-01', actingAssignmentId: 'assignment-sales', recordedAt: '2026-10-04T08:00:00Z', retrievedAt: '2026-10-04T08:00:00Z', providerCheckedAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private' };
const syntheticExecution = { id: 'execution-1', contextId: task.contextId, workItemId: task.id, attemptId: task.attemptId, requestedBy: session.principalId, requesterResponsibility: session.actingAssignmentId, executedBy: 'organization-synthetic/agent-01', executorInvocationKind: 'agent', providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' }], effectiveContextRevision: 2, taskRevision: 2, purpose: '参照を確認', evidenceRevisionRefs: [{ id: 'evidence-1', revision: 1 }], status: 'queued', startedAt: '2026-10-04T13:00:00Z', endedAt: null, result: null, failureCode: null };
const evidence2 = { ...evidenceRecord, id: 'evidence-2', relevantLocation: '第2節' };
const refs = [{ id: 'evidence-1', revision: 1 }];
const structured = { summary: '合成の構造化結果', findingRevisionRefs: [{ id: 'synthetic-finding', revision: 1 }], evidenceRevisionRefs: refs, uncertainty: ['原本本文は未検証'], simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false, sourceOutcomes: [{ evidenceRevisionRef: refs[0], outcome: 'referenced' }], generatedArtifactIds: ['generated-1'], suggestedActionIds: ['suggested-1', 'suggested-2'] };
const succeeded = { ...syntheticExecution, status: 'succeeded', taskRevision: 3, endedAt: '2026-10-07T09:00:01Z', result: structured };
const generated = { id: 'generated-1', executionId: 'execution-1', contextId: task.contextId, workItemId: task.id, attemptId: task.attemptId, schemaId: 'organization.text-draft.v1', title: '確認メモの下書き（合成）', value: { text: '【合成Agentの下書き】確認メモ' }, sourceRevisionRefs: refs, author: 'organization-synthetic/agent-01', simulated: true, visibility: 'agent_execution_private', createdAt: '2026-10-07T09:00:01Z' };
const review = { id: 'suggested-1', executionId: 'execution-1', contextId: task.contextId, workItemId: task.id, attemptId: task.attemptId, action: { kind: 'review_finding', findingRevisionRef: { id: 'synthetic-finding', revision: 1 } }, rationale: '候補の根拠を原本で確認してください', supportingRevisionRefs: refs, author: 'organization-synthetic/agent-01', visibility: 'agent_execution_private', createdAt: '2026-10-07T09:00:01Z' };
const adopt = { ...review, id: 'suggested-2', action: { kind: 'use_generated_artifact', generatedArtifactId: 'generated-1' }, rationale: '下書きを作業文案に使えます' };

function mockAgent(result: object = structured, execution: object = succeeded) {
  jest.mocked(workApi.listEvidence).mockResolvedValue({ items: [evidenceRecord, evidence2], nextCursor: null } as never);
  jest.spyOn(workApi, 'getAgentExecution').mockImplementation(async (id) => ({ ...execution, id, result: (execution as { status: string }).status === 'succeeded' ? result : null }) as never);
  jest.spyOn(workApi, 'getAgentResult').mockResolvedValue(result as never);
  jest.spyOn(workApi, 'getGeneratedArtifact').mockResolvedValue(generated as never);
  jest.spyOn(workApi, 'getSuggestedAction').mockImplementation(async (id) => (id === review.id ? review : adopt) as never);
}
async function openAgent() {
  await screen.findByRole('heading', { name: '内容確認' });
  await userEvent.click(screen.getByRole('button', { name: 'Agent' }));
  return screen.findByRole('region', { name: '合成Agent' });
}

test('structured result shows per-source use, a private draft candidate and typed proposals; using the candidate only fills the unsaved draft', async () => {
  setup(); mockAgent();
  const save = jest.spyOn(workApi, 'saveDraft');
  const module = await openAgent();
  expect(module).toHaveTextContent('Agent Chat');
  const turn = await within(module).findByRole('region', { name: 'Agent実行 execution-1' });
  await within(turn).findByText('合成の構造化結果');
  expect(within(turn).getByRole('heading', { name: '依頼 1' })).toBeVisible();
  expect(within(turn).getByRole('list', { name: '根拠ごとの利用結果' })).toHaveTextContent('根拠 evidence-1（版 1）：参照情報だけを使用');
  expect(within(turn).queryByText(/一部の根拠を利用できませんでした/)).not.toBeInTheDocument();
  const candidate = await within(turn).findByRole('article', { name: '下書き候補 generated-1' });
  expect(candidate).toHaveTextContent('確認メモの下書き（合成）');
  expect(candidate).toHaveTextContent('未保存・非公開');
  expect(candidate).toHaveTextContent('【合成Agentの下書き】確認メモ');
  const proposals = within(turn).getByRole('region', { name: 'Agentの提案' });
  expect(await within(proposals).findByRole('listitem', { name: '提案 suggested-1' })).toHaveTextContent('候補 synthetic-finding を確認し、人間判断を記録する');
  expect(await within(proposals).findByRole('listitem', { name: '提案 suggested-2' })).toHaveTextContent('下書き候補を作業文案に使う');
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('保存済みの文案');
  await userEvent.click(within(candidate).getByRole('button', { name: '作業文案に入れる' }));
  // Re-read under current authorization before use; nothing is saved automatically.
  await waitFor(() => expect(screen.getByLabelText('作業中の文案')).toHaveValue('【合成Agentの下書き】確認メモ'));
  expect(workApi.getGeneratedArtifact).toHaveBeenCalledTimes(2);
  expect(await screen.findByText(/作業中の文案に入れました（未保存）/)).toBeVisible();
  expect(save).not.toHaveBeenCalled();
  // Unsaved draft changes are never overwritten by a candidate.
  fireEvent.change(screen.getByLabelText('作業中の文案'), { target: { value: '担当者の編集' } });
  expect(within(candidate).getByRole('button', { name: '作業文案に入れる' })).toBeDisabled();
  expect(candidate).toHaveTextContent('保存していない文案の変更があります');
  expect(within(within(proposals).getByRole('listitem', { name: '提案 suggested-2' })).getByRole('button', { name: '提案を開く' })).toBeDisabled();
  expect(screen.getByLabelText('作業中の文案')).toHaveValue('担当者の編集');
});

test('choosing a review proposal re-reads it and opens the Evidence module without recording a Human decision', async () => {
  setup(); mockAgent();
  const decide = jest.spyOn(workApi, 'recordDecision');
  const module = await openAgent();
  const item = await within(module).findByRole('listitem', { name: '提案 suggested-1' });
  await userEvent.click(within(item).getByRole('button', { name: '提案を開く' }));
  expect(await screen.findByRole('region', { name: '根拠・候補・人間判断' })).toBeVisible();
  expect(workApi.getSuggestedAction).toHaveBeenCalledWith('suggested-1');
  expect(jest.mocked(workApi.getSuggestedAction).mock.calls.filter(([id]) => id === 'suggested-1')).toHaveLength(2);
  expect(await screen.findByText(/候補 synthetic-finding を根拠・判断モジュールで確認し、人間判断を記録してください。提案は実行されていません。/)).toBeVisible();
  expect(decide).not.toHaveBeenCalled();
});

test('the use proposal re-reads its candidate and a proposal whose target left the result is refused', async () => {
  setup(); mockAgent();
  const module = await openAgent();
  const use = await within(module).findByRole('listitem', { name: '提案 suggested-2' });
  await userEvent.click(within(use).getByRole('button', { name: '提案を開く' }));
  await waitFor(() => expect(screen.getByLabelText('作業中の文案')).toHaveValue('【合成Agentの下書き】確認メモ'));
  const item = await within(module).findByRole('listitem', { name: '提案 suggested-1' });
  jest.mocked(workApi.getSuggestedAction).mockResolvedValue({ ...review, action: { kind: 'review_finding', findingRevisionRef: { id: 'other-finding', revision: 1 } } } as never);
  await userEvent.click(within(item).getByRole('button', { name: '提案を開く' }));
  expect(await within(item).findByRole('alert')).toHaveTextContent('提案の対象が現在の結果と一致しません');
  expect(screen.queryByRole('region', { name: '根拠・候補・人間判断' })).not.toBeInTheDocument();
});

test('partial source use is explicit and a result without a Finding offers no candidate review', async () => {
  setup();
  const partial = { ...structured, evidenceRevisionRefs: [...refs, { id: 'evidence-2', revision: 1 }], findingRevisionRefs: [], sourceOutcomes: [{ evidenceRevisionRef: refs[0], outcome: 'referenced' }, { evidenceRevisionRef: { id: 'evidence-2', revision: 1 }, outcome: 'unavailable' }], suggestedActionIds: ['suggested-2'] };
  mockAgent(partial, { ...succeeded, evidenceRevisionRefs: partial.evidenceRevisionRefs });
  const module = await openAgent();
  const turn = await within(module).findByRole('region', { name: 'Agent実行 execution-1' });
  expect(await within(turn).findByText(/一部の根拠を利用できませんでした/)).toBeVisible();
  expect(within(turn).getByRole('list', { name: '根拠ごとの利用結果' })).toHaveTextContent('根拠 evidence-2（版 1）：利用できなかった');
  expect(within(turn).getByText(/根拠付きの候補はありません/)).toBeVisible();
  expect(within(turn).queryByRole('button', { name: '候補を根拠モジュールで確認' })).not.toBeInTheDocument();
  expect(within(turn).queryByRole('listitem', { name: '提案 suggested-1' })).not.toBeInTheDocument();
});

test('a reviewer without draft editing cannot put a candidate into Work', async () => {
  setup();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, canEdit: false, canSubmit: false, workingArtifacts: [] } as never);
  mockAgent();
  const module = await openAgent();
  const candidate = await within(module).findByRole('article', { name: '下書き候補 generated-1' });
  expect(within(candidate).getByRole('button', { name: '作業文案に入れる' })).toBeDisabled();
  expect(candidate).toHaveTextContent('この工程では作業文案を編集できません');
});

test('the thread keeps earlier requests collapsed and reads only the focused execution', async () => {
  setup();
  jest.mocked(workApi.getTask).mockResolvedValue({ ...detail, agentExecutionIds: ['execution-0', 'execution-1'] } as never);
  mockAgent();
  const module = await openAgent();
  const thread = within(module).getByRole('list', { name: 'Agentとのやり取り' });
  await within(thread).findByRole('region', { name: 'Agent実行 execution-1' });
  expect(jest.mocked(workApi.getAgentExecution).mock.calls.map(([id]) => id)).not.toContain('execution-0');
  await userEvent.click(within(thread).getByRole('button', { name: '依頼 1 を表示' }));
  expect(await within(thread).findByRole('region', { name: 'Agent実行 execution-0' })).toBeVisible();
  expect(within(thread).getByRole('button', { name: '依頼 2 を表示' })).toBeVisible();
  expect(within(thread).queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
});

test('denial of a candidate read hides private Agent content instead of showing stale records', async () => {
  setup(); mockAgent();
  jest.mocked(workApi.getGeneratedArtifact).mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  await openAgent();
  await screen.findByText(/内容を非表示にしました/);
  expect(screen.queryByText('合成の構造化結果')).not.toBeInTheDocument();
  expect(screen.queryByRole('region', { name: 'Agent実行 execution-1' })).not.toBeInTheDocument();
  expect(screen.queryByLabelText('Agentへの依頼目的')).not.toBeInTheDocument();
});
