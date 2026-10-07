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
export type PolicyAction = Generated.PolicyAction;
export type Responsibility = Generated.Responsibility;
export type OrganizationalUnit = Generated.OrganizationalUnit;
export type BusinessRole = Generated.BusinessRole;
export type RoleAssignment = Generated.RoleAssignment;
export type Delegation = Generated.Delegation;
export type TaskAssignmentView = Generated.TaskAssignmentView;
export type WorkAssignmentRecord = Generated.WorkAssignmentRecord;
export type RoleAssignmentCommand = Generated.RoleAssignmentCommand;
export type DelegationCommand = Generated.DelegationCommand;
export type RevokePolicyRecordCommand = Generated.RevokePolicyRecordCommand;
export type AssignmentCommand = Generated.AssignmentCommand;
export type Attention = Generated.Attention;
export type TaskAttention = Generated.TaskAttention;
export type WorkContext = Generated.WorkContext;
export type WorkContextHistory = Generated.WorkContextHistory;
export type WorkViewProfile = Generated.WorkViewProfile;
export type PolicyResult = Generated.RoleAssignmentCreated | Generated.RoleAssignmentRevoked | Generated.DelegationCreated | Generated.DelegationRevoked;
export const SYNTHETIC_PRINCIPALS = ['sales-01', 'office-01', 'review-01', 'approver-01', 'multi-role-01', 'delegate-01'] as const;
export type SyntheticPrincipal = (typeof SYNTHETIC_PRINCIPALS)[number];
export type CancelAgentExecution = Generated.CancelAgentExecution;
export type WorkFile = Generated.WorkFile;
export type FileGeneration = Generated.FileGeneration;
export type FileArtifactCommand = Generated.FileArtifactCommand;
export type DiscardArtifactCommand = Generated.DiscardArtifactCommand;
export type ImportSubmissionCommand = Generated.ImportSubmissionCommand;
export type WorkResult = Generated.ArtifactCreated | Generated.ArtifactContentWritten | Generated.ArtifactDiscarded | Generated.SubmissionImported | Generated.Completed | Generated.Held | Generated.Resumed | Generated.AgentExecutionRequested | Generated.AgentExecutionCancelled | Generated.EvidenceRegistered | Generated.FindingRegistered | Generated.DecisionRecorded | Generated.Returned | Generated.DraftSaved | Generated.Claimed | Generated.Assigned | (Omit<Generated.Submitted, 'snapshot'> & { snapshot: HandoffSnapshot });
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
const nullable = <T>(value: unknown, decode: (value: unknown) => T): T | null => value === null ? null : decode(value);
function principal(value: unknown): SyntheticPrincipal { const text = string(value); if (!(SYNTHETIC_PRINCIPALS as readonly string[]).includes(text)) throw new Error('invalid_actor'); return text as SyntheticPrincipal; }
const POLICY_ACTIONS: readonly PolicyAction[] = ['context.read', 'context.progress.read', 'context.history.read', 'queue.read', 'work.read', 'work.claim', 'work.assign', 'work.edit', 'work.submit', 'work.return', 'work.complete', 'work.hold', 'work.resume', 'evidence.register', 'finding.register', 'decision.record', 'agent.request', 'organization.manage'];
function policyAction(value: unknown): PolicyAction { const text = string(value); if (!(POLICY_ACTIONS as readonly string[]).includes(text)) throw new Error('invalid_action'); return text as PolicyAction; }
function responsibilityKind(value: unknown): 'role_assignment' | 'delegation' { if (value !== 'role_assignment' && value !== 'delegation') throw new Error('invalid_responsibility'); return value; }
function responsibility(value: unknown): Responsibility {
  const item = object(value);
  return { id: string(item.id), kind: responsibilityKind(item.kind), principal: principal(item.principal), roleId: string(item.roleId), roleLabel: string(item.roleLabel), unitId: string(item.unitId), unitLabel: string(item.unitLabel), actions: array(item.actions, policyAction), validFrom: string(item.validFrom), validUntil: nullable(item.validUntil, string), sourceAssignmentId: nullable(item.sourceAssignmentId, string), delegator: nullable(item.delegator, principal), workViewProfileId: string(item.workViewProfileId) };
}
function oneOf<T extends string>(value: unknown, allowed: readonly T[]): T { const text = string(value); if (!(allowed as readonly string[]).includes(text)) throw new Error('invalid_response'); return text as T; }
const ATTENTION_KINDS = ['newly_assigned', 'returned', 'due_soon', 'overdue'] as const;
const TASK_STATES = ['ready', 'active', 'held', 'completed'] as const;
function attention(value: unknown): Attention { const item = object(value); return { kind: oneOf(item.kind, ATTENTION_KINDS), sourceId: nullable(item.sourceId, string), dueAt: nullable(item.dueAt, string) }; }
function taskAttention(value: unknown): TaskAttention { const item = object(value); return { taskId: string(item.taskId), attemptId: string(item.attemptId), evaluatedAt: string(item.evaluatedAt), items: array(item.items, attention) }; }
function workContext(value: unknown): WorkContext {
  const item = object(value);
  const progress = nullable(item.progress, (entry) => array(entry, (step) => { const value = object(step); if (revision(value.attemptNumber) < 1) throw new Error('invalid_attempt'); return { taskId: string(value.taskId), stepLabel: string(value.stepLabel), workTypeId: string(value.workTypeId), state: oneOf(value.state, TASK_STATES), attemptNumber: revision(value.attemptNumber), dueAt: nullable(value.dueAt, string), assigned: bool(value.assigned) }; }));
  return { id: string(item.id), kind: oneOf(item.kind, ['case', 'routine_run', 'batch', 'request'] as const), title: string(item.title), ownerUnitId: string(item.ownerUnitId), progress, canReadHistory: bool(item.canReadHistory), ownTaskIds: array(item.ownTaskIds, string), attentionCount: revision(item.attentionCount) };
}
const MODULES = ['document', 'history', 'evidence', 'agent', 'search', 'resources', 'return'] as const;
function workViewProfile(value: unknown): WorkViewProfile {
  const item = object(value);
  return { id: string(item.id), key: string(item.key), label: string(item.label), archetype: oneOf(item.archetype, ['context', 'queue'] as const), primaryGrouping: oneOf(item.primaryGrouping, ['context', 'work_type'] as const), defaultSort: oneOf(item.defaultSort, ['due_at'] as const), initialModule: oneOf(item.initialModule, MODULES), modules: array(item.modules, (entry) => { const module = object(entry); return { module: oneOf(module.module, MODULES), presentation: oneOf(module.presentation, ['hidden', 'available', 'visible', 'prominent'] as const) }; }) };
}
function assignmentView(value: unknown): TaskAssignmentView {
  const item = object(value);
  return { principalId: principal(item.principalId), displayName: string(item.displayName), actingAssignmentId: string(item.actingAssignmentId), actingKind: nullable(item.actingKind, responsibilityKind), roleLabel: nullable(item.roleLabel, string), delegatorPrincipalId: nullable(item.delegatorPrincipalId, principal), responsibilityEffective: bool(item.responsibilityEffective) };
}
function workAssignment(value: unknown): WorkAssignmentRecord {
  const item = object(value);
  return { id: string(item.id), taskId: string(item.taskId), attemptId: string(item.attemptId), principal: principal(item.principal), actingAssignmentId: string(item.actingAssignmentId), assignedBy: nullable(item.assignedBy, principal), managerAssignmentId: nullable(item.managerAssignmentId, string), reason: nullable(item.reason, string), startedAt: string(item.startedAt), endedAt: nullable(item.endedAt, string), endedBy: nullable(item.endedBy, principal) };
}
function unit(value: unknown): OrganizationalUnit { const item = object(value); if (item.defaultArchetype !== 'context' && item.defaultArchetype !== 'queue') throw new Error('invalid_unit'); return { id: string(item.id), label: string(item.label), defaultArchetype: item.defaultArchetype, roleIds: array(item.roleIds, string) }; }
function role(value: unknown): BusinessRole { const item = object(value); return { id: string(item.id), key: string(item.key), label: string(item.label), actions: array(item.actions, policyAction) }; }
function roleAssignment(value: unknown): RoleAssignment {
  const item = object(value);
  return { id: string(item.id), principal: principal(item.principal), roleId: string(item.roleId), unitId: string(item.unitId), validFrom: string(item.validFrom), validUntil: nullable(item.validUntil, string), reason: string(item.reason), createdBy: nullable(item.createdBy, principal), createdAt: string(item.createdAt), revokedAt: nullable(item.revokedAt, string), revokedBy: nullable(item.revokedBy, principal), revokeReason: nullable(item.revokeReason, string) };
}
function delegation(value: unknown): Delegation {
  const item = object(value); const actions = array(item.actions, policyAction);
  if (!actions.length) throw new Error('invalid_delegation');
  return { id: string(item.id), sourceAssignmentId: string(item.sourceAssignmentId), delegator: principal(item.delegator), recipient: principal(item.recipient), actions, validFrom: string(item.validFrom), validUntil: string(item.validUntil), reason: string(item.reason), createdBy: principal(item.createdBy), createdAt: string(item.createdAt), revokedAt: nullable(item.revokedAt, string), revokedBy: nullable(item.revokedBy, principal), revokeReason: nullable(item.revokeReason, string) };
}
function policyResult(value: unknown): PolicyResult {
  const item = object(value); const policyRevision = revision(item.policyRevision);
  if (policyRevision < 1) throw new Error('invalid_policy_revision');
  if (item.kind === 'role_assignment_created' || item.kind === 'role_assignment_revoked') return { kind: item.kind, assignment: roleAssignment(item.assignment), policyRevision };
  if (item.kind === 'delegation_created' || item.kind === 'delegation_revoked') return { kind: item.kind, delegation: delegation(item.delegation), policyRevision };
  throw new Error('invalid_result');
}
const TEXT_SCHEMA = 'organization.text-draft.v1', FILE_SCHEMA = 'organization.work-file.v1';
export const MAX_WORK_FILE_BYTES = 8 * 1024 * 1024;
function schema(value: unknown): typeof TEXT_SCHEMA | typeof FILE_SCHEMA { if (value !== TEXT_SCHEMA && value !== FILE_SCHEMA) throw new Error('unsupported_schema'); return value; }
function fileGeneration(value: unknown): FileGeneration {
  const item = object(value); const sizeBytes = revision(item.sizeBytes); const sha256 = string(item.sha256);
  if (sizeBytes < 1 || sizeBytes > MAX_WORK_FILE_BYTES || !/^[0-9a-f]{64}$/.test(sha256) || item.providerId !== 'organization.work-artifacts' || Number.isNaN(Date.parse(string(item.storedAt)))) throw new Error('invalid_file_generation');
  return { id: string(item.id), sizeBytes, sha256, storedAt: string(item.storedAt), providerId: item.providerId };
}
function workFile(value: unknown): WorkFile { const item = object(value); return { fileName: string(item.fileName), mediaType: string(item.mediaType), generation: nullable(item.generation, fileGeneration) }; }
/** A text draft carries only `value`; a work file carries only `file`. */
function body(item: RecordValue, pinned: boolean): Pick<WorkingArtifact, 'schemaId' | 'value' | 'file'> {
  const schemaId = schema(item.schemaId);
  if (schemaId === TEXT_SCHEMA) { if (item.file !== undefined) throw new Error('invalid_artifact'); return { schemaId, value: textValue(item.value) }; }
  if (item.value !== undefined) throw new Error('invalid_artifact');
  const file = workFile(item.file);
  // A submission pins an immutable generation, never a pending file.
  if (pinned && !file.generation) throw new Error('invalid_artifact');
  return { schemaId, file };
}
function task(value: unknown): TaskSummary {
  const item = object(value);
  if (revision(item.attemptNumber) < 1) throw new Error('invalid_attempt');
  if (!['ready', 'active', 'held', 'completed'].includes(string(item.state))) throw new Error('invalid_state');
  return { id: string(item.id), contextId: nullable(item.contextId, string), attemptId: string(item.attemptId), attemptNumber: revision(item.attemptNumber), revision: revision(item.revision), title: string(item.title), stepLabel: string(item.stepLabel), state: item.state as TaskSummary['state'], canClaim: bool(item.canClaim), canEdit: bool(item.canEdit), canSubmit: bool(item.canSubmit), canComplete: bool(item.canComplete), completionActionId: item.completionActionId === null ? null : string(item.completionActionId), canHold: bool(item.canHold), holdActionId: item.holdActionId === null ? null : string(item.holdActionId), canResume: bool(item.canResume), resumeActionId: item.resumeActionId === null ? null : string(item.resumeActionId), canReturn: bool(item.canReturn), canRegisterEvidence: bool(item.canRegisterEvidence), canRegisterFinding: bool(item.canRegisterFinding), canRecordDecision: bool(item.canRecordDecision), canRequestAgent: bool(item.canRequestAgent), returnTransition: returnTransition(item.returnTransition), returnInstructionId: item.returnInstructionId === null ? null : string(item.returnInstructionId), handoffSnapshotId: item.handoffSnapshotId === null ? null : string(item.handoffSnapshotId), requiredRoleId: nullable(item.requiredRoleId, string), claimAssignmentId: nullable(item.claimAssignmentId, string), canAssign: bool(item.canAssign), assignment: nullable(item.assignment, assignmentView), workTypeId: string(item.workTypeId), workTypeLabel: string(item.workTypeLabel), dueAt: nullable(item.dueAt, string), attention: array(item.attention, attention), contextTitle: nullable(item.contextTitle, string) };
}
function returnTransition(value: unknown): TaskSummary['returnTransition'] {
  if (value === null) return null;
  const item = object(value);
  return { transitionId: string(item.transitionId), targetTaskId: string(item.targetTaskId), previousSubmissionId: string(item.previousSubmissionId) };
}
function returnInstruction(value: unknown): ReturnInstruction {
  const item = object(value);
  principal(item.returnedBy);
  return { id: string(item.id), workflowId: string(item.workflowId), contextId: string(item.contextId), sourceTaskId: string(item.sourceTaskId), sourceAttemptId: string(item.sourceAttemptId), targetTaskId: string(item.targetTaskId), targetAttemptId: string(item.targetAttemptId), previousSubmissionId: string(item.previousSubmissionId), transitionId: string(item.transitionId), reason: string(item.reason), returnedBy: item.returnedBy as ReturnInstruction['returnedBy'], actingAssignmentId: string(item.actingAssignmentId), createdAt: string(item.createdAt) };
}
function artifact(value: unknown): WorkingArtifact {
  const item = object(value);
  if (item.visibility !== 'work_item_private') throw new Error('invalid_visibility');
  const derived = item.derivedFrom === undefined ? undefined : object(item.derivedFrom);
  return { id: string(item.id), taskId: string(item.taskId), attemptId: string(item.attemptId), revision: revision(item.revision), ...body(item, false), visibility: item.visibility, ...(derived ? { derivedFrom: { snapshotId: string(derived.snapshotId), artifactId: string(derived.artifactId) } } : {}) };
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
/** Policy record pages carry the server evaluation instant used for status. */
function policyPage<T>(value: unknown, decode: (value: unknown) => T): { items: T[]; nextCursor: null; evaluatedAt: string } { const base = page(value, decode); const evaluatedAt = string(object(value).evaluatedAt); if (Number.isNaN(Date.parse(evaluatedAt))) throw new Error('invalid_response'); return { ...base, evaluatedAt }; }
function exact<T extends { id: string }>(value: unknown, id: string, decode: (value: unknown) => T): T { const record = decode(value); if (record.id !== id) throw new Error('response_target_mismatch'); return record; }
function snapshot(value: unknown): HandoffSnapshot {
  const item = object(value);
  return { evidenceRevisionRefs: array(item.evidenceRevisionRefs, reference), findingRevisionRefs: array(item.findingRevisionRefs, reference), decisionRevisionRefs: array(item.decisionRevisionRefs, reference), id: string(item.id), sourceTaskId: string(item.sourceTaskId), sourceAttemptId: string(item.sourceAttemptId), targetTaskId: string(item.targetTaskId), createdAt: string(item.createdAt), artifacts: array(item.artifacts, (entry) => { const a = object(entry); return { artifactId: string(a.artifactId), revision: revision(a.revision), ...body(a, true) }; }) };
}
function agentResult(value: unknown): AgentResult {
  const item = object(value);
  const findings = array(item.findingRevisionRefs, reference), evidence = array(item.evidenceRevisionRefs, reference);
  if (item.simulated !== true || item.bodyAnalyzed !== false || item.liveLlm !== false || item.mcpWireExecuted !== false || findings.length !== 1 || !evidence.length || evidence.length > 16) throw new Error('unsupported_agent_result');
  return { summary: string(item.summary), findingRevisionRefs: [findings[0]!], evidenceRevisionRefs: evidence, uncertainty: array(item.uncertainty, string), simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false };
}
function agentExecution(value: unknown): AgentExecution {
  const item = object(value);
  if (!(SYNTHETIC_PRINCIPALS as readonly string[]).includes(string(item.requestedBy)) || item.executedBy !== 'organization-synthetic/agent-01' || item.executorInvocationKind !== 'agent' || !['queued', 'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'].includes(string(item.status))) throw new Error('unsupported_agent_execution');
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
    if (execution.workItemId !== summary.id || execution.attemptId !== summary.attemptId || (summary.contextId !== null && execution.contextId !== summary.contextId)) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, execution };
  }
  if (item.kind === 'evidence_registered' || item.kind === 'finding_registered' || item.kind === 'decision_recorded') {
    const summary = task(item.task); const record = item.kind === 'evidence_registered' ? evidence(item.evidence) : item.kind === 'finding_registered' ? finding(item.finding) : decision(item.decision);
    if (record.taskId !== summary.id || record.attemptId !== summary.attemptId || (summary.contextId !== null && record.contextId !== summary.contextId)) throw new Error('response_target_mismatch');
    return item.kind === 'evidence_registered' ? { kind: item.kind, task: summary, evidence: record as EvidenceRecord } : item.kind === 'finding_registered' ? { kind: item.kind, task: summary, finding: record as Finding } : { kind: item.kind, task: summary, decision: record as HumanDecision };
  }
  if (item.kind === 'completed' || item.kind === 'held' || item.kind === 'resumed') { const summary = task(item.task); if (summary.state !== ({ completed: 'completed', held: 'held', resumed: 'active' })[item.kind]) throw new Error('invalid_workflow_state'); return { kind: item.kind, task: summary }; }
  if (item.kind === 'claimed') return { kind: item.kind, task: task(item.task) };
  if (item.kind === 'assigned') {
    const summary = task(item.task); const record = workAssignment(item.assignment);
    if (record.taskId !== summary.id || record.attemptId !== summary.attemptId) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, assignment: record };
  }
  if (item.kind === 'draft_saved') {
    const summary = task(item.task); const saved = artifact(item.artifact);
    if (saved.taskId !== summary.id || saved.attemptId !== summary.attemptId) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, artifact: saved };
  }
  if (item.kind === 'artifact_created' || item.kind === 'artifact_content_written') {
    const summary = task(item.task); const saved = artifact(item.artifact);
    if (saved.taskId !== summary.id || saved.attemptId !== summary.attemptId || saved.schemaId !== FILE_SCHEMA || Boolean(saved.file?.generation) !== (item.kind === 'artifact_content_written')) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, artifact: saved };
  }
  if (item.kind === 'artifact_discarded') return { kind: item.kind, task: task(item.task), artifactId: string(item.artifactId) };
  if (item.kind === 'submission_imported') {
    const summary = task(item.task); const artifacts = array(item.artifacts, artifact);
    if (!artifacts.length || artifacts.some((entry) => entry.taskId !== summary.id || entry.attemptId !== summary.attemptId || !entry.derivedFrom)) throw new Error('response_target_mismatch');
    return { kind: item.kind, task: summary, artifacts };
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
    throw new WorkApiError(response.status, code, method !== 'GET' && (response.status >= 500 || code === 'COMMIT_OUTCOME_UNKNOWN') && code !== 'WORK_ARTIFACT_UNAVAILABLE');
  }
  try { return decode(await response.json()); } catch { throw new WorkApiError(response.status, 'invalid_response', method !== 'GET'); }
}
/** The store refusing a generation is a definite failure: nothing was committed. */
async function failure(response: Response, mutation: boolean): Promise<never> {
  let code = 'request_failed';
  try { const problem = object(await response.json()); if (typeof problem.code === 'string') code = problem.code; } catch { /* Never render untrusted response bodies. */ }
  throw new WorkApiError(response.status, code, mutation && (response.status >= 500 || code === 'COMMIT_OUTCOME_UNKNOWN') && code !== 'WORK_ARTIFACT_UNAVAILABLE');
}
/** Binary content upload: operation identity travels in headers, never a local path. */
async function upload(artifactId: string, command: WorkCommand & { expectedArtifactRevision: number }, content: Blob): Promise<WorkResult> {
  let response: Response;
  try {
    response = await fetch(`/v1/organization/working-artifacts/${segment(artifactId)}/content`, { method: 'PUT', credentials: 'same-origin', cache: 'no-store', headers: { Accept: 'application/json', 'Content-Type': 'application/octet-stream', 'x-operation-id': command.operationId, 'x-expected-revision': String(command.expectedRevision), 'x-acting-assignment-id': command.actingAssignmentId, 'x-expected-artifact-revision': String(command.expectedArtifactRevision) }, body: content });
  } catch { throw new WorkApiError(0, 'network_unavailable', true); }
  if (!response.ok) return failure(response, true);
  try {
    const receipt = result(await response.json());
    if (receipt.kind !== 'artifact_content_written' || receipt.artifact.id !== artifactId || receipt.artifact.file?.generation?.id !== command.operationId || receipt.artifact.file.generation.sizeBytes !== content.size) throw new Error('response_target_mismatch');
    return receipt;
  } catch { throw new WorkApiError(response.status, 'invalid_response', true); }
}
/** An opaque attachment of exactly the expected generation size. */
async function download(path: string, generation: FileGeneration, signal?: AbortSignal): Promise<Blob> {
  let response: Response;
  try { response = await fetch(`/v1/organization${path}`, { method: 'GET', credentials: 'same-origin', cache: 'no-store', headers: { Accept: 'application/octet-stream' }, signal }); }
  catch { throw new WorkApiError(0, 'network_unavailable', false); }
  if (!response.ok) return failure(response, false);
  const blob = await response.blob().catch(() => { throw new WorkApiError(response.status, 'invalid_response', false); });
  if (blob.size !== generation.sizeBytes) throw new WorkApiError(response.status, 'invalid_response', false);
  return blob;
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
  getSession: () => request('/session', (value): WorkSession => {
    const item = object(value); const c = object(item.capabilities); const principalId = principal(item.principalId);
    const responsibilities = nullable(item.responsibilities, (entry) => array(entry, responsibility));
    if (responsibilities?.some((entry) => entry.principal !== principalId)) throw new Error('response_target_mismatch');
    const actingAssignmentId = nullable(item.actingAssignmentId, string);
    if (actingAssignmentId !== null && !responsibilities?.some((entry) => entry.id === actingAssignmentId)) throw new Error('invalid_acting_responsibility');
    return { principalId, displayName: string(item.displayName), actingAssignmentId, responsibilities, canManageOrganization: bool(item.canManageOrganization), policyRevision: nullable(item.policyRevision, revision), capabilities: { nativeWorkspace: bool(c.nativeWorkspace), agent: bool(c.agent), search: bool(c.search), fileUpload: bool(c.fileUpload), return: bool(c.return) } };
  }),
  listTasks: (view: 'context' | 'queue', actingAssignmentId?: string, filter: { contextId?: string; workTypeId?: string } = {}) => request(`/tasks?view=${view}&limit=100${actingAssignmentId ? `&actingAssignmentId=${segment(actingAssignmentId)}` : ''}${filter.contextId ? `&contextId=${segment(filter.contextId)}` : ''}${filter.workTypeId ? `&workTypeId=${segment(filter.workTypeId)}` : ''}`, (value) => { const item = object(value); if (item.nextCursor !== null) throw new Error('unsupported_pagination'); const items = array(item.items, task); if (items.some((entry) => (filter.contextId && entry.contextId !== filter.contextId) || (filter.workTypeId && entry.workTypeId !== filter.workTypeId))) throw new Error('response_target_mismatch'); return { items, nextCursor: null }; }),
  listWorkViewProfiles: () => request('/work-view-profiles', (value) => page(value, workViewProfile)),
  listWorkContexts: (actingAssignmentId?: string) => request(`/work-contexts${actingAssignmentId ? `?actingAssignmentId=${segment(actingAssignmentId)}` : ''}`, (value) => page(value, workContext)),
  getWorkContext: (id: string) => request(`/work-contexts/${segment(id)}`, (value) => exact(value, id, workContext)),
  getWorkContextHistory: (id: string) => request(`/work-contexts/${segment(id)}/history`, (value): WorkContextHistory => { const item = object(value); if (string(item.contextId) !== id) throw new Error('response_target_mismatch'); return { contextId: id, entries: array(item.entries, (entry) => { const event = object(entry); return { kind: string(event.kind), occurredAt: string(event.occurredAt) }; }) }; }),
  getTaskAttention: (id: string) => request(`/tasks/${segment(id)}/attention`, (value) => { const receipt = taskAttention(value); if (receipt.taskId !== id) throw new Error('response_target_mismatch'); return receipt; }),
  markAttentionSeen: (id: string, workAssignmentId: string) => request(`/tasks/${segment(id)}/attention-seen`, (value) => { const receipt = taskAttention(value); if (receipt.taskId !== id) throw new Error('response_target_mismatch'); return receipt; }, 'POST', { workAssignmentId }),
  listUnits: () => request('/units', (value) => page(value, unit)),
  listRoles: () => request('/roles', (value) => page(value, role)),
  listRoleAssignments: () => request('/role-assignments', (value) => policyPage(value, roleAssignment)),
  listDelegations: () => request('/delegations', (value) => policyPage(value, delegation)),
  createRoleAssignment: (command: RoleAssignmentCommand) => request('/role-assignments', (value) => { const receipt = policyResult(value); if (receipt.kind !== 'role_assignment_created' || receipt.assignment.principal !== command.principalId || receipt.assignment.roleId !== command.roleId) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  revokeRoleAssignment: (id: string, command: RevokePolicyRecordCommand) => request(`/role-assignments/${segment(id)}/revoke`, (value) => { const receipt = policyResult(value); if (receipt.kind !== 'role_assignment_revoked' || receipt.assignment.id !== id) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  createDelegation: (command: DelegationCommand) => request('/delegations', (value) => { const receipt = policyResult(value); if (receipt.kind !== 'delegation_created' || receipt.delegation.sourceAssignmentId !== command.sourceAssignmentId || receipt.delegation.recipient !== command.recipientPrincipalId) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  revokeDelegation: (id: string, command: RevokePolicyRecordCommand) => request(`/delegations/${segment(id)}/revoke`, (value) => { const receipt = policyResult(value); if (receipt.kind !== 'delegation_revoked' || receipt.delegation.id !== id) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  assignTask: (id: string, command: AssignmentCommand) => request(`/tasks/${segment(id)}/assignment`, (value) => { const receipt = result(value); if (receipt.kind !== 'assigned' || receipt.task.id !== id || receipt.task.attemptId !== command.expectedAttemptId || receipt.assignment.principal !== command.assigneePrincipalId) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  getPolicyOperation: (id: string) => request(`/operations/${segment(id)}`, policyResult),
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
  createFileArtifact: (taskId: string, command: FileArtifactCommand) => request(`/tasks/${segment(taskId)}/working-artifacts`, (value) => { const receipt = result(value); if (receipt.kind !== 'artifact_created' || receipt.task.id !== taskId || receipt.artifact.file?.fileName !== command.file.fileName) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  writeArtifactContent: (artifactId: string, command: WorkCommand & { expectedArtifactRevision: number }, content: Blob) => upload(artifactId, command, content),
  readArtifactContent: (artifactId: string, generation: FileGeneration, signal?: AbortSignal) => download(`/working-artifacts/${segment(artifactId)}/content`, generation, signal),
  readSnapshotContent: (snapshotId: string, artifactId: string, generation: FileGeneration, signal?: AbortSignal) => download(`/handoff-snapshots/${segment(snapshotId)}/artifacts/${segment(artifactId)}/content`, generation, signal),
  discardArtifact: (artifactId: string, command: DiscardArtifactCommand) => request(`/working-artifacts/${segment(artifactId)}/discard`, (value) => { const receipt = result(value); if (receipt.kind !== 'artifact_discarded' || receipt.artifactId !== artifactId) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
  importSubmission: (taskId: string, command: ImportSubmissionCommand) => request(`/tasks/${segment(taskId)}/working-artifacts/import`, (value) => { const receipt = result(value); if (receipt.kind !== 'submission_imported' || receipt.task.id !== taskId || receipt.task.attemptId !== command.expectedAttemptId || receipt.artifacts.some((entry) => entry.derivedFrom?.snapshotId !== command.snapshotId)) throw new Error('response_target_mismatch'); return receipt; }, 'POST', command),
};
