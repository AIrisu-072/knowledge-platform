import type { QueryClient } from '@tanstack/react-query';
import { documentApi } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

export type ScheduleCancelIntent = {
  documentId: string; versionId: string; title: string; versionNo: number; scheduledPublishAt: string;
  body: { operationId: string; publishOperationId: string; expectedRevision: number };
};
export type ScheduleCancelOperation = { intent: ScheduleCancelIntent; status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown };
// Keep the immutable command in this tab's QueryClient across Back/Forward. This is
// recovery state, never an authoritative reservation or a replacement for current reads.
export const scheduleCancelKey = (documentId: string, versionId: string) => ['document-schedule-cancel', documentId, versionId] as const;
const unloadGuards = new WeakMap<QueryClient, () => void>();
function guardUnknown(client: QueryClient) {
  if (unloadGuards.has(client)) return;
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const cleanup = () => { window.removeEventListener('beforeunload', warn); unsubscribe(); unloadGuards.delete(client); };
  const check = () => {
    if (!client.getQueriesData<ScheduleCancelOperation | null>({ queryKey: ['document-schedule-cancel'] })
      .some(([, value]) => value?.status === 'pending' || value?.status === 'unknown')) cleanup();
  };
  const unsubscribe = client.getQueryCache().subscribe(check);
  unloadGuards.set(client, cleanup); window.addEventListener('beforeunload', warn); check();
}
export async function refreshScheduleQueries(client: QueryClient, documentId: string) {
  await Promise.all([
    ...['document', 'document-version', 'document-versions', 'document-revisions', 'document-history'].map(key => client.invalidateQueries({ queryKey: [key, documentId] }, { throwOnError: true })),
    client.invalidateQueries({ queryKey: ['documents'] }, { throwOnError: true }),
  ]);
}
function rejected(error: unknown) {
  const problem = problemFromUnknown(error);
  if (!problem || problem.exactRetry) return false;
  const codes: Record<string, number> = { AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404,
    DOCUMENT_VERSION_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, STALE_VERSION: 409,
    RESERVED_DOCUMENT: 409, VALIDATION_FAILED: 422, BUSINESS_RULE_REJECTED: 422 };
  return codes[problem.code] === problem.status;
}
export async function runScheduleCancellation(client: QueryClient, proposed: ScheduleCancelIntent) {
  const key = scheduleCancelKey(proposed.documentId, proposed.versionId);
  const previous = client.getQueryData<ScheduleCancelOperation | null>(key);
  if (previous?.status === 'pending' || previous?.status === 'rejected') return;
  const intent = previous?.status === 'unknown' ? previous.intent : proposed;
  // Synchronous cache update also fences a second submit before React rerenders.
  client.setQueryData<ScheduleCancelOperation>(key, { intent, status: 'pending' }); guardUnknown(client);
  try {
    const result = await documentApi.cancelPublicationSchedule(intent.documentId, intent.versionId, intent.body);
    if (result.operationId !== intent.body.operationId || result.publishOperationId !== intent.body.publishOperationId
      || result.documentId !== intent.documentId || result.targetVersionId !== intent.versionId
      || result.resultingRevision !== intent.body.expectedRevision + 1) throw new Error('取消結果の対象を照合できません。');
    client.setQueryData<ScheduleCancelOperation>(key, { intent, status: 'succeeded' });
  } catch (error) {
    client.setQueryData<ScheduleCancelOperation>(key, { intent, status: previous?.status !== 'unknown' && rejected(error) ? 'rejected' : 'unknown', error });
    return;
  }
  // A known committed result stays successful even if the subsequent read fails.
  await refreshScheduleQueries(client, intent.documentId).catch(() => undefined);
}
