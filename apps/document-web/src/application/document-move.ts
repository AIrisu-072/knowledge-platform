import type { CommandsMoveDocument, DocumentDetail, MutationResult } from '@knowledge-platform/document-api-client';
import { rootFolderValidation, validOccurredAt } from './document-root-folder';
import { problemFromUnknown } from './problem-mapping';
import type { FolderMoveDestination } from './document-folder-move';
export type DocumentMoveContext = Readonly<{ documentId: string; title: string; folderId: string; folderName: string; view: 'published' | 'authoring' }>;
export function documentMoveReasonValidation(reason: string): string | null {
  return rootFolderValidation('資料', reason, '移動理由');
}
export function canMoveDocument(document: DocumentDetail | undefined): document is DocumentDetail & { folderId: string; folderName: string } {
  return Boolean(document && typeof document.documentId === 'string' && document.documentId
    && typeof document.title === 'string' && document.title && typeof document.folderId === 'string' && document.folderId
    && typeof document.folderName === 'string' && document.folderName
    && Number.isSafeInteger(document.revision) && document.revision >= 0
    && document.capabilities?.moveDocument?.status === 'available');
}
export function documentMoveRevisionError(revision: number, changed: boolean): string | null {
  return !Number.isSafeInteger(revision) || revision < 0 || changed && revision >= Number.MAX_SAFE_INTEGER
    ? '文書revisionの次の値を安全に扱えません。管理者へ確認してください。' : null;
}
export type DocumentMoveOperation = {
  targetDocumentId: string; request: Readonly<CommandsMoveDocument>; context: DocumentMoveContext;
  destination: FolderMoveDestination; expectedChanged: boolean;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; result?: MutationResult;
  refresh?: 'pending' | 'complete' | 'failed';
};
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: DocumentMoveOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    window.removeEventListener('beforeunload', warn);
    if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: () => operation,
    put: (next: DocumentMoveOperation) => { operation = next; emit(); },
    clearSettled: (expected: DocumentMoveOperation) => {
      if (operation !== expected || operation.status !== 'rejected' && operation.status !== 'succeeded') return false;
      operation = undefined; emit(); return true;
    },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function documentMoveOperations(owner: object) {
  let store = stores.get(owner);
  if (!store) { store = createStore(); stores.set(owner, store); }
  return store;
}
function rejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  const codes: Record<string, number> = { VALIDATION_FAILED: 422, AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404, FOLDER_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, BUSINESS_RULE_REJECTED: 422, RESERVED_DOCUMENT: 409 };
  return Boolean(problem && !problem.exactRetry && codes[problem.code] === problem.status);
}
export async function sendDocumentMoveOperation(input: {
  store: ReturnType<typeof documentMoveOperations>; targetDocumentId: string; request: CommandsMoveDocument;
  context: DocumentMoveContext; destination: FolderMoveDestination;
  send: (documentId: string, request: CommandsMoveDocument) => Promise<MutationResult>;
  invalidate: (operation: DocumentMoveOperation) => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input;
  const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  if (!previous) {
    const error = documentMoveRevisionError(input.request.expectedDocumentRevision, input.request.fromFolderId !== input.request.toFolderId);
    if (error) throw new Error(error);
  }
  const fixed = previous ?? {
    targetDocumentId: input.targetDocumentId, request: Object.freeze({ ...input.request }),
    context: Object.freeze({ ...input.context }), destination: Object.freeze({ ...input.destination }),
    expectedChanged: input.request.fromFolderId !== input.request.toFolderId,
  };
  store.put({ ...fixed, status: 'pending', error: undefined });
  let confirmed: DocumentMoveOperation;
  try {
    const result = await send(fixed.targetDocumentId, fixed.request);
    const revision = fixed.request.expectedDocumentRevision + (fixed.expectedChanged ? 1 : 0);
    if (!result || result.operationId !== fixed.request.operationId || result.resourceId !== fixed.targetDocumentId
      || result.changed !== fixed.expectedChanged || !Number.isSafeInteger(result.resultingRevision)
      || result.resultingRevision !== revision || !validOccurredAt(result.occurredAt)) throw new Error('移動結果を照合できません。');
    confirmed = { ...fixed, status: 'succeeded', error: undefined, result, refresh: 'pending' }; store.put(confirmed);
  } catch (error) {
    store.put({ ...fixed, status: !previous && rejected(error) ? 'rejected' : 'unknown', error }); return;
  }
  // A receipt is operation-time evidence, never the current folder/title/policy projection.
  let refresh: DocumentMoveOperation['refresh'] = 'complete';
  try { await invalidate(confirmed); } catch { refresh = 'failed'; }
  if (store.get() === confirmed) store.put({ ...confirmed, refresh });
}
