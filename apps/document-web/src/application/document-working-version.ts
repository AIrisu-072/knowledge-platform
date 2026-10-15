import type { QueryClient } from '@tanstack/react-query';
import type { CommandsVersionWrite, ModelsEditManifest, VersionMutationResult } from '@knowledge-platform/document-api-client';
import { documentApi, type DocumentDetail, type VersionDetail } from './document-workspace';
import { createOperationId } from './operation-id';
import { problemFromUnknown } from './problem-mapping';
import { refreshLifecycleQueries } from './document-lifecycle-operations';

export type EditManifest = ModelsEditManifest;
export type AddedWorkingOriginal = { id: string; logicalPath: string; file: File };
export type WorkingStructure = { itemIds: string[]; additions: AddedWorkingOriginal[] };
export function addedOriginalPathError(path: string, existingPaths: readonly string[]): string | null {
  const normalized = path.normalize('NFC');
  if (!normalized || normalized.startsWith('/') || /[\\\u0000-\u001f\u007f-\u009f]/.test(normalized)
    || normalized.split('/').some(segment => !segment || segment === '.' || segment === '..')) return '原本パスは曖昧な要素のない相対パスを入力してください。';
  if (existingPaths.some(existing => existing.normalize('NFC') === normalized)) return '原本パスが重複しています。';
  return null;
}
export function addedOriginalFileError(file: File): string | null {
  if (file.size > 256 * 1024 * 1024) return '1ファイルあたり256 MiB以下のファイルを選択してください。';
  if (!file.type) return '追加原本のファイル形式を確認できません。';
  return null;
}
type Representation = EditManifest['items'][number]['representations'][number];
export type WorkingWriteIntent = {
  kind: 'create' | 'update'; documentId: string; sourceVersionId: string; body: CommandsVersionWrite;
  files: ReadonlyMap<string, Blob | File>; prepared: ReturnType<typeof documentApi.prepareVersionUpload>;
};
export type WorkingIntent = WorkingWriteIntent | {
  kind: 'rebase'; documentId: string; sourceVersionId: string; currentVersionId: string;
  body: { operationId: string; expectedRevision: number };
};
export type WorkingOperation = { intent: WorkingIntent; status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; message?: string };
export const workingOperationKey = (documentId: string) => ['document-working-operation', documentId] as const;
export function workingEditorSource(document: DocumentDetail | undefined, version: VersionDetail | undefined) {
  const working = version?.lifecycleState === 'working' || (!version && document?.displayVersion.lifecycleState === 'WORKING');
  return { mode: working ? 'update' as const : 'create' as const, purpose: working ? 'authoring' as const : 'published' as const,
    sourceVersionId: working ? version?.versionId ?? document?.displayVersion.versionId : document?.currentVersionId };
}
export function unresolvedWorkingOperation(client: QueryClient, documentId: string): boolean {
  const operation = client.getQueryData<WorkingOperation | null>(workingOperationKey(documentId));
  return operation?.status === 'pending' || operation?.status === 'unknown';
}
const MiB = 1024 * 1024;
export function originalOf(item: EditManifest['items'][number]): Representation {
  const originals = item.representations.filter(part => part.role === 'authoritative');
  if (originals.length !== 1) throw new Error('原本の構成を確認できません。');
  return originals[0]!;
}
export function replacementError(original: Representation, file: File): string | null {
  if (file.size > 256 * MiB) return '1ファイルあたり256 MiB以下のファイルを選択してください。';
  // This is only a conservative UI check. The server inspects the actual format and authority.
  if (file.type && file.type.toLowerCase() !== original.mediaType.toLowerCase()) return '原本と同じ形式のファイルを選択してください。形式変更には対応していません。';
  const extension = (name: string) => name.match(/\.[^.\/]+$/)?.[0]?.toLowerCase();
  if (!file.type && extension(file.name) !== extension(original.originalFilename)) return 'ファイル形式を確認できません。原本と同じ拡張子のファイルを選択してください。';
  return null;
}
function checkCancelled(signal: AbortSignal) {
  if (signal.aborted) throw new Error('保存の準備をキャンセルしました。');
}
export async function prepareWorkingVersion(input: {
  mode: 'create' | 'update'; manifest: EditManifest; title: string; replacements: ReadonlyMap<string, File>; signal: AbortSignal; structure?: WorkingStructure;
}): Promise<WorkingWriteIntent> {
  const { manifest, replacements, signal } = input;
  checkCancelled(signal);
  if (!input.title.trim() || !manifest.items.length) throw new Error('文書名と原本を確認してください。');
  if (manifest.purpose !== (input.mode === 'update' ? 'authoring' : 'published')) throw new Error('編集元の用途が一致しません。');
  const structure = input.structure;
  if (structure && input.mode !== 'update') throw new Error('原本の構成変更は作業版で行ってください。');
  const orderedIds = structure?.itemIds ?? manifest.items.map(item => item.contentItemId);
  if (!orderedIds.length) throw new Error('原本は1件以上必要です。');
  const additions = new Map<string, AddedWorkingOriginal>();
  const existingItems = new Map(manifest.items.map(item => [item.contentItemId, item]));
  if (new Set(orderedIds).size !== orderedIds.length || existingItems.size !== manifest.items.length) throw new Error('原本の構成を確認できません。');
  const paths = orderedIds.flatMap(id => existingItems.has(id) ? [existingItems.get(id)!.logicalPath] : []);
  for (const addition of structure?.additions ?? []) {
    if (existingItems.has(addition.id) || additions.has(addition.id) || !orderedIds.includes(addition.id)) throw new Error('追加原本の構成を確認できません。');
    const error = addedOriginalPathError(addition.logicalPath, paths) ?? addedOriginalFileError(addition.file);
    if (error) throw new Error(error);
    const normalized = addition.logicalPath.normalize('NFC'); paths.push(normalized);
    additions.set(addition.id, { ...addition, logicalPath: normalized });
  }
  if (orderedIds.some(id => !existingItems.has(id) && !additions.has(id))) throw new Error('原本の構成を確認できません。');
  const structuralChange = additions.size > 0 || orderedIds.length !== manifest.items.length
    || orderedIds.some((id, index) => id !== manifest.items[index]!.contentItemId);
  const ids = new Set<string>();
  const planned: Array<{ partId: string; file?: File; itemId: string; representation: Representation }> = [];
  const body: CommandsVersionWrite = {
    operationId: createOperationId(), targetVersionId: input.mode === 'update' ? manifest.sourceVersionId : createOperationId(),
    expectedRevision: manifest.documentRevision, title: input.title.trim(), items: orderedIds.map((itemId, index) => {
      const addition = additions.get(itemId);
      if (addition) {
        const fileId = createOperationId(); const partId = createOperationId(); ids.add(fileId);
        const representation: Representation = { fileId, representationId: '', role: 'authoritative', mediaType: addition.file.type,
          originalFilename: addition.file.name, sizeBytes: addition.file.size };
        planned.push({ partId, file: addition.file, itemId, representation });
        return { logicalPath: addition.logicalPath, ordinal: index, fileId, partId, mediaType: addition.file.type, originalFilename: addition.file.name, renditions: [] };
      }
      const item = existingItems.get(itemId)!;
      const original = originalOf(item);
      const replacement = replacements.get(item.contentItemId);
      if (replacement) { const error = replacementError(original, replacement); if (error) throw new Error(error); }
      const parts = replacement ? [original] : item.representations;
      const mapped = parts.map(part => {
        const isReplacement = Boolean(replacement && part.role === 'authoritative');
        const fileId = isReplacement ? createOperationId() : part.fileId;
        if (ids.has(fileId)) throw new Error('保持するファイルに共有FileIDがあります。この画面から安全に保存できません。');
        ids.add(fileId);
        if (!Number.isSafeInteger(part.sizeBytes) || part.sizeBytes < 0 || (!isReplacement && part.sizeBytes > 256 * MiB)) throw new Error('保持する各ファイルは256 MiB以下である必要があります。');
        const partId = createOperationId();
        planned.push({ partId, ...(isReplacement ? { file: replacement! } : {}), itemId: item.contentItemId, representation: part });
        return { fileId, partId, mediaType: part.mediaType, originalFilename: isReplacement ? replacement!.name : part.originalFilename };
      });
      const authoritativeIndex = parts.findIndex(part => part.role === 'authoritative');
      return { logicalPath: item.logicalPath, ordinal: structuralChange ? index : item.ordinal, ...mapped[authoritativeIndex]!, renditions: mapped.filter((_, index) => index !== authoritativeIndex) };
    }),
  };
  if (planned.length > 63) throw new Error('原本と補助ファイルの合計は63件以下である必要があります。');
  if (new Blob([JSON.stringify(body)]).size > MiB) throw new Error('manifest JSONは1 MiB以下である必要があります。');
  if (planned.reduce((total, part) => total + (part.file?.size ?? part.representation.sizeBytes), 0) >= 1024 * MiB) throw new Error('JSONと境界を含む送信全体は1 GiB以下である必要があります。');
  const files = new Map<string, Blob | File>();
  for (const part of planned) {
    checkCancelled(signal);
    const blob = part.file ?? await documentApi.downloadVersionFile({ documentId: manifest.documentId, versionId: manifest.sourceVersionId,
      contentItemId: part.itemId, representationId: part.representation.representationId, purpose: manifest.purpose }, { signal });
    checkCancelled(signal);
    if (!part.file && blob.size !== part.representation.sizeBytes) throw new Error('取得したファイルのサイズがmanifestと一致しません。');
    files.set(part.partId, blob);
  }
  const prepared = documentApi.prepareVersionUpload(body, files);
  for (const item of body.items) { item.renditions?.forEach(Object.freeze); Object.freeze(item.renditions); Object.freeze(item); }
  Object.freeze(body.items); Object.freeze(body);
  return { kind: input.mode, documentId: manifest.documentId, sourceVersionId: manifest.sourceVersionId, body, files, prepared };
}
const unloadGuards = new WeakMap<QueryClient, () => void>();
function guardUnresolved(client: QueryClient) {
  if (unloadGuards.has(client)) return;
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const cleanup = () => { window.removeEventListener('beforeunload', warn); unsubscribe(); unloadGuards.delete(client); };
  const check = () => {
    if (!client.getQueriesData<WorkingOperation | null>({ queryKey: ['document-working-operation'] }).some(([, item]) => item?.status === 'pending' || item?.status === 'unknown')) cleanup();
  };
  const unsubscribe = client.getQueryCache().subscribe(check);
  unloadGuards.set(client, cleanup); window.addEventListener('beforeunload', warn); check();
}
function wasRejected(error: unknown) {
  const problem = problemFromUnknown(error);
  if (!problem || problem.exactRetry) return false;
  const codes: Record<string, number> = { AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404, DOCUMENT_VERSION_NOT_FOUND: 404,
    REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, STALE_VERSION: 409, RESERVED_DOCUMENT: 409, VALIDATION_FAILED: 422,
    BUSINESS_RULE_REJECTED: 422, PUBLISH_QUALITY_REJECTED: 422, UNSUPPORTED_MEDIA_TYPE: 415 };
  return codes[problem.code] === problem.status;
}
function verifyResult(intent: WorkingIntent, value: VersionMutationResult) {
  const target = intent.kind === 'rebase' ? intent.sourceVersionId : intent.body.targetVersionId;
  if (!value || value.operationId !== intent.body.operationId || value.documentId !== intent.documentId || value.targetVersionId !== target
    || value.resultingRevision !== intent.body.expectedRevision + 1 || !Number.isInteger(value.versionNo)
    || !(value.baseVersionId === null || typeof value.baseVersionId === 'string')
    || (intent.kind === 'rebase' && value.baseVersionId !== intent.currentVersionId)) throw new Error('保存結果の対象を照合できません。');
}
export async function refreshWorkingQueries(client: QueryClient, documentId: string) {
  await Promise.all([refreshLifecycleQueries(client, documentId), client.invalidateQueries({ queryKey: ['document-edit-manifest', documentId] }, { throwOnError: true })]);
}
export async function runWorkingOperation(client: QueryClient, proposed: WorkingIntent): Promise<void> {
  const key = workingOperationKey(proposed.documentId);
  const previous = client.getQueryData<WorkingOperation | null>(key);
  if (previous?.status === 'pending' || previous?.status === 'rejected') return;
  const wasUnknown = previous?.status === 'unknown';
  const intent = wasUnknown ? previous.intent : proposed;
  client.setQueryDefaults(key, { gcTime: Infinity });
  client.setQueryData<WorkingOperation>(key, { intent, status: 'pending' }); guardUnresolved(client);
  try {
    const value = intent.kind === 'create' ? await documentApi.createVersion(intent.documentId, intent.body, intent.files, intent.prepared)
      : intent.kind === 'update' ? await documentApi.updateWorkingVersion(intent.documentId, intent.sourceVersionId, intent.body, intent.files, intent.prepared)
        : await documentApi.rebaseWorkingVersion(intent.documentId, intent.sourceVersionId, intent.body);
    verifyResult(intent, value);
    client.setQueryData<WorkingOperation>(key, { intent, status: 'succeeded', message: intent.kind === 'create' ? '新しい作業版を作成しました。'
      : intent.kind === 'update' ? '作業版を保存しました。' : '作業版の基準を現行の公開版へ更新しました。' });
  } catch (error) {
    client.setQueryData<WorkingOperation>(key, { intent, status: !wasUnknown && wasRejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  await refreshWorkingQueries(client, intent.documentId).catch(() => undefined);
}
