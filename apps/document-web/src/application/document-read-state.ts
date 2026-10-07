import type { CurrentReadState, ReadStateMutationRequest, ReadStateMutationResult } from '@knowledge-platform/document-api-client';
import { validOccurredAt } from './document-root-folder';
import { problemFromUnknown } from './problem-mapping';

export type ReadStateIntent = Readonly<{
  documentId: string;
  versionId: string;
  title: string;
  versionNo: number;
  kind: 'VIEW' | 'RESET';
  body: Readonly<ReadStateMutationRequest>;
}>;
export type ReadStateOperation = {
  intent: ReadStateIntent;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded';
  result?: ReadStateMutationResult;
  error?: unknown;
  refresh?: 'pending' | 'complete' | 'failed';
};
export const currentReadStateKey = (documentId: string, versionId: string) => ['document-current-read-state', documentId, versionId] as const;
export const maximumReadStateMessage = '既読状態を更新できません。管理者に確認してください';
function revision(value: unknown): value is number { return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0; }
function validProjection(value: unknown): value is CurrentReadState {
  if (!value || typeof value !== 'object') return false;
  const state = value as CurrentReadState;
  if (!revision(state.readStateRevision) || typeof state.needsRecheck !== 'boolean' || typeof state.isRead !== 'boolean') return false;
  if (state.firstReadAt === null) return state.readStateRevision === 0 && !state.needsRecheck && !state.isRead;
  return validOccurredAt(state.firstReadAt) && state.readStateRevision > 0 && state.isRead === !state.needsRecheck;
}
export function validCurrentReadState(value: unknown, documentId: string, versionId: string): value is CurrentReadState {
  return validProjection(value) && value.documentId === documentId && value.versionId === versionId;
}
function verifiedReceipt(intent: ReadStateIntent, result: ReadStateMutationResult): boolean {
  const state = result?.resultingReadState;
  return Boolean(result && result.operationId === intent.body.operationId && result.documentId === intent.documentId
    && result.versionId === intent.versionId && result.kind === intent.kind && result.expectedReadStateRevision === intent.body.expectedReadStateRevision
    && typeof result.changed === 'boolean' && validOccurredAt(result.occurredAt) && validProjection(state) && state.firstReadAt !== null
    && state.readStateRevision === intent.body.expectedReadStateRevision + (result.changed ? 1 : 0)
    && state.needsRecheck === (intent.kind === 'RESET') && state.isRead === (intent.kind === 'VIEW') && (intent.kind !== 'RESET' || result.changed));
}
function recognizedRejection(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  if (!problem || problem.exactRetry) return false;
  const codes: Record<string, number> = { AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404, DOCUMENT_VERSION_NOT_FOUND: 404,
    REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, STALE_VERSION: 409, VALIDATION_FAILED: 422, BUSINESS_RULE_REJECTED: 422 };
  return codes[problem.code] === problem.status;
}
function createStore() {
  const values = new Map<string, ReadStateOperation>();
  const listeners = new Set<() => void>();
  const key = (documentId: string, versionId: string) => JSON.stringify([documentId, versionId]);
  let all: ReadStateOperation[] = [];
  let unresolved: ReadStateOperation[] = [];
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    all = [...values.values()]; unresolved = all.filter(value => value.status === 'pending' || value.status === 'unknown');
    window.removeEventListener('beforeunload', warn);
    if (unresolved.length) window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: (documentId: string, versionId: string) => values.get(key(documentId, versionId)),
    list: () => all,
    listUnresolved: () => unresolved,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    put: (next: ReadStateOperation) => {
      const target = key(next.intent.documentId, next.intent.versionId); const previous = values.get(target);
      if (previous && (previous.status === 'pending' || previous.status === 'unknown') && previous.intent !== next.intent) return;
      values.set(target, next); emit();
    },
    clearSettled: (documentId: string, versionId: string, expected: ReadStateOperation) => {
      if (values.get(key(documentId, versionId)) !== expected || expected.status !== 'succeeded' && expected.status !== 'rejected') return false;
      values.delete(key(documentId, versionId)); emit(); return true;
    },
  };
}
export type ReadStateStore = ReturnType<typeof createStore>;
const owners = new WeakMap<object, ReadStateStore>();
export function documentReadStateOperations(owner: object): ReadStateStore {
  let store = owners.get(owner); if (!store) { store = createStore(); owners.set(owner, store); } return store;
}
export async function sendReadStateOperation({ store, intent, send, invalidate }: {
  store: ReadStateStore;
  intent: ReadStateIntent;
  send: (intent: ReadStateIntent) => Promise<ReadStateMutationResult>;
  invalidate: (intent: ReadStateIntent) => Promise<void>;
}): Promise<void> {
  const previous = store.get(intent.documentId, intent.versionId);
  if (previous && previous.status !== 'unknown') return;
  if (!previous && (!revision(intent.body.expectedReadStateRevision) || intent.body.expectedReadStateRevision >= Number.MAX_SAFE_INTEGER)) throw new Error(maximumReadStateMessage);
  const fixed = previous?.intent ?? Object.freeze({ ...intent, body: Object.freeze({ ...intent.body }) });
  store.put({ intent: fixed, status: 'pending' });
  let confirmed: ReadStateOperation;
  try {
    const result = await send(fixed);
    if (!verifiedReceipt(fixed, result)) throw new Error('既読状態の操作結果を照合できません。');
    confirmed = { intent: fixed, status: 'succeeded', result, refresh: 'pending' }; store.put(confirmed);
  } catch (error) {
    store.put({ intent: fixed, status: !previous && recognizedRejection(error) ? 'rejected' : 'unknown', error }); return;
  }
  let refresh: ReadStateOperation['refresh'] = 'complete';
  try { await invalidate(fixed); } catch { refresh = 'failed'; }
  if (store.get(fixed.documentId, fixed.versionId) === confirmed) store.put({ ...confirmed, refresh });
}
