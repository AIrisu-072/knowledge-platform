import { workApi, WorkApiError } from '../src/api/work-api';

const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null, requiredRoleId: null, claimAssignmentId: null, canAssign: false, assignment: null, workTypeId: 'work-type-1', workTypeLabel: '内容確認', dueAt: null, attention: [], contextTitle: null };
const artifact = { id: 'draft-1', taskId: task.id, attemptId: task.attemptId, revision: 1, schemaId: 'organization.text-draft.v1', value: { text: '文案' }, visibility: 'work_item_private' };
const response = (body: unknown, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => body } as Response);
let fetchMock: jest.Mock;
const original = globalThis.fetch;
beforeEach(() => { fetchMock = jest.fn(); globalThis.fetch = fetchMock; });
afterAll(() => { globalThis.fetch = original; });

test('draft transport sends only OCC, assignment and text to the dedicated private Work endpoint', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'draft_saved', task, artifact }));
  const command = { operationId: '01990000-0000-7000-8000-000000000001', expectedRevision: 1, actingAssignmentId: 'assignment-sales', value: { text: '文案' } };
  const result = await workApi.saveDraft({ ...command, taskId: task.id, artifactId: artifact.id });
  expect(result).toEqual({ kind: 'draft_saved', task, artifact });
  const [url, init] = fetchMock.mock.calls[0]!;
  expect(url).toBe('/v1/organization/working-artifacts/draft-1');
  expect(init).toMatchObject({ method: 'PUT', credentials: 'same-origin', cache: 'no-store' });
  expect(JSON.parse(init.body)).toEqual(command);
  expect(init.headers).not.toHaveProperty('Authorization');
});

test('malformed or pending write response remains unknown rather than success', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'pending' }, 202));
  await expect(workApi.claim('task-1', { operationId: 'operation', expectedRevision: 1, actingAssignmentId: 'assignment' })).rejects.toMatchObject({ outcomeUnknown: true, code: 'invalid_response' });
});

test('a task detail response cannot substitute another task or private attempt', async () => {
  fetchMock.mockResolvedValue(response({ ...task, id: 'other-task', inputResources: [], history: [], workingArtifacts: [artifact] }));
  await expect(workApi.getTask(task.id)).rejects.toBeInstanceOf(WorkApiError);
  fetchMock.mockResolvedValue(response({ ...task, inputResources: [], history: [], workingArtifacts: [{ ...artifact, attemptId: 'other-attempt' }] }));
  await expect(workApi.getTask(task.id)).rejects.toBeInstanceOf(WorkApiError);
});

test('conflict remains a typed conflict and never renders untrusted error body', async () => {
  fetchMock.mockResolvedValue(response({ code: 'REVISION_CONFLICT', title: 'private untrusted message' }, 409));
  await expect(workApi.claim(task.id, { operationId: 'operation', expectedRevision: 1, actingAssignmentId: 'assignment' })).rejects.toMatchObject({ status: 409, code: 'REVISION_CONFLICT', outcomeUnknown: false });
});

test('network loss on a write is explicitly unknown', async () => {
  fetchMock.mockRejectedValue(new TypeError('offline'));
  await expect(workApi.claim(task.id, { operationId: 'operation', expectedRevision: 1, actingAssignmentId: 'assignment' })).rejects.toMatchObject({ status: 0, outcomeUnknown: true });
});

test('operation recovery decodes an immutable return instruction and the separate ready next attempt', async () => {
  const instruction = { id: 'return-1', workflowId: 'workflow-1', contextId: task.contextId, sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: 'sales-task', targetAttemptId: 'sales-attempt-2', previousSubmissionId: 'snapshot-1', transitionId: 'return-transition', reason: '修正してください', returnedBy: 'office-01', actingAssignmentId: 'assignment-office', createdAt: '2026-10-04T07:00:00Z' };
  const result = { kind: 'returned', task: { ...task, state: 'completed', returnInstructionId: instruction.id }, returnInstruction: instruction, nextTask: { ...task, id: instruction.targetTaskId, attemptId: instruction.targetAttemptId, attemptNumber: 2, state: 'ready', canClaim: true, canEdit: false, canSubmit: false } };
  fetchMock.mockResolvedValue(response(result));
  await expect(workApi.getOperation('operation-1')).resolves.toEqual(result);
});

test('return sends the exact OCC attempt, causal target and reason without an identity override', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'claimed', task }));
  const command = { operationId: 'operation-1', expectedRevision: 4, actingAssignmentId: 'assignment-office', expectedAttemptId: 'office-attempt-1', previousSubmissionId: 'snapshot-1', targetTaskId: 'sales-task', transitionId: 'return-transition', reason: '修正してください' };
  await workApi.returnTask('office-task', command);
  const [url, init] = fetchMock.mock.calls[0]!;
  expect(url).toBe('/v1/organization/tasks/office-task/return');
  expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin', cache: 'no-store' });
  expect(JSON.parse(init.body)).toEqual(command);
  expect(init.headers).not.toHaveProperty('Authorization');
});

test('return instruction reads reject a substituted known ID', async () => {
  fetchMock.mockResolvedValue(response({ id: 'other-return', workflowId: 'workflow-1', contextId: task.contextId, sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: 'sales-task', targetAttemptId: 'sales-attempt-2', previousSubmissionId: 'snapshot-1', transitionId: 'return-transition', reason: '修正してください', returnedBy: 'office-01', actingAssignmentId: 'assignment-office', createdAt: '2026-10-04T07:00:00Z' }));
  await expect(workApi.getReturnInstruction('return-1')).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: false });
});

test('a recovered draft cannot inject an artifact from another private attempt', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'draft_saved', task, artifact: { ...artifact, attemptId: 'older-attempt' } }));
  await expect(workApi.getOperation('operation-1')).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: false });
});

const evidence = { id: 'evidence-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, sourceRef: { providerId: 'document', resourceId: 'document-1', revisionId: 'revision-1', versionId: 'version-1' }, authoritativeLocator: { kind: 'contentItem', contentItemId: 'content-1', representationId: 'representation-1' }, relevantLocation: '第1節', origin: 'human', fragmentOmissionReason: 'not_retained', coverage: 'unknown', relevantLocationVerified: false, policyDisposition: 'reference_only', uncertainty: [], conflictReferences: [], createdBy: 'sales-01', actingAssignmentId: 'assignment-sales', recordedAt: '2026-10-04T08:00:00Z', retrievedAt: '2026-10-04T08:00:00Z', providerCheckedAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private' };
const finding = { id: 'finding-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, claim: '候補', evidenceRevisionRefs: [{ id: evidence.id, revision: evidence.revision }], author: 'sales-01', uncertainty: [], conflicts: [], supersedesFindingId: null, actingAssignmentId: 'assignment-sales', createdAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private' };
const decision = { id: 'decision-1', revision: 1, contextId: task.contextId, taskId: task.id, attemptId: task.attemptId, findingId: finding.id, findingRevision: 1, decision: 'modified', adoptedClaim: '修正して採用', reason: '理由', evidenceRevisionRefs: finding.evidenceRevisionRefs, humanPrincipal: 'sales-01', supersedesDecisionId: null, actingAssignmentId: 'assignment-sales', createdAt: '2026-10-04T08:00:00Z', visibility: 'work_item_private' };

test('evidence registration uses the exact published source and closed same-operation transport', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'evidence_registered', task, evidence }));
  const command = { operationId: 'operation-evidence', expectedRevision: 1, actingAssignmentId: 'assignment-sales', expectedAttemptId: task.attemptId, sourceRef: evidence.sourceRef, authoritativeLocator: evidence.authoritativeLocator, relevantLocation: evidence.relevantLocation };
  await expect(workApi.registerEvidence(task.id, command as never)).resolves.toMatchObject({ kind: 'evidence_registered', evidence });
  expect(fetchMock.mock.calls[0]?.[0]).toBe('/v1/organization/tasks/task-1/evidence');
  expect(JSON.parse(fetchMock.mock.calls[0]?.[1].body)).toEqual(command);
});
test('received finding decisions carry the current decision task attempt independently of the finding author', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'decision_recorded', task, decision }));
  const command = { operationId: 'operation-decision', expectedRevision: 1, actingAssignmentId: 'assignment-office', expectedAttemptId: 'office-attempt', taskId: 'office-task', findingRevision: 1, decision: 'modified', adoptedClaim: '修正して採用', evidenceRevisionRefs: finding.evidenceRevisionRefs };
  await expect(workApi.recordDecision(finding.id, command as never)).resolves.toMatchObject({ kind: 'decision_recorded', decision });
  expect(fetchMock.mock.calls[0]?.[0]).toBe('/v1/organization/findings/finding-1/decisions');
  expect(JSON.parse(fetchMock.mock.calls[0]?.[1].body)).toEqual(command);
});
test('snapshot decoding retains exactly the selected evidence, candidate and judgment revisions', async () => {
  const receipt = { id: 'snapshot-1', sourceTaskId: task.id, sourceAttemptId: task.attemptId, targetTaskId: 'office-task', createdAt: evidence.recordedAt, artifacts: [], evidenceRevisionRefs: [finding.evidenceRevisionRefs[0]], findingRevisionRefs: [{ id: finding.id, revision: 1 }], decisionRevisionRefs: [{ id: decision.id, revision: 1 }] };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(workApi.getSnapshot('snapshot-1')).resolves.toEqual(receipt);
});
test('direct evidence reads cannot substitute a different source record ID', async () => {
  fetchMock.mockResolvedValue(response({ ...evidence, id: 'other-evidence' }));
  await expect(workApi.getEvidence(evidence.id)).rejects.toMatchObject({ code: 'invalid_response' });
});

test('reference-only provenance and uncertainty are preserved without upgrading coverage or location verification', async () => {
  const record = { ...evidence, relevantLocationVerified: false, policyDisposition: 'reference_only', uncertainty: ['原本照合は未実施'], conflictReferences: [] };
  fetchMock.mockResolvedValue(response(record));
  await expect(workApi.getEvidence(evidence.id)).resolves.toEqual(record);
  fetchMock.mockResolvedValue(response({ ...record, relevantLocationVerified: true }));
  await expect(workApi.getEvidence(evidence.id)).rejects.toMatchObject({ code: 'invalid_response' });
});

const agentResult = { summary: '固定規則で候補を作成しました。本文分析なし。', findingRevisionRefs: [{ id: 'synthetic-finding', revision: 1 }], evidenceRevisionRefs: [{ id: evidence.id, revision: 1 }], uncertainty: ['本文の検証は行っていません'], simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false };
const execution = { id: 'execution-1', contextId: task.contextId, workItemId: task.id, attemptId: task.attemptId, requestedBy: 'sales-01', requesterResponsibility: 'assignment-sales', executedBy: 'organization-synthetic/agent-01', executorInvocationKind: 'agent', providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' }], effectiveContextRevision: 2, taskRevision: 2, purpose: '参照の確認', evidenceRevisionRefs: [{ id: evidence.id, revision: 1 }], status: 'queued', startedAt: '2026-10-04T13:00:00Z', endedAt: null, result: null, failureCode: null };

test('operation recovery preserves the historical synthetic request receipt and distinct executor provider identities', async () => {
  const receipt = { kind: 'agent_execution_requested', task, execution };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(workApi.getOperation('operation-agent')).resolves.toEqual(receipt);
});

test('synthetic provenance is retained by direct immutable Finding reads', async () => {
  const generated = { ...finding, author: 'organization-synthetic/agent-01', originExecutionId: execution.id };
  fetchMock.mockResolvedValue(response(generated));
  await expect(workApi.getFinding(finding.id)).resolves.toEqual(generated);
});

test('task detail exposes only authorized persisted execution IDs for current-attempt reload', async () => {
  const expected = { ...task, agentExecutionIds: [execution.id], inputResources: [], history: [], workingArtifacts: [] };
  fetchMock.mockResolvedValue(response(expected));
  await expect(workApi.getTask(task.id)).resolves.toEqual(expected);
});

test('Agent request and cancel use the four frozen endpoints and only the scoped command body', async () => {
  const command = { operationId: 'request-op', expectedRevision: 1, actingAssignmentId: 'assignment-sales', expectedAttemptId: task.attemptId, purpose: '参照の確認', evidenceRevisionRefs: [{ id: evidence.id, revision: 1 as const }] };
  fetchMock.mockResolvedValue(response({ kind: 'agent_execution_requested', task, execution }, 202));
  await expect(workApi.requestAgentExecution(task.id, command)).resolves.toMatchObject({ kind: 'agent_execution_requested', execution });
  expect(fetchMock.mock.calls[0]?.[0]).toBe('/v1/organization/tasks/task-1/agent-executions');
  expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'POST', cache: 'no-store', credentials: 'same-origin', body: JSON.stringify(command) });
  const cancelled = { ...execution, status: 'cancelled' };
  const cancel = { operationId: 'cancel-op', expectedRevision: 2, actingAssignmentId: 'assignment-sales', expectedAttemptId: task.attemptId, taskId: task.id };
  fetchMock.mockResolvedValue(response({ kind: 'agent_execution_cancelled', task, execution: cancelled }));
  await expect(workApi.cancelAgentExecution(execution.id, cancel)).resolves.toMatchObject({ execution: cancelled });
  expect(fetchMock.mock.calls[1]?.[0]).toBe('/v1/organization/agent-executions/execution-1/cancel');
  expect(JSON.parse(fetchMock.mock.calls[1]?.[1].body)).toEqual(cancel);
  fetchMock.mockResolvedValue(response(execution));
  await expect(workApi.getAgentExecution(execution.id)).resolves.toEqual(execution);
  expect(fetchMock.mock.calls[2]?.[0]).toBe('/v1/organization/agent-executions/execution-1');
  fetchMock.mockResolvedValue(response(agentResult));
  await expect(workApi.getAgentResult(execution.id)).resolves.toEqual(agentResult);
  expect(fetchMock.mock.calls[3]?.[0]).toBe('/v1/organization/agent-executions/execution-1/result');
});

test('Agent execution decoding rejects swapped IDs and upgraded simulation or provider claims', async () => {
  fetchMock.mockResolvedValue(response({ ...execution, id: 'other-execution' }));
  await expect(workApi.getAgentExecution(execution.id)).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: false });
  fetchMock.mockResolvedValue(response({ ...execution, executedBy: 'sales-01' }));
  await expect(workApi.getAgentExecution(execution.id)).rejects.toMatchObject({ code: 'invalid_response' });
  fetchMock.mockResolvedValue(response({ ...execution, providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-human', invocationKind: 'human' }] }));
  await expect(workApi.getAgentExecution(execution.id)).rejects.toMatchObject({ code: 'invalid_response' });
  for (const changed of [{ liveLlm: true }, { bodyAnalyzed: true }, { mcpWireExecuted: true }, { simulated: false }, { findingRevisionRefs: [] }]) {
    fetchMock.mockResolvedValue(response({ ...agentResult, ...changed }));
    await expect(workApi.getAgentResult(execution.id)).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: false });
  }
});

test('an Agent cancellation response cannot substitute another execution from the same task', async () => {
  fetchMock.mockResolvedValue(response({ kind: 'agent_execution_cancelled', task, execution: { ...execution, id: 'other-execution', status: 'cancelled' } }));
  await expect(workApi.cancelAgentExecution(execution.id, { operationId: 'cancel-op', expectedRevision: 2, actingAssignmentId: 'assignment-sales', expectedAttemptId: task.attemptId, taskId: task.id })).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: true });
});

test('commit-unknown execution remains a typed unknown outcome on current read and historical recovery', async () => {
  const unknown = { ...execution, status: 'outcome_unknown', failureCode: 'commit_outcome_unknown', endedAt: '2026-10-04T13:01:00Z' };
  fetchMock.mockResolvedValue(response(unknown));
  await expect(workApi.getAgentExecution(execution.id)).resolves.toEqual(unknown);
  const receipt = { kind: 'agent_execution_requested', task, execution: unknown };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(workApi.getOperation('request-op')).resolves.toEqual(receipt);
});


test('completion capability and its definition action survive task decoding independently of editing', async () => {
  const current = { ...task, canEdit: false, canSubmit: false, canComplete: true, completionActionId: 'complete-definition-action', inputResources: [], history: [], workingArtifacts: [], agentExecutionIds: [] };
  fetchMock.mockResolvedValue(response(current));
  await expect(workApi.getTask(task.id)).resolves.toEqual(current);
});

test('completion sends only the frozen action and OCC, then recovers the same completed receipt', async () => {
  const command = { operationId: 'complete-op', expectedRevision: 2, actingAssignmentId: 'assignment-office', expectedAttemptId: task.attemptId, action: 'complete' as const, definitionActionId: 'definition-action' };
  const receipt = { kind: 'completed', task: { ...task, revision: 3, state: 'completed', canEdit: false, canSubmit: false, canComplete: false, completionActionId: null } };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(workApi.completeTask(task.id, command)).resolves.toEqual(receipt);
  expect(fetchMock.mock.calls[0]).toEqual(['/v1/organization/tasks/task-1/actions', expect.objectContaining({ method: 'POST', credentials: 'same-origin', cache: 'no-store', body: JSON.stringify(command) })]);
  expect(fetchMock.mock.calls[0]?.[1].headers).not.toHaveProperty('Authorization');
  await expect(workApi.getOperation(command.operationId)).resolves.toEqual(receipt);
});

test.each([{ id: 'other-task' }, { attemptId: 'other-attempt' }, { state: 'active' }])('completion rejects a mismatched receipt as unknown: %j', async (patch) => {
  fetchMock.mockResolvedValue(response({ kind: 'completed', task: { ...task, state: 'completed', ...patch } }));
  await expect(workApi.completeTask(task.id, { operationId: 'complete-op', expectedRevision: 1, actingAssignmentId: 'assignment-office', expectedAttemptId: task.attemptId, action: 'complete', definitionActionId: 'definition-action' })).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: true });
});


test.each([
  { kind: 'held', state: 'held', canHold: false, holdActionId: null, canResume: true, resumeActionId: 'resume-action' },
  { kind: 'resumed', state: 'active', canHold: true, holdActionId: 'hold-action', canResume: false, resumeActionId: null },
])('hold/resume recovery preserves the same task and server action capabilities: $kind', async ({ kind, ...patch }) => {
  const receipt = { kind, task: { ...task, ...patch, revision: 2 } };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(workApi.getOperation('hold-resume-operation')).resolves.toEqual(receipt);
});

test('hold/resume task decoding requires explicit capability and action identity fields', async () => {
  const current = { ...task, canHold: true, holdActionId: 'hold-action', inputResources: [], history: [], workingArtifacts: [], agentExecutionIds: [] };
  fetchMock.mockResolvedValue(response(current));
  await expect(workApi.getTask(task.id)).resolves.toEqual(current);
  for (const field of ['canHold', 'holdActionId', 'canResume', 'resumeActionId']) {
    const incomplete = { ...current } as Record<string, unknown>;
    delete incomplete[field];
    fetchMock.mockResolvedValue(response(incomplete));
    await expect(workApi.getTask(task.id)).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: false });
  }
});

test.each([
  { action: 'hold' as const, kind: 'held', state: 'held' },
  { action: 'resume' as const, kind: 'resumed', state: 'active' },
])('$action transport binds the matching closed result kind and unchanged attempt', async ({ action, kind, state }) => {
  const command = { operationId: 'same-operation', expectedRevision: 2, actingAssignmentId: 'assignment-sales', expectedAttemptId: task.attemptId, definitionActionId: 'definition-action', action };
  const execute = () => action === 'hold' ? workApi.holdTask(task.id, { ...command, action }) : workApi.resumeTask(task.id, { ...command, action });
  const receipt = { kind, task: { ...task, revision: 3, state } };
  fetchMock.mockResolvedValue(response(receipt));
  await expect(execute()).resolves.toEqual(receipt);
  expect(fetchMock.mock.calls[0]).toEqual(['/v1/organization/tasks/task-1/actions', expect.objectContaining({ method: 'POST', credentials: 'same-origin', cache: 'no-store', body: JSON.stringify(command) })]);
  for (const patch of [{ kind: 'claimed' }, { task: { ...receipt.task, id: 'another-task' } }, { task: { ...receipt.task, attemptId: 'another-attempt' } }, { task: { ...receipt.task, state: 'completed' } }]) {
    fetchMock.mockResolvedValue(response({ ...receipt, ...patch }));
    await expect(execute()).rejects.toMatchObject({ code: 'invalid_response', outcomeUnknown: true });
  }
});

test('context, attention and profile decoders are closed and bound to the requested target', async () => {
  const context = { id: 'context-c', kind: 'request', title: '合成依頼C', ownerUnitId: 'unit-sales', progress: [{ taskId: 'task-1', stepLabel: '営業内容整理', workTypeId: 'type-sales', state: 'ready', attemptNumber: 1, dueAt: '2026-10-07T08:00:00Z', assigned: false }], canReadHistory: true, ownTaskIds: [], attentionCount: 1 };
  fetchMock.mockResolvedValue(response({ items: [context], nextCursor: null }));
  expect((await workApi.listWorkContexts('assignment-sales')).items).toEqual([context]);
  expect(fetchMock.mock.calls[0]![0]).toBe('/v1/organization/work-contexts?actingAssignmentId=assignment-sales');
  for (const invalid of [{ ...context, kind: 'customer' }, { ...context, progress: [{ ...context.progress[0], state: 'returned' }] }, { ...context, progress: [{ ...context.progress[0], attemptNumber: 0 }] }]) {
    fetchMock.mockResolvedValue(response({ items: [invalid], nextCursor: null }));
    await expect(workApi.listWorkContexts()).rejects.toMatchObject({ code: 'invalid_response' });
  }
  fetchMock.mockResolvedValue(response({ ...context, id: 'other' }));
  await expect(workApi.getWorkContext(context.id)).rejects.toMatchObject({ code: 'invalid_response' });
  fetchMock.mockResolvedValue(response({ contextId: 'other', entries: [] }));
  await expect(workApi.getWorkContextHistory(context.id)).rejects.toMatchObject({ code: 'invalid_response' });
  const attention = { taskId: 'task-1', attemptId: 'attempt-1', evaluatedAt: '2026-10-07T09:00:00Z', items: [{ kind: 'overdue', sourceId: null, dueAt: '2026-10-07T08:00:00Z' }] };
  fetchMock.mockResolvedValue(response(attention));
  expect(await workApi.markAttentionSeen('task-1', 'period-1')).toEqual(attention);
  expect(JSON.parse(fetchMock.mock.calls.at(-1)![1].body)).toEqual({ workAssignmentId: 'period-1' });
  fetchMock.mockResolvedValue(response({ ...attention, items: [{ kind: 'blocked', sourceId: null, dueAt: null }] }));
  await expect(workApi.getTaskAttention('task-1')).rejects.toMatchObject({ code: 'invalid_response' });
  fetchMock.mockResolvedValue(response({ ...attention, taskId: 'other' }));
  await expect(workApi.getTaskAttention('task-1')).rejects.toMatchObject({ code: 'invalid_response' });
  const profile = { id: 'p', key: 'review-queue', label: '審査', archetype: 'queue', primaryGrouping: 'work_type', defaultSort: 'due_at', initialModule: 'evidence', modules: [{ module: 'evidence', presentation: 'prominent' }] };
  fetchMock.mockResolvedValue(response({ items: [profile], nextCursor: null }));
  expect((await workApi.listWorkViewProfiles()).items).toEqual([profile]);
  fetchMock.mockResolvedValue(response({ items: [{ ...profile, modules: [{ module: 'evidence', presentation: 'granted' }] }], nextCursor: null }));
  await expect(workApi.listWorkViewProfiles()).rejects.toMatchObject({ code: 'invalid_response' });
  // A filtered list never accepts rows outside the requested context.
  fetchMock.mockResolvedValue(response({ items: [{ ...task, contextId: 'other' }], nextCursor: null }));
  await expect(workApi.listTasks('context', undefined, { contextId: 'context-1' })).rejects.toMatchObject({ code: 'invalid_response' });
});
