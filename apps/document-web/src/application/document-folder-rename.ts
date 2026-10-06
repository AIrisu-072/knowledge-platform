import type { CommandsRenameFolder, FolderDetail, MutationResult } from '@knowledge-platform/document-api-client';
import { folderName, rootFolderValidation, validOccurredAt, wasRejected, type SelectedFolderContext } from './document-root-folder';

export function renameFolderValidation(name: string, reason: string): string | null {
  return rootFolderValidation(name, reason, '変更理由');
}
export function canRenameFolder(folder: FolderDetail | undefined): folder is FolderDetail {
  return Boolean(folder?.folderId && Number.isSafeInteger(folder.revision) && folder.revision >= 0
    && typeof folder.name === 'string' && folderName(folder.name) === folder.name
    && !rootFolderValidation(folder.name, '確認') && folder.capabilities?.renameFolder?.status === 'available');
}
export function renameRevisionError(folder: Pick<FolderDetail, 'revision' | 'name'>, desiredName: string): string | null {
  if (!Number.isSafeInteger(folder.revision) || folder.revision < 0
    || (folder.name !== folderName(desiredName) && folder.revision >= Number.MAX_SAFE_INTEGER)) {
    return 'フォルダーrevisionの次の値を安全に扱えません。管理者へ確認してください。';
  }
  return null;
}
export type FolderRenameOperation = {
  targetFolderId: string;
  request: Readonly<CommandsRenameFolder>;
  context: SelectedFolderContext;
  currentName: string;
  expectedChanged: boolean;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded';
  error?: unknown;
  result?: MutationResult;
  refresh?: 'pending' | 'complete' | 'failed';
};
// Separate from create: one immutable rename request/receipt per QueryClient, without browser storage.
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: FolderRenameOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    window.removeEventListener('beforeunload', warn);
    if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: () => operation,
    put: (next: FolderRenameOperation) => { operation = next; emit(); },
    clearSettled: (expected: FolderRenameOperation) => {
      if (operation !== expected || (operation.status !== 'rejected' && operation.status !== 'succeeded')) return false;
      operation = undefined; emit(); return true;
    },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function folderRenameOperations(owner: object) {
  let store = stores.get(owner);
  if (!store) { store = createStore(); stores.set(owner, store); }
  return store;
}
export async function sendFolderRenameOperation(input: {
  store: ReturnType<typeof folderRenameOperations>; targetFolderId: string; request: CommandsRenameFolder;
  context: SelectedFolderContext; currentName: string;
  send: (folderId: string, request: CommandsRenameFolder) => Promise<MutationResult>;
  invalidate: (operation: FolderRenameOperation) => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input;
  const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  if (!previous) {
    const error = renameRevisionError({ revision: input.request.expectedFolderRevision, name: input.currentName }, input.request.name);
    if (error) throw new Error(error);
  }
  const fixed = previous ?? {
    targetFolderId: input.targetFolderId, request: Object.freeze({ ...input.request }),
    context: Object.freeze({ ...input.context }), currentName: input.currentName,
    expectedChanged: input.currentName !== input.request.name,
  };
  store.put({ ...fixed, status: 'pending', error: undefined });
  let confirmed: FolderRenameOperation;
  try {
    const result = await send(fixed.targetFolderId, fixed.request);
    const revision = fixed.request.expectedFolderRevision + (fixed.expectedChanged ? 1 : 0);
    if (!result || result.operationId !== fixed.request.operationId || result.resourceId !== fixed.targetFolderId
      || result.changed !== fixed.expectedChanged || !Number.isSafeInteger(result.resultingRevision)
      || result.resultingRevision !== revision || !validOccurredAt(result.occurredAt)) {
      throw new Error('改名結果を照合できません。');
    }
    confirmed = { ...fixed, status: 'succeeded', error: undefined, result, refresh: 'pending' };
    store.put(confirmed);
  } catch (error) {
    store.put({ ...fixed, status: !previous && wasRejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  // A receipt confirms this operation, not the current name. Never patch a row from an old replay.
  let refresh: FolderRenameOperation['refresh'] = 'complete';
  try { await invalidate(confirmed); } catch { refresh = 'failed'; }
  if (store.get() === confirmed) store.put({ ...confirmed, refresh });
}
