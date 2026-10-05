import type { CommandsMetadataPatch, MutationResult } from '@knowledge-platform/document-api-client';
import { problemFromUnknown } from './problem-mapping';

export const metadataFields = [
  { key: 'document_type', label: '文書種別' },
  { key: 'owning_department', label: '所管部署' },
  { key: 'category', label: 'カテゴリ' },
] as const;
type MetadataKey = typeof metadataFields[number]['key'];
export type MetadataField = { original: unknown; present: boolean; value: string; touched: boolean; remove: boolean };
export type MetadataDraft = Record<MetadataKey, MetadataField>;

export function metadataDraft(metadata: Record<string, unknown>): MetadataDraft {
  return Object.fromEntries(metadataFields.map(({ key }) => [key, {
    original: metadata[key], present: Object.hasOwn(metadata, key),
    value: typeof metadata[key] === 'string' ? metadata[key] : '', touched: false, remove: false,
  }])) as MetadataDraft;
}

// Rust str::trim uses Unicode White_Space, which differs from JavaScript trim at NEL/BOM.
export function metadataReason(reason: string): string {
  return reason.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '');
}
function utf8Bytes(value: string): number {
  let bytes = 0;
  for (const character of value) {
    const point = character.codePointAt(0)!;
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes;
}
export function metadataPatch(draft: MetadataDraft): Pick<CommandsMetadataPatch, 'set' | 'unset'> {
  const set: Record<string, string> = {};
  const unset: string[] = [];
  for (const { key } of metadataFields) {
    const field = draft[key];
    if (field.remove) unset.push(key);
    else if (field.touched && (!field.present || field.value !== field.original)) set[key] = field.value;
  }
  return { set, unset };
}
export function metadataValidation(draft: MetadataDraft, reason: string): string | null {
  const normalized = metadataReason(reason);
  if (!normalized || utf8Bytes(normalized) > 1024 || /\p{Cc}/u.test(normalized)) return '変更理由は前後の空白を除き1〜1024 UTF-8 bytesで、制御文字を含めず入力してください。';
  if (utf8Bytes(JSON.stringify(metadataPatch(draft))) > 64 * 1024) return '変更するメタデータはJSON全体で64 KiB以下にしてください。';
  return null;
}
function wasRejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  const codes: Record<string, number> = {
    VALIDATION_FAILED: 422, AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403,
    DOCUMENT_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409,
    RESERVED_DOCUMENT: 409, BUSINESS_RULE_REJECTED: 422,
  };
  return Boolean(problem && codes[problem.code] === problem.status);
}
export type MetadataOperation = {
  request: CommandsMetadataPatch;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded';
  error?: unknown;
  result?: MutationResult;
};

// Only unresolved operation payloads/receipts live here. No storage, draft cache or cross-session replay.
// QueryClient ownership keeps separate application sessions and tests isolated.
const operationStores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  const records = new Map<string, MetadataOperation>();
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => {
    window.removeEventListener('beforeunload', warn);
    if ([...records.values()].some(record => record.status === 'pending' || record.status === 'unknown')) window.addEventListener('beforeunload', warn);
    listeners.forEach(listener => listener());
  };
  return {
    get: (id: string) => records.get(id),
    put: (id: string, operation: MetadataOperation) => { records.set(id, operation); emit(); },
    clear: (id: string) => { records.delete(id); emit(); },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function metadataOperations(owner: object) {
  let store = operationStores.get(owner);
  if (!store) { store = createStore(); operationStores.set(owner, store); }
  return store;
}

export async function sendMetadataOperation(input: {
  store: ReturnType<typeof metadataOperations>; documentId: string; request: CommandsMetadataPatch;
  send: (id: string, request: CommandsMetadataPatch) => Promise<MutationResult>;
  invalidate: () => Promise<unknown>;
}): Promise<void> {
  const { store, documentId, send, invalidate } = input;
  const previous = store.get(documentId);
  if (previous && previous.status !== 'unknown') return;
  // Freeze one canonical wire payload before the first call; retries never rebuild it from current fields/GET.
  const request = previous?.request ?? Object.freeze({ ...input.request,
    set: Object.freeze({ ...input.request.set }), unset: Object.freeze([...input.request.unset]) as unknown as string[],
  });
  store.put(documentId, { request, status: 'pending' });
  try {
    const result = await send(documentId, request);
    if (result.operationId !== request.operationId || result.resourceId !== documentId
      || typeof result.changed !== 'boolean' || !Number.isSafeInteger(result.resultingRevision)
      || result.resultingRevision < 0 || typeof result.occurredAt !== 'string') throw new Error('操作結果を照合できません。');
    store.put(documentId, { request, status: 'succeeded', result });
  } catch (error) {
    store.put(documentId, { request, status: !previous && wasRejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  // A failed refresh cannot change a confirmed mutation into an unknown outcome.
  try { await invalidate(); } catch { /* Read surfaces show their own refresh errors. */ }
}
