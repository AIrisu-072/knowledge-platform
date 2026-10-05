/** Transport boundary for the bounded Organization browser PoC. No identity override is sent. */
import type * as Generated from './generated-work/types.gen';
export type WorkSession = Generated.WorkSession;
export type TaskSummary = Generated.TaskSummary;
export type WorkingArtifact = Generated.WorkingArtifact;
export type TaskDetail = Generated.TaskDetail;
export type HandoffSnapshot = Pick<Generated.HandoffSnapshot, 'id' | 'sourceTaskId' | 'sourceAttemptId' | 'targetTaskId' | 'createdAt' | 'artifacts' | 'evidenceRevisionRefs' | 'findingRevisionRefs' | 'decisionRevisionRefs'>;
export type WorkCommand = Generated.WorkCommand;
export type ReturnCommand = Generated.ReturnCommand;
export type WorkflowActionCommand = Generated.WorkflowActionCommand;
export type ReturnInstruction = Generated.ReturnInstruction;
export type RevisionRef = Generated.RevisionRef;
export type SelectedHandoff = Pick<HandoffSnapshot, 'evidenceRevisionRefs' | 'findingRevisionRefs' | 'decisionRevisionRefs'>;
export type EvidenceSource = Pick<Generated.EvidenceRecord, 'sourceRef' | 'authoritativeLocator'>;
type RecordScope = Pick<Generated.EvidenceRecord, 'id' | 'revision' | 'contextId' | 'taskId' | 'attemptId' | 'actingAssignmentId' | 'visibility'>;
export type EvidenceRecord = Generated.EvidenceRecord;
export type Finding = Generated.Finding;
export type HumanDecision = Generated.HumanDecision;
export type EvidenceCommand = Generated.EvidenceCommand;
export type FindingCommand = Generated.FindingCommand;
export type DecisionCommand = Generated.DecisionCommand;
export type SubmitCommand = Generated.SubmitCommand;
export type AgentExecution = Generated.AgentExecution;
export type AgentResult = Generated.AgentResult;
export type AgentExecutionRequest = Generated.AgentExecutionRequest;
export type CancelAgentExecution = Generated.CancelAgentExecution;
export type WorkResult = Generated.Completed | Generated.Held | Generated.Resumed | Generated.AgentExecutionRequested | Generated.AgentExecutionCancelled | Generated.EvidenceRegistered | Generated.FindingRegistered | Generated.DecisionRecorded | Generated.Returned | Generated.DraftSaved | Generated.Claimed | (Omit<Generated.Submitted, 'snapshot'> & { snapshot: HandoffSnapshot });
export class WorkApiError extends Error {
  constructor(public readonly status: number, public readonly code: string, public readonly outcomeUnknown = false) { super(code); this.name = 'WorkApiError'; }
}

type RecordValue = Record<string, unknown>;
function object(value: unknown): RecordValue { if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid_response'); return value as RecordValue; }
function string(value: unknown): string { if (typeof value !== 'string') throw new Error('invalid_response'); return value; }
function bool(value: unknown): boolean { if (typeof value !== 'boolean') throw new Error('invalid_response'); return value; }
function revision(value: unknown): number { if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) throw new Error('invalid_response'); return value; }
function array<T>(value: unknown, decode: (item: unknown) => T): T[] { if (!Array.isArray(value)) throw new Error('invalid_response'); return value.map(decode); }
function textValue(value: unknown): { text: string } { return { text: string(object(value).text) }; }
function schema(value: unknown): 'organization.text-draft.v1' { if (value !== 'organization.text-draft.v1') throw new Error('unsupported_schema'); return value; }
function task(value: unknown): TaskSummary {
  const item = object(value);
  if (revision(item.attemptNumber) < 1) throw new Error('invalid_attempt');
  if (!['ready', 'active', 'held', 'completed'].includes(string(item.state))) throw new Error('invalid_state');
  return { id: string(item.id), contextId: string(item.contextId), attemptId: string(item.attemptId), attemptNumber: revision(item.attemptNumber), revision: revision(item.revision), title: string(item.title), stepLabel: string(item.stepLabel), state: item.state as TaskSummary['state'], canClaim: bool(item.canClaim), canEdit: bool(item.canEdit), canSubmit: bool(item.canSubmit), canComplete: bool(item.canComplete), completionActionId: item.completionActionId === null ? null : string(item.completionActionId), canHold: bool(item.canHold), holdActionId: item.holdActionId === null ? null : string(item.holdActionId), canResume: bool(item.canResume), resumeActionId: item.resumeActionId === null ? null : string(item.resumeActionId), canReturn: bool(item.canReturn), canRegisterEvidence: bool(item.canRegisterEvidence), canRegisterFinding: bool(item.canRegisterFinding), canRecordDecision: bool(item.canRecordDecision), canRequestAgent: bool(item.canRequestAgent), returnTransition: returnTransition(item.returnTransition), returnInstructionId: item.returnInstructionId === null ? null : string(item.returnInstructionId), handoffSnapshotId: item.handoffSnapshotId === null ? null : string(item.handoffSnapshotId) };
}
function returnTransition(value: unknown): TaskSummary['returnTransition'] {
  if (value === null) return null;
  const item = object(value);
  return { transitionId: string(item.transitionId), targetTaskId: string(item.targetTaskId), previousSubmissionId: string(item.previousSubmissionId) };
}
function returnInstruction(value: unknown): ReturnInstruction {
  const item = object(value);
  if (!['sales-01', 'office-01'].includes(string(item.returnedBy))) throw new Error('invalid_actor');
  return { id: string(item.id), workflowId: string(item.workflowId), contextId: string(item.contextId), sourceTaskId: string(item.sourceTaskId), sourceAttemptId: string(item.sourceAttemptId), targetTaskId: string(item.targetTaskId), targetAttemptId: string(item.targetAttemptId), previousSubmissionId: string(item.previousSubmissionId), transitionId: string(item.transitionId), reason: string(item.reason), returnedBy: item.returnedBy as ReturnInstruction['returnedBy'], actingAssignmentId: string(item.actingAssignmentId), createdAt: string(item.createdAt) };
}
function artifact(value: unknown): WorkingArtifact {
  const item = object(value);
  if (item.visibility !== 'work_item_private') throw new Error('invalid_visibility');
  return { id: string(item.id), taskId: string(item.taskId), attemptId: string(item.attemptId), revision: revision(item.revision), schemaId: schema(item.schemaId), value: textValue(item.value), visibility: item.visibility };
}
function reference(value: unknown): RevisionRef { const item = object(value); if (item.revision !== 1) throw new Error('invalid_record_revision'); return { id: string(item.id), revision: item.revision }; }
function scope(value: unknown): RecordScope {
  const item = object(value);
  if (item.visibility !== 'work_item_private' || item.revision !== 1) throw new Error('invalid_record');
  return { ...reference(item), contextId: string(item.contextId), taskId: string(item.taskId), attemptId: string(item.attemptId), actingAssignmentId: string(item.actingAssignmentId), visibility: item.visibility };
}
const nullableString = (value: unknown) => value === null ? null : string(value);
function evidence(value: unknown): EvidenceRecord {
  const item = object(value), source = object(item.sourceRef), locator = object(item.authoritativeLocator);
  if (source.providerId !== 'document' || locator.kind !== 'contentItem' || item.origin !== 'human' || item.fragmentOmissionReason !== 'not_retained' || item.coverage !== 'unknown' || item.relevantLocationVerified !== false || item.policyDisposition !== 'reference_only') throw new Error('unsupported_evidence');
  return { ...scope(item), relevantLocationVerified: false, policyDisposition: 'reference_only', uncertainty: array(item.uncertainty, string), conflictReferences: array(item.conflictReferences, reference), sourceRef: { providerId: source.providerId, resourceId: string(source.resourceId), revisionId: string(source.revisionId), versionId: string(source.versionId) }, authoritativeLocator: { kind: locator.kind, contentItemId: string(locator.contentItemId), representationId: string(locator.representationId) }, relevantLocation: string(item.relevantLocation), createdBy: string(item.createdBy), origin: item.origin, fragmentOmissionReason: item.fragmentOmissionReason, coverage: item.coverage, retrievedAt: string(item.retrievedAt), recordedAt: string(item.recordedAt), providerCheckedAt: string(item.providerCheckedAt) };
}
function finding(value: unknown): Finding { const item = object(value); const refs = array(item.evidenceRevisionRefs, reference); if (!refs.length) throw new Error('unsupported_finding'); return { ...scope(item), uncertainty: array(item.uncertainty, string), conflicts: array(item.conflicts, reference), claim: string(item.claim), evidenceRevisionRefs: refs, author: string(item.author), ...(item.originExecutionId === undefined ? {} : { originExecutionId: string(item.originExecutionId) }), supersedesFindingId: nullableString(item.supersedesFindingId), createdAt: string(item.createdAt) }; }
function decision(value: unknown): HumanDecision {
  const item = object(value);
  if (item.findingRevision !== 1 || !['accepted', 'modified', 'rejected'].includes(string(item.decision)) || (item.decision === 'modified' && !string(item.adoptedClaim).trim())) throw new Error('invalid_decision');
  return { ...scope(item), findingId: string(item.findingId), findingRevision: 1, decision: item.decision as HumanDecision['decision'], adoptedClaim: nullableString(item.adoptedClaim), reason: nullableString(item.reason), evidenceRevisionRefs: array(item.evidenceRevisionRefs, reference), humanPrincipal: string(item.humanPrincipal), createdAt: string(item.createdAt), supersedesDecisionId: nullableString(item.supersedesDecisionId) };
}
function page<T>(value: unknown, decode: (value: unknown) => T): { items: T[]; nextCursor: null } { const item = object(value); if (item.nextCursor !== null) throw new Error('unsupported_pagination'); return { items: array(item.items, decode), nextCursor: null }; }
function exact<T extends { id: string }>(value: unknown, id: string, decode: (value: unknown) => T): T { const record = decode(value); if (record.id !== id) throw new Error('response_target_mismatch'); return record; }
function snapshot(value: unknown): HandoffSnapshot {
  const item = object(value);
  return { evidenceRevisionRefs: array(item.evidenceRevisionRefs, reference), findingRevisionRefs: array(item.findingRevisionRefs, reference), decisionRevisionRefs: array(item.decisionRevisionRefs, reference), id: string(item.id), sourceTaskId: string(item.sourceTaskId), sourceAttemptId: string(item.sourceAttemptId), targetTaskId: string(item.targetTaskId), createdAt: string(item.createdAt), artifacts: array(item.artifacts, (entry) => { const a = object(entry); return { artifactId: string(a.artifactId), revision: revision(a.revision), schemaId: schema(a.schemaId), value: textValue(a.value) }; }) };
}
function agentResult(value: unknown): AgentResult {
  const item = object(value);
  const findings = array(item.findingRevisionRefs, reference), evidence = array(item.evidenceRevisionRefs, reference);
  if (item.simulated !== true || item.bodyAnalyzed !== false || item.liveLlm !== false || item.mcpWireExecuted !== false || findings.length !== 1 || !evidence.length || evidence.length > 16) throw new Error('unsupported_agent_result');
  return { summary: string(item.summary), findingRevisionRefs: [findings[0]!], evidenceRevisionRefs: evidence, uncertainty: array(item.uncertainty, string), simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false };
}
function agentExecution(value: unknown): AgentExecution {
  const item = object(value);
  if (!['sales-01', 'office-01'].includes(string(item.requestedBy)) || item.executedBy !== 'organization-synthetic/agent-01' || item.executorInvocationKind !== 'agent' || !['queued', 'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'].includes(string(item.status))) throw new Error('unsupported_agent_execution');
  const bindings = array(item.providerPrincipalBindings, (value): Generated.ProviderPrincipalBinding => {
    const binding = object(value);
    if (binding.providerId !== 'document' || binding.principalId !== 'poc/poc-agent' || binding.invocationKind !== 'agent') throw new Error('unsupported_agent_provider');
    return { providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' };
  });
  const refs = array(item.evidenceRevisionRefs, reference);
  if (bindings.length !== 1 || !refs.length || refs.length > 16 || (item.failureCode !== null && !['provider_denied', 'context_stale', 'invalid_output', 'dependency_unavailable', 'interrupted', 'commit_outcome_unknown'].includes(string(item.failureCode)))) throw new Error('unsupported_agent_execution');
  const output = item.result === null ? null : agentResult(item.result);
  if ((item.status === 'succeeded') !== Boolean(output)) throw new Error('invalid_agent_outcome');
  return { id: string(item.id), contextId: string(item.contextId), workItemId: string(item.workItemId), attemptId: string(item.attemptId), requestedBy: item.requestedBy as AgentExecution['requestedBy'], requesterResponsibility: string(item.requesterResponsibility), executedBy: item.executedBy, executorInvocationKind: item.executorInvocationKind, providerPrincipalBindings: [bindings[0]!], effectiveContextRevision: revision(item.effectiveContextRevision), taskRevision: revision(item.taskRevision), purpose: string(item.purpose), evidenceRevisionRefs: refs, status: item.status as AgentExecution['status'], startedAt: string(item.startedAt), endedAt: nullableString(item.endedAt), result: output, failureCode: item.failureCode as AgentExecution['failureCode'] };
}
function result(value: unknown): WorkResult {
  const item = object(value);
  if (item.kind === 'agent_execution_requested' || item.kind === 'agent_execution_cancelled') {
    const summary = task(item.task), execution = agentExecution(item.execution);
    if (execution.workItemId !== summary.id || execution.attemptId !== summary.attemptId || execution.contextId !== summary.contextId) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, execution };
  }
  if (item.kind === 'evidence_registered' || item.kind === 'finding_registered' || item.kind === 'decision_recorded') {
    const summary = task(item.task); const record = item.kind === 'evidence_registered' ? evidence(item.evidence) : item.kind === 'finding_registered' ? finding(item.finding) : decision(item.decision);
    if (record.taskId !== summary.id || record.attemptId !== summary.attemptId || record.contextId !== summary.contextId) throw new Error('response_target_mismatch');
    return item.kind === 'evidence_registered' ? { kind: item.kind, task: summary, evidence: record as EvidenceRecord } : item.kind === 'finding_registered' ? { kind: item.kind, task: summary, finding: record as Finding } : { kind: item.kind, task: summary, decision: record as HumanDecision };
  }
  if (item.kind === 'completed' || item.kind === 'held' || item.kind === 'resumed') { const summary = task(item.task); if (summary.state !== ({ completed: 'completed', held: 'held', resumed: 'active' })[item.kind]) throw new Error('invalid_workflow_state'); return { kind: item.kind, task: summary }; }
  if (item.kind === 'claimed') return { kind: item.kind, task: task(item.task) };
  if (item.kind === 'draft_saved') {
    const summary = task(item.task); const saved = artifact(item.artifact);
    if (saved.taskId !== summary.id || saved.attemptId !== summary.attemptId) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, artifact: saved };
  }
  if (item.kind === 'submitted') return { kind: item.kind, task: task(item.task), snapshot: snapshot(item.snapshot), nextTask: task(item.nextTask) };
  if (item.kind === 'returned') {
    const source = task(item.task); const instruction = returnInstruction(item.returnInstruction); const nextTask = task(item.nextTask);
    if (source.id !== instruction.sourceTaskId || source.attemptId !== instruction.sourceAttemptId || source.returnInstructionId !== instruction.id || nextTask.id !== instruction.targetTaskId || nextTask.attemptId !== instruction.targetAttemptId) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: source, returnInstruction: instruction, nextTask };
  }
  throw new Error('invalid_result');
}
async function request<T>(path: string, decode: (value: unknown) => T, method = 'GET', body?: unknown): Promise<T> {
  let response: Response;
  try { response = await fetch(`/v1/organization${path}`, { method, credentials: 'same-origin', cache: 'no-store', headers: body === undefined ? { Accept: 'application/json' } : { Accept: 'application/json', 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }); }
  catch { throw new WorkApiError(0, 'network_unavailable', method !== 'GET'); }
  if (!response.ok) {
    let code = 'request_failed';
    try { const problem = object(await response.json()); if (typeof problem.code === 'string') code = problem.code; } catch { /* Never render untrusted response bodies. */ }
    throw new WorkApiError(response.status, code, method !== 'GET' && (response.status >= 500 || code === 'COMMIT_OUTCOME_UNKNOWN'));
  }
  try { return decode(await response.json()); } catch { throw new WorkApiError(response.status, 'invalid_response', method !== 'GET'); }
}
const segment = encodeURIComponent;
function workflowAction(id: string, command: WorkflowActionCommand) {
  return request(`/tasks/${segment(id)}/actions`, (value) => {
    const receipt = result(value);
    if (receipt.kind !== ({ complete: 'completed', hold: 'held', resume: 'resumed' })[command.action] || receipt.task.id !== id || receipt.task.attemptId !== command.expectedAttemptId) throw new Error('response_target_mismatch');
    return receipt;
  }, 'POST', command);
}
export const workApi = {
  getSession: () => request('/session', (value): WorkSession => { const item = object(value); const c = object(item.capabilities); return { principalId: string(item.principalId), displayName: string(item.displayName), actingAssignmentId: string(item.actingAssignmentId), capabilities: { nativeWorkspace: bool(c.nativeWorkspace), agent: bool(c.agent), search: bool(c.search), fileUpload: bool(c.fileUpload), return: bool(c.return) } }; }),
  listTasks: (view: 'context' | 'queue') => request(`/tasks?view=${view}`, (value) => { const item = object(value); if (item.nextCursor !== null) throw new Error('unsupported_pagination'); return { items: array(item.items, task), nextCursor: null }; }),
  getTask: (id: string) => request(`/tasks/${segment(id)}`, (value): TaskDetail => { const item = object(value); const summary = task(item); const artifacts = array(item.workingArtifacts, artifact); if (summary.id !== id || artifacts.some((entry) => entry.taskId !== id || entry.attemptId !== summary.attemptId)) throw new Error('response_target_mismatch'); return { ...summary, agentExecutionIds: array(item.agentExecutionIds, string), inputResources: array(item.inputResources, (entry) => { const resource = object(entry); if (resource.kind !== 'document') throw new Error('unsupported_resource'); return { kind: 'document', documentId: string(resource.documentId), label: string(resource.label) }; }), history: array(item.history, (entry) => { const event = object(entry); return { kind: string(event.kind), occurredAt: string(event.occurredAt) }; }), workingArtifacts: artifacts }; }),
  getSnapshot: (id: string) => request(`/handoff-snapshots/${segment(id)}`, (value) => { const receipt = snapshot(value); if (receipt.id !== id) throw new Error('response_target_mismatch'); return receipt; }),
  getReturnInstruction: (id: string) => request(`/return-instructions/${segment(id)}`, (value) => { const instruction = returnInstruction(value); if (instruction.id !== id) throw new Error('response_target_mismatch'); return instruction; }),
  completeTask: (id: string, command: WorkflowActionCommand & { action: 'complete' }) => workflowAction(id, command),
  holdTask: (id: string, command: WorkflowActionCommand & { action: 'hold' }) => workflowAction(id, command),
  resumeTask: (id: string, command: WorkflowActionCommand & { action: 'resume' }) => workflowAction(id, command),
  returnTask: (id: string, command: ReturnCommand) => request(`/tasks/${segment(id)}/return`, result, 'POST', command),
  claim: (id: string, command: WorkCommand) => request(`/tasks/${segment(id)}/claim`, result, 'POST', command),
  submit: (id: string, command: SubmitCommand) => request(`/tasks/${segment(id)}/submit`, result, 'POST', command),
  saveDraft: ({ taskId, artifactId, ...command }: WorkCommand & { taskId: string; artifactId?: string; value: { text: string } }) => request(artifactId ? `/working-artifacts/${segment(artifactId)}` : `/tasks/${segment(taskId)}/working-artifacts`, result, artifactId ? 'PUT' : 'POST', command),
  listEvidence: (id: string) => request(`/tasks/${segment(id)}/evidence?limit=100`, (value) => page(value, evidence)),
  getEvidence: (id: string) => request(`/evidence/${segment(id)}`, (value) => exact(value, id, evidence)),
  registerEvidence: (id: string, command: EvidenceCommand) => request(`/tasks/${segment(id)}/evidence`, result, 'POST', command),
  listFindings: (id: string) => request(`/tasks/${segment(id)}/findings?limit=100`, (value) => page(value, finding)),
  getFinding: (id: string) => request(`/findings/${segment(id)}`, (value) => exact(value, id, finding)),
  registerFinding: (id: string, command: FindingCommand) => request(`/tasks/${segment(id)}/findings`, result, 'POST', command),
  listDecisions: (id: string) => request(`/findings/${segment(id)}/decisions?limit=100`, (value) => { const records = page(value, decision); if (records.items.some((item) => item.findingId !== id)) throw new Error('response_target_mismatch'); return records; }),
  recordDecision: (id: string, command: DecisionCommand) => request(`/findings/${segment(id)}/decisions`, result, 'POST', command),
  requestAgentExecution: (id: string, command: AgentExecutionRequest) => request(`/tasks/${segment(id)}/agent-executions`, result, 'POST', command),
  getAgentExecution: (id: string) => request(`/agent-executions/${segment(id)}`, (value) => exact(value, id, agentExecution)),
  getAgentResult: (id: string) => request(`/agent-executions/${segment(id)}/result`, agentResult),
  cancelAgentExecution: (id: string, command: CancelAgentExecution) => request(`/agent-executions/${segment(id)}/cancel`, (value) => { const receipt = result(value); if (receipt.kind !== 'agent_execution_cancelled' || receipt.execution.id !== id || receipt.task.id !== command.taskId || receipt.task.attemptId !== command.expectedAttemptId) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  getOperation: (id: string) => request(`/operations/${segment(id)}`, result),
};
