import { invalidateDocumentReadStateViews } from './document-view-navigation';
import type { QueryClient } from '@tanstack/react-query';
import { documentApi, type CommandsWithdrawVersion, type CommandsEndPublication, type WithdrawResult, type EndPublicationResult } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

export type LifecycleIntent = { documentId: string; title: string; versionNo: number } & (
  | { kind: 'withdraw'; versionId: string; body: CommandsWithdrawVersion }
  | { kind: 'end'; body: CommandsEndPublication }
);
export type LifecycleOperation = { intent: LifecycleIntent; status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; message?: string };
// An immutable client command, not a copy of authoritative document state. Retain it in this
// QueryClient across route unmounts, including Back/Forward. Never persist the user's reason.
export const lifecycleKey = (documentId: string) => ['document-lifecycle-operation', documentId] as const;
const unloadGuards = new WeakMap<QueryClient, () => void>();
function guardUnresolvedOperations(client: QueryClient) {
  if (unloadGuards.has(client)) return;
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const cleanup = () => { window.removeEventListener('beforeunload', warn); unsubscribe(); unloadGuards.delete(client); };
  const check = () => {
    const unresolved = client.getQueriesData<LifecycleOperation | null>({ queryKey: ['document-lifecycle-operation'] })
      .some(([, value]) => value?.status === 'pending' || value?.status === 'unknown');
    if (!unresolved) cleanup();
  };
  const unsubscribe = client.getQueryCache().subscribe(check);
  unloadGuards.set(client, cleanup);
  window.addEventListener('beforeunload', warn);
  check();
}
const documentQueries = ['document', 'document-versions', 'document-version', 'document-revisions', 'document-history', 'document-version-files', 'document-access-policy', 'revision-comparison', 'document-read-opening', 'document-current-read-state'];
export async function refreshLifecycleQueries(client: QueryClient, documentId: string) {
  invalidateDocumentReadStateViews(client, documentId);
  await Promise.all([
    ...documentQueries.map(key => client.invalidateQueries({ queryKey: [key, documentId] }, { throwOnError: true })),
    client.invalidateQueries({ queryKey: ['documents'] }, { throwOnError: true }),
  ]);
}
function wasRejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  if (!problem || problem.exactRetry) return false;
  const known: Record<string, number> = { AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404,
    DOCUMENT_VERSION_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, STALE_VERSION: 409,
    RESERVED_DOCUMENT: 409, VALIDATION_FAILED: 422, BUSINESS_RULE_REJECTED: 422, PUBLISH_QUALITY_REJECTED: 422 };
  return known[problem.code] === problem.status;
}
function verifiedMessage(intent: LifecycleIntent, value: unknown): string {
  if (!value || typeof value !== 'object') throw new Error('操作結果を照合できません。');
  const result = value as WithdrawResult & EndPublicationResult;
  if (result.operationId !== intent.body.operationId || result.documentId !== intent.documentId) throw new Error('操作結果の対象が一致しません。');
  if (intent.kind === 'end') {
    if (result.formerCurrentVersionId !== intent.body.expectedCurrentVersionId || result.resultingCurrentVersionId !== null
      || result.resultingDocumentRevision !== intent.body.expectedRevision + 1 || !Number.isFinite(Date.parse(result.endedAt))) throw new Error('公開終了結果を照合できません。');
    return '文書の公開を終了しました。原本と過去版は保持されています。';
  }
  if (result.targetVersionId !== intent.versionId || result.resultingRevision !== intent.body.expectedRevision + 1
    || ![result.formerCurrentVersionId, result.resultingCurrentVersionId].every(id => id === null || typeof id === 'string')
    || !(result.restorationWithheldReason === null || typeof result.restorationWithheldReason === 'string')) throw new Error('取下げ結果を照合できません。');
  const outcome = result.resultingCurrentVersionId === null ? '現行の公開版はありません。'
    : result.resultingCurrentVersionId === result.formerCurrentVersionId ? '現行の公開版は変わりません。' : '直前の公開版へ復帰しました。';
  return `版を取下げました。${outcome}${result.restorationWithheldReason ? '復帰候補の安全性を確認できなかったため、復帰していません。' : ''}`;
}
export async function runLifecycleOperation(client: QueryClient, proposed: LifecycleIntent): Promise<void> {
  const key = lifecycleKey(proposed.documentId);
  const previous = client.getQueryData<LifecycleOperation | null>(key);
  if (previous?.status === 'pending' || previous?.status === 'rejected') return;
  const intent = previous?.status === 'unknown' ? previous.intent : proposed;
  const wasUnknown = previous?.status === 'unknown';
  // Synchronous cache write prevents two submit events before React rerenders.
  client.setQueryData<LifecycleOperation>(key, { intent, status: 'pending' });
  guardUnresolvedOperations(client);
  try {
    const result = intent.kind === 'withdraw'
      ? await documentApi.withdrawVersion(intent.documentId, intent.versionId, intent.body)
      : await documentApi.endDocumentPublication(intent.documentId, intent.body);
    client.setQueryData<LifecycleOperation>(key, { intent, status: 'succeeded', message: verifiedMessage(intent, result) });
  } catch (error) {
    client.setQueryData<LifecycleOperation>(key, { intent, status: !wasUnknown && wasRejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  // A post-commit read failure must never turn a verified success into a mutation retry.
  await refreshLifecycleQueries(client, intent.documentId).catch(() => undefined);
}
