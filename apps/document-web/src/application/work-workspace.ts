export { workApi, WorkApiError } from '../api/work-api';
export type { WorkSession, TaskSummary, TaskDetail, WorkingArtifact, HandoffSnapshot, WorkCommand, WorkResult } from '../api/work-api';
import { WorkApiError, workApi, type WorkCommand, type WorkResult } from '../api/work-api';
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
  | { kind: 'claimed'; taskId: string; input: WorkCommand }
  | { kind: 'draft_saved'; taskId: string; input: WorkCommand & { artifactId?: string; value: { text: string } } }
  | { kind: 'submitted'; taskId: string; input: WorkCommand & { artifacts: { artifactId: string; revision: number }[] } };
export function executeWorkOperation(operation: WorkOperation): Promise<WorkResult> {
  switch (operation.kind) {
    case 'claimed': return workApi.claim(operation.taskId, operation.input);
    case 'draft_saved': return workApi.saveDraft({ ...operation.input, taskId: operation.taskId });
    case 'submitted': return workApi.submit(operation.taskId, operation.input);
  }
}
export function isOperationNotFound(error: unknown): boolean { return error instanceof WorkApiError && error.status === 404; }
