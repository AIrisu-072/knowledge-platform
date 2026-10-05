export { workApi, WorkApiError } from '../api/work-api';
export type { WorkSession, TaskSummary, TaskDetail, WorkingArtifact, HandoffSnapshot, WorkCommand, WorkResult, ReturnCommand, ReturnInstruction, EvidenceRecord, Finding, HumanDecision, RevisionRef, SelectedHandoff, EvidenceCommand, FindingCommand, DecisionCommand, SubmitCommand, AgentExecution, AgentResult, AgentExecutionRequest, CancelAgentExecution } from '../api/work-api';
import { WorkApiError, workApi, type WorkCommand, type ReturnCommand, type EvidenceCommand, type FindingCommand, type DecisionCommand, type SubmitCommand, type WorkResult, type AgentExecutionRequest, type CancelAgentExecution } from '../api/work-api';
export type TaskSearch = { view: 'context' | 'queue'; taskId?: string };
export function validateTaskSearch(value: Record<string, unknown>): TaskSearch {
  return { view: value.view === 'queue' ? 'queue' : 'context', ...(typeof value.taskId === 'string' && /^[a-zA-Z0-9-]{1,128}$/.test(value.taskId) ? { taskId: value.taskId } : {}) };
}
export function workErrorMessage(error: unknown): string {
  if (error instanceof WorkApiError) {
    if (error.outcomeUnknown) return '結果を確認できません。操作が確定した可能性があります。同じ操作IDで結果を確認してください。';
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
  | { kind: 'returned'; taskId: string; input: ReturnCommand }
  | { kind: 'claimed'; taskId: string; input: WorkCommand }
  | { kind: 'draft_saved'; taskId: string; input: WorkCommand & { artifactId?: string; value: { text: string } } }
  | { kind: 'submitted'; taskId: string; input: SubmitCommand };
export function executeWorkOperation(operation: WorkOperation): Promise<WorkResult> {
  switch (operation.kind) {
    case 'agent_execution_requested': return workApi.requestAgentExecution(operation.taskId, operation.input);
    case 'agent_execution_cancelled': return workApi.cancelAgentExecution(operation.executionId, operation.input);
    case 'evidence_registered': return workApi.registerEvidence(operation.taskId, operation.input);
    case 'finding_registered': return workApi.registerFinding(operation.taskId, operation.input);
    case 'decision_recorded': return workApi.recordDecision(operation.findingId, operation.input);
    case 'returned': return workApi.returnTask(operation.taskId, operation.input);
    case 'claimed': return workApi.claim(operation.taskId, operation.input);
    case 'draft_saved': return workApi.saveDraft({ ...operation.input, taskId: operation.taskId });
    case 'submitted': return workApi.submit(operation.taskId, operation.input);
  }
}
export function isOperationNotFound(error: unknown): boolean { return error instanceof WorkApiError && error.status === 404 && error.code === 'WORK_ITEM_NOT_FOUND'; }
