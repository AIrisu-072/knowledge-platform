export { workApi, WorkApiError } from '../api/work-api';
export type { PolicyAction, PolicyResult, Responsibility, RoleAssignment, Delegation, OrganizationalUnit, BusinessRole, SyntheticPrincipal, TaskAssignmentView, Attention, TaskAttention, WorkContext, WorkContextHistory, WorkViewProfile } from '../api/work-api';
export { MAX_WORK_FILE_BYTES } from '../api/work-api';
export type { WorkFile, FileGeneration } from '../api/work-api';
export type { WorkSession, TaskSummary, TaskDetail, WorkingArtifact, HandoffSnapshot, WorkCommand, WorkflowActionCommand, WorkResult, ReturnCommand, ReturnInstruction, EvidenceRecord, Finding, HumanDecision, RevisionRef, SelectedHandoff, EvidenceCommand, FindingCommand, DecisionCommand, SubmitCommand, AgentExecution, AgentResult, AgentExecutionRequest, CancelAgentExecution } from '../api/work-api';
import { WorkApiError, workApi, type WorkSession, type WorkCommand, type WorkflowActionCommand, type ReturnCommand, type EvidenceCommand, type FindingCommand, type DecisionCommand, type SubmitCommand, type WorkResult, type AgentExecutionRequest, type CancelAgentExecution } from '../api/work-api';
/** `acting` selects a projection scope only; it is never sent as identity. An absent
 * `view` follows the selected responsibility's WorkViewProfile (presentation only);
 * `contextId`/`workTypeId` narrow the same authorized projection. */
export type TaskSearch = { view?: 'context' | 'queue'; taskId?: string; acting?: string; contextId?: string; workTypeId?: string };
export function validateTaskSearch(value: Record<string, unknown>): TaskSearch {
  const id = (input: unknown) => typeof input === 'string' && /^[a-zA-Z0-9-]{1,128}$/.test(input);
  const view = value.view === 'queue' || value.view === 'context' ? value.view : undefined;
  return { ...(view ? { view } : {}), ...(id(value.taskId) ? { taskId: value.taskId as string } : {}), ...(id(value.acting) ? { acting: value.acting as string } : {}), ...(id(value.contextId) ? { contextId: value.contextId as string } : {}), ...(id(value.workTypeId) ? { workTypeId: value.workTypeId as string } : {}) };
}
export function workErrorMessage(error: unknown): string {
  if (error instanceof WorkApiError) {
    if (error.outcomeUnknown) return '結果を確認できません。操作が確定した可能性があります。同じ操作IDで結果を確認してください。';
    if (error.code === 'WORK_ARTIFACT_UNAVAILABLE') return '作業用保存領域でファイルを確認できません。内容は表示していません。時間をおいて再試行してください。';
    if (error.status === 409) return '競合が発生しました。入力を保持しています。現在の状態を再読込して確認してください。';
    if ([401, 403, 404].includes(error.status)) return 'この情報を現在の担当では利用できません。権限または対象を確認してください。';
  }
  return 'サーバーから結果を取得できませんでした。接続を確認して再読込してください。';
}
export function isDisclosureDenied(error: unknown): boolean { return error instanceof WorkApiError && [401, 403, 404].includes(error.status); }
export function isUnknownOutcome(error: unknown): boolean { return error instanceof WorkApiError && error.outcomeUnknown; }
export function taskStateLabel(state: string): string { return ({ ready: '担当待ち', active: '作業中', held: '保留中', completed: '完了' })[state] ?? '状態を確認できません'; }

/** Closed, replayable commands retain the original OCC and payload under one operation ID. */
export type WorkOperation =
  | { kind: 'agent_execution_requested'; taskId: string; input: AgentExecutionRequest }
  | { kind: 'agent_execution_cancelled'; taskId: string; executionId: string; input: CancelAgentExecution }
  | { kind: 'evidence_registered'; taskId: string; input: EvidenceCommand }
  | { kind: 'finding_registered'; taskId: string; input: FindingCommand }
  | { kind: 'decision_recorded'; taskId: string; findingId: string; input: DecisionCommand }
  | { kind: 'completed'; taskId: string; input: WorkflowActionCommand & { action: 'complete' } }
  | { kind: 'held'; taskId: string; input: WorkflowActionCommand & { action: 'hold' } }
  | { kind: 'resumed'; taskId: string; input: WorkflowActionCommand & { action: 'resume' } }
  | { kind: 'returned'; taskId: string; input: ReturnCommand }
  | { kind: 'claimed'; taskId: string; input: WorkCommand }
  | { kind: 'draft_saved'; taskId: string; input: WorkCommand & { artifactId?: string; value: { text: string } } }
  | { kind: 'submitted'; taskId: string; input: SubmitCommand }
  | { kind: 'artifact_created'; taskId: string; input: WorkCommand & { file: { fileName: string; mediaType: string } } }
  | { kind: 'artifact_content_written'; taskId: string; artifactId: string; content: Blob; input: WorkCommand & { expectedArtifactRevision: number } }
  | { kind: 'artifact_discarded'; taskId: string; artifactId: string; input: WorkCommand & { expectedArtifactRevision: number } }
  | { kind: 'submission_imported'; taskId: string; input: WorkCommand & { expectedAttemptId: string; snapshotId: string } };
export function executeWorkOperation(operation: WorkOperation): Promise<WorkResult> {
  switch (operation.kind) {
    case 'agent_execution_requested': return workApi.requestAgentExecution(operation.taskId, operation.input);
    case 'agent_execution_cancelled': return workApi.cancelAgentExecution(operation.executionId, operation.input);
    case 'evidence_registered': return workApi.registerEvidence(operation.taskId, operation.input);
    case 'finding_registered': return workApi.registerFinding(operation.taskId, operation.input);
    case 'decision_recorded': return workApi.recordDecision(operation.findingId, operation.input);
    case 'completed': return workApi.completeTask(operation.taskId, operation.input);
    case 'held': return workApi.holdTask(operation.taskId, operation.input);
    case 'resumed': return workApi.resumeTask(operation.taskId, operation.input);
    case 'returned': return workApi.returnTask(operation.taskId, operation.input);
    case 'claimed': return workApi.claim(operation.taskId, operation.input);
    case 'draft_saved': return workApi.saveDraft({ ...operation.input, taskId: operation.taskId });
    case 'submitted': return workApi.submit(operation.taskId, operation.input);
    case 'artifact_created': return workApi.createFileArtifact(operation.taskId, operation.input);
    case 'artifact_content_written': return workApi.writeArtifactContent(operation.artifactId, operation.input, operation.content);
    case 'artifact_discarded': return workApi.discardArtifact(operation.artifactId, operation.input);
    case 'submission_imported': return workApi.importSubmission(operation.taskId, operation.input);
  }
}
/** The attempt's recorded acting responsibility when the actor is its assignee,
 * otherwise the session default. A request hint only: the server re-resolves it. */
export function actingFor(session: Pick<WorkSession, 'principalId' | 'actingAssignmentId'>, task: { assignment?: { principalId: string; actingAssignmentId: string } | null }): string {
  return task.assignment && task.assignment.principalId === session.principalId ? task.assignment.actingAssignmentId : (session.actingAssignmentId ?? '');
}
export function isStaleArtifact(error: unknown): boolean { return error instanceof WorkApiError && error.status === 404 && error.code === 'WORK_ARTIFACT_NOT_FOUND'; }
export function isOperationNotFound(error: unknown): boolean { return error instanceof WorkApiError && error.status === 404 && error.code === 'WORK_ITEM_NOT_FOUND'; }
