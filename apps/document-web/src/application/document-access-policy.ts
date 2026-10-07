import type { AccessPolicyRead, CommandsPolicyExplicit, CommandsPolicyInherit, DocumentDetail, MutationResult } from '@knowledge-platform/document-api-client';
import { validOccurredAt, wasRejected } from './document-root-folder';
import { problemFromUnknown } from './problem-mapping';
import { freezePolicyRequest, policyChanged, policyRevisionError, policyReadEqual, validateAccessPolicy } from './document-folder-access-policy';
export { policyChanged, policyRevisionError } from './document-folder-access-policy';
export type DocumentPolicyRequest = CommandsPolicyExplicit | CommandsPolicyInherit;
export type DocumentPolicyContext = { documentId: string; title: string; view: 'published' | 'authoring' };
export type DocumentPolicySnapshot = { document: DocumentDetail; policy: AccessPolicyRead };
export function canManageDocumentPolicy(document: DocumentDetail | undefined, id: string): document is DocumentDetail {
  return Boolean(document?.documentId === id && Number.isSafeInteger(document.revision) && document.revision >= 0 && document.capabilities?.manageAccess?.status === 'available');
}
export function validateDocumentPolicy(policy: AccessPolicyRead | undefined, id: string): policy is AccessPolicyRead { return validateAccessPolicy(policy, 'document', id); }
export function documentPolicySnapshotEqual(a: DocumentPolicySnapshot, b: DocumentPolicySnapshot): boolean {
  return a.document.documentId === b.document.documentId && a.document.revision === b.document.revision && a.document.title === b.document.title && a.document.folderId === b.document.folderId && policyReadEqual(a.policy, b.policy);
}
export type DocumentAccessPolicyOperation = {
  targetDocumentId: string; context: DocumentPolicyContext; request: Readonly<DocumentPolicyRequest>; expectedChanged: boolean;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; result?: MutationResult; refresh?: 'pending' | 'complete' | 'failed';
};
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: DocumentAccessPolicyOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => { window.removeEventListener('beforeunload', warn); if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn); listeners.forEach(listener => listener()); };
  return {
    get: () => operation,
    put: (next: DocumentAccessPolicyOperation) => { operation = next; emit(); },
    clearSettled: (expected: DocumentAccessPolicyOperation) => { if (operation !== expected || operation.status !== 'rejected' && operation.status !== 'succeeded') return false; operation = undefined; emit(); return true; },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function documentAccessPolicyOperations(owner: object) { let store = stores.get(owner); if (!store) { store = createStore(); stores.set(owner, store); } return store; }
export async function sendDocumentAccessPolicyOperation(input: {
  store: ReturnType<typeof documentAccessPolicyOperations>; targetDocumentId: string; context: DocumentPolicyContext; request: DocumentPolicyRequest; baseline?: AccessPolicyRead;
  send: (documentId: string, request: DocumentPolicyRequest) => Promise<MutationResult>;
  invalidate: (operation: DocumentAccessPolicyOperation) => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input; const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  const expectedChanged = previous?.expectedChanged ?? (input.baseline ? policyChanged(input.baseline, input.request) : true);
  if (!previous) { const error = policyRevisionError(input.request.expectedPolicyRevision, expectedChanged); if (error) throw new Error(error); }
  const fixed = previous ?? { targetDocumentId: input.targetDocumentId, context: Object.freeze({ ...input.context }), request: freezePolicyRequest(input.request), expectedChanged };
  store.put({ ...fixed, status: 'pending', error: undefined });
  let confirmed: DocumentAccessPolicyOperation;
  try {
    const result = await send(fixed.targetDocumentId, fixed.request);
    if (!result || result.operationId !== fixed.request.operationId || result.resourceId !== fixed.targetDocumentId || result.changed !== fixed.expectedChanged
      || !Number.isSafeInteger(result.resultingRevision) || result.resultingRevision !== fixed.request.expectedPolicyRevision + (fixed.expectedChanged ? 1 : 0) || !validOccurredAt(result.occurredAt)) throw new Error('アクセス設定の結果を照合できません。');
    confirmed = { ...fixed, status: 'succeeded', result, error: undefined, refresh: 'pending' }; store.put(confirmed);
  } catch (error) { store.put({ ...fixed, status: !previous && (wasRejected(error) || problemFromUnknown(error)?.code === 'DOCUMENT_NOT_FOUND' && problemFromUnknown(error)?.status === 404) && !problemFromUnknown(error)?.exactRetry ? 'rejected' : 'unknown', error }); return; }
  let refresh: DocumentAccessPolicyOperation['refresh'] = 'complete';
  try { await invalidate(confirmed); } catch { refresh = 'failed'; }
  if (store.get() === confirmed) store.put({ ...confirmed, refresh });
}
