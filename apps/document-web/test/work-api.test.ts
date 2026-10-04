import { workApi, WorkApiError } from '../src/api/work-api';

const task = { id: 'task-1', contextId: 'context-1', attemptId: 'attempt-1', attemptNumber: 1, revision: 1, title: '内容確認', stepLabel: '内容確認', state: 'active', canClaim: false, canEdit: true, canSubmit: true, canReturn: false, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null };
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
