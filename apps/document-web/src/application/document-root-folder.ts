import type { CommandsCreateFolder, FolderDetail, MutationResult } from '@knowledge-platform/document-api-client';
import { metadataReason } from './document-metadata';
import { problemFromUnknown } from './problem-mapping';

export function folderName(name: string): string {
  return metadataReason(name).normalize('NFC');
}
export function rootFolderValidation(name: string, reason: string): string | null {
  const normalized = folderName(name);
  // Unlike reason, the backend rejects name control characters before Unicode White_Space trim.
  if (/[\p{Cc}\p{Cs}]/u.test(name) || !normalized || [...normalized].length > 255
    || normalized === '.' || normalized === '..' || /[/\\]/u.test(normalized)) {
    return 'フォルダー名は前後の空白を除き1〜255文字で、制御文字・「.」「..」・スラッシュを含めず入力してください。';
  }
  const trimmedReason = metadataReason(reason);
  const bytes = [...trimmedReason].reduce((total, character) => {
    const point = character.codePointAt(0)!;
    return total + (point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4);
  }, 0);
  if (!trimmedReason || bytes > 1024 || /[\p{Cc}\p{Cs}]/u.test(trimmedReason)) {
    return '作成理由は前後の空白を除き1〜1024 UTF-8 bytesで、制御文字を含めず入力してください。';
  }
  return null;
}
export function canCreateRootFolder(root: FolderDetail | undefined): root is FolderDetail {
  return Boolean(root?.folderId && Number.isSafeInteger(root.revision) && root.revision >= 0
    && root.capabilities?.createFolder?.status === 'available');
}
function wasRejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  const codes: Record<string, number> = {
    VALIDATION_FAILED: 422, AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403,
    FOLDER_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409,
    ROOT_PROTECTED: 409, BUSINESS_RULE_REJECTED: 422,
  };
  return Boolean(problem && codes[problem.code] === problem.status);
}
export type RootFolderOperation = {
  request: Readonly<CommandsCreateFolder>;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded';
  error?: unknown;
  result?: MutationResult;
};

// One root-create operation per QueryClient, including its receipt, across route unmounts.
// No browser storage or cross-session replay; pending/unknown must not be discarded.
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: RootFolderOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    window.removeEventListener('beforeunload', warn);
    if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: () => operation,
    put: (next: RootFolderOperation) => { operation = next; emit(); },
    clearSettled: (expected: RootFolderOperation) => {
      if (operation !== expected || (operation.status !== 'rejected' && operation.status !== 'succeeded')) return false;
      operation = undefined; emit(); return true;
    },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function rootFolderOperations(owner: object) {
  let store = stores.get(owner);
  if (!store) { store = createStore(); stores.set(owner, store); }
  return store;
}
function validOccurredAt(value: unknown): boolean {
  if (typeof value !== 'string' || !Number.isFinite(Date.parse(value))) return false;
  const match = /^(\d{4}-\d{2}-\d{2})T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d+)?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/i.exec(value);
  // Date.parse alone accepts date-only strings and rolls February 30 into March.
  return Boolean(match && new Date(`${match[1]}T00:00:00Z`).toISOString().slice(0, 10) === match[1]);
}
export async function sendRootFolderOperation(input: {
  store: ReturnType<typeof rootFolderOperations>; request: CommandsCreateFolder;
  send: (request: CommandsCreateFolder) => Promise<MutationResult>;
  invalidate: () => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input;
  const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  const request = previous?.request ?? Object.freeze({ ...input.request });
  store.put({ request, status: 'pending' });
  try {
    const result = await send(request);
    if (result.operationId !== request.operationId || result.resourceId !== request.folderId
      || result.changed !== true || result.resultingRevision !== 0
      || !validOccurredAt(result.occurredAt)) {
      throw new Error('作成結果を照合できません。');
    }
    store.put({ request, status: 'succeeded', result });
  } catch (error) {
    store.put({ request, status: !previous && wasRejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  // The returned revision belongs to the new child, never to its parent.
  // A read failure cannot undo the confirmed mutation receipt.
  try { await invalidate(); } catch { /* The read surface owns refresh errors. */ }
}
