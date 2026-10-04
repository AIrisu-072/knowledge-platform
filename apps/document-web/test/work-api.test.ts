import { workApi, WorkApiError } from '../src/api/work-api';

const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canReturn: false, canRegisterEvidence: true, canRegisterFinding: true, canRecordDecision: true, canRequestAgent: true, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null };
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
