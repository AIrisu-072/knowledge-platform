/** Transport boundary for the bounded Organization browser PoC. No identity override is sent. */
import type * as Generated from './generated-work/types.gen';
export type WorkSession = Generated.WorkSession;
export type TaskSummary = Generated.TaskSummary;
export type WorkingArtifact = Generated.WorkingArtifact;
export type TaskDetail = Generated.TaskDetail;
// This view intentionally projects only the snapshot fields rendered by the first slice.
export type HandoffSnapshot = Pick<Generated.HandoffSnapshot, 'id' | 'sourceTaskId' | 'sourceAttemptId' | 'targetTaskId' | 'createdAt' | 'artifacts'>;
export type WorkCommand = Generated.WorkCommand;
export type WorkResult = Generated.DraftSaved | Generated.Claimed | (Omit<Generated.Submitted, 'snapshot'> & { snapshot: HandoffSnapshot });
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
  if (!['ready', 'active', 'held', 'completed'].includes(string(item.state))) throw new Error('invalid_state');
  return { id: string(item.id), contextId: string(item.contextId), attemptId: string(item.attemptId), revision: revision(item.revision), title: string(item.title), stepLabel: string(item.stepLabel), state: item.state as TaskSummary['state'], canClaim: bool(item.canClaim), canEdit: bool(item.canEdit), canSubmit: bool(item.canSubmit), handoffSnapshotId: item.handoffSnapshotId === null ? null : string(item.handoffSnapshotId) };
}
function artifact(value: unknown): WorkingArtifact {
  const item = object(value);
  if (item.visibility !== 'work_item_private') throw new Error('invalid_visibility');
  return { id: string(item.id), taskId: string(item.taskId), attemptId: string(item.attemptId), revision: revision(item.revision), schemaId: schema(item.schemaId), value: textValue(item.value), visibility: item.visibility };
}
function snapshot(value: unknown): HandoffSnapshot {
  const item = object(value);
  return { id: string(item.id), sourceTaskId: string(item.sourceTaskId), sourceAttemptId: string(item.sourceAttemptId), targetTaskId: string(item.targetTaskId), createdAt: string(item.createdAt), artifacts: array(item.artifacts, (entry) => { const a = object(entry); return { artifactId: string(a.artifactId), revision: revision(a.revision), schemaId: schema(a.schemaId), value: textValue(a.value) }; }) };
}
function result(value: unknown): WorkResult {
  const item = object(value);
  if (item.kind === 'claimed') return { kind: item.kind, task: task(item.task) };
  if (item.kind === 'draft_saved') return { kind: item.kind, task: task(item.task), artifact: artifact(item.artifact) };
  if (item.kind === 'submitted') return { kind: item.kind, task: task(item.task), snapshot: snapshot(item.snapshot), nextTask: task(item.nextTask) };
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
export const workApi = {
  getSession: () => request('/session', (value): WorkSession => { const item = object(value); const c = object(item.capabilities); return { principalId: string(item.principalId), displayName: string(item.displayName), actingAssignmentId: string(item.actingAssignmentId), capabilities: { nativeWorkspace: bool(c.nativeWorkspace), agent: bool(c.agent), search: bool(c.search), fileUpload: bool(c.fileUpload), return: bool(c.return) } }; }),
  listTasks: (view: 'context' | 'queue') => request(`/tasks?view=${view}`, (value) => { const item = object(value); if (item.nextCursor !== null) throw new Error('unsupported_pagination'); return { items: array(item.items, task), nextCursor: null }; }),
  getTask: (id: string) => request(`/tasks/${segment(id)}`, (value): TaskDetail => { const item = object(value); const summary = task(item); const artifacts = array(item.workingArtifacts, artifact); if (summary.id !== id || artifacts.some((entry) => entry.taskId !== id || entry.attemptId !== summary.attemptId)) throw new Error('response_target_mismatch'); return { ...summary, inputResources: array(item.inputResources, (entry) => { const resource = object(entry); if (resource.kind !== 'document') throw new Error('unsupported_resource'); return { kind: 'document', documentId: string(resource.documentId), label: string(resource.label) }; }), history: array(item.history, (entry) => { const event = object(entry); return { kind: string(event.kind), occurredAt: string(event.occurredAt) }; }), workingArtifacts: artifacts }; }),
  getSnapshot: (id: string) => request(`/handoff-snapshots/${segment(id)}`, (value) => { const receipt = snapshot(value); if (receipt.id !== id) throw new Error('response_target_mismatch'); return receipt; }),
  claim: (id: string, command: WorkCommand) => request(`/tasks/${segment(id)}/claim`, result, 'POST', command),
  submit: (id: string, command: WorkCommand & { artifacts: { artifactId: string; revision: number }[] }) => request(`/tasks/${segment(id)}/submit`, result, 'POST', command),
  saveDraft: ({ taskId, artifactId, ...command }: WorkCommand & { taskId: string; artifactId?: string; value: { text: string } }) => request(artifactId ? `/working-artifacts/${segment(artifactId)}` : `/tasks/${segment(taskId)}/working-artifacts`, result, artifactId ? 'PUT' : 'POST', command),
  getOperation: (id: string) => request(`/operations/${segment(id)}`, result),
};
