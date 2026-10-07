import { invalidateDocumentReadStateViews } from './document-view-navigation';
import type { CommandsMoveFolder, FolderDetail, MutationResult } from '@knowledge-platform/document-api-client';
import type { QueryClient } from '@tanstack/react-query';
import { rootFolderValidation, validOccurredAt, wasRejected, type SelectedFolderContext } from './document-root-folder';
import { problemFromUnknown } from './problem-mapping';

export type FolderMoveDestination = SelectedFolderContext | Readonly<{ kind: 'root'; folderId: string; name: string }>;
export function moveReasonValidation(reason: string): string | null {
  return rootFolderValidation('資料', reason, '移動理由');
}
export function canMoveFolder(folder: FolderDetail | undefined, rootId?: string): folder is FolderDetail {
  return Boolean(folder?.folderId && folder.folderId !== rootId && folder.parentFolderId
    && Number.isSafeInteger(folder.revision) && folder.revision >= 0 && folder.name
    && folder.capabilities?.moveFolder?.status === 'available');
}
export function moveRevisionError(revision: number, changed: boolean): string | null {
  return !Number.isSafeInteger(revision) || revision < 0 || changed && revision >= Number.MAX_SAFE_INTEGER
    ? 'フォルダーrevisionの次の値を安全に扱えません。管理者へ確認してください。' : null;
}
export type FolderMoveOperation = {
  targetFolderId: string; request: Readonly<CommandsMoveFolder>; context: SelectedFolderContext;
  destination: FolderMoveDestination; currentName: string; expectedChanged: boolean;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; result?: MutationResult;
  refresh?: 'pending' | 'complete' | 'failed';
};
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: FolderMoveOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    window.removeEventListener('beforeunload', warn);
    if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: () => operation,
    put: (next: FolderMoveOperation) => { operation = next; emit(); },
    clearSettled: (expected: FolderMoveOperation) => {
      if (operation !== expected || operation.status !== 'rejected' && operation.status !== 'succeeded') return false;
      operation = undefined; emit(); return true;
    },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function folderMoveOperations(owner: object) {
  let store = stores.get(owner);
  if (!store) { store = createStore(); stores.set(owner, store); }
  return store;
}
// Folder move changes global access_revision. Reset reads, including Organization document/evidence
// reads, without clearing immutable operation payloads, blobs, mutation cache or provider state.
export async function refreshFolderMoveReads(client: QueryClient): Promise<void> {
  invalidateDocumentReadStateViews(client);
  const reads = new Set(['folder-tree', 'documents', 'document', 'document-versions', 'document-revisions',
    'document-history', 'folder-access-policy', 'document-access-policy', 'document-version', 'document-version-files',
    'document-edit-manifest', 'revision-comparison', 'document-read-opening', 'document-current-read-state']);
  const operations = new Set(['document-working-operation', 'document-schedule-cancel', 'document-lifecycle-operation']);
  // Keep Organization identity/task selection stable: its transient provider holds fixed operations.
  // These module reads can disclose Document/evidence content under current Document authorization.
  const organizationReads = new Set(['document-context', 'evidence-context', 'agent-context', 'snapshot']);
  const filter = { predicate: (query: { queryKey: readonly unknown[] }) => typeof query.queryKey[0] === 'string'
    && !operations.has(query.queryKey[0]) && (reads.has(query.queryKey[0])
      || query.queryKey[0] === 'organization' && typeof query.queryKey[3] === 'string' && organizationReads.has(query.queryKey[3])) };
  // Revoke every old read synchronously before the first await, including a Blob resolving this tick.
  void client.invalidateQueries({ ...filter, refetchType: 'none' });
  await client.cancelQueries(filter, { revert: false });
  // Infinite Folder reads restart at initialPageParam; an old Document URL cursor remains explicit.
  await client.resetQueries(filter, { throwOnError: true });
}
function rejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  return Boolean(problem && !problem.exactRetry && (wasRejected(error)
    || problem.status === 409 && ['FOLDER_CYCLE', 'RESERVED_DOCUMENT'].includes(problem.code)));
}
export async function sendFolderMoveOperation(input: {
  store: ReturnType<typeof folderMoveOperations>; targetFolderId: string; request: CommandsMoveFolder;
  context: SelectedFolderContext; destination: FolderMoveDestination; currentName: string;
  send: (folderId: string, request: CommandsMoveFolder) => Promise<MutationResult>;
  invalidate: (operation: FolderMoveOperation) => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input;
  const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  if (!previous) {
    const error = moveRevisionError(input.request.expectedFolderRevision, input.request.fromParentId !== input.request.toParentId);
    if (error) throw new Error(error);
  }
  const fixed = previous ?? {
    targetFolderId: input.targetFolderId, request: Object.freeze({ ...input.request }),
    context: Object.freeze({ ...input.context }), destination: Object.freeze({ ...input.destination }),
    currentName: input.currentName, expectedChanged: input.request.fromParentId !== input.request.toParentId,
  };
  store.put({ ...fixed, status: 'pending', error: undefined });
  let confirmed: FolderMoveOperation;
  try {
    const result = await send(fixed.targetFolderId, fixed.request);
    const revision = fixed.request.expectedFolderRevision + (fixed.expectedChanged ? 1 : 0);
    if (!result || result.operationId !== fixed.request.operationId || result.resourceId !== fixed.targetFolderId
      || result.changed !== fixed.expectedChanged || !Number.isSafeInteger(result.resultingRevision)
      || result.resultingRevision !== revision || !validOccurredAt(result.occurredAt)) throw new Error('移動結果を照合できません。');
    confirmed = { ...fixed, status: 'succeeded', error: undefined, result, refresh: 'pending' }; store.put(confirmed);
  } catch (error) {
    store.put({ ...fixed, status: !previous && rejected(error) ? 'rejected' : 'unknown', error }); return;
  }
  // A replay receipt is old evidence, never a current parent/name/policy projection.
  let refresh: FolderMoveOperation['refresh'] = 'complete';
  try { await invalidate(confirmed); } catch { refresh = 'failed'; }
  if (store.get() === confirmed) store.put({ ...confirmed, refresh });
}
