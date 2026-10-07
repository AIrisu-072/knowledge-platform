import { useEffect, useRef, useSyncExternalStore, type RefObject } from 'react';
import { skipToken, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { documentApi, type CurrentReadState, type DocumentDetail } from './document-workspace';
import { currentReadStateKey, documentReadStateOperations, maximumReadStateMessage, sendReadStateOperation, validCurrentReadState, type ReadStateIntent } from './document-read-state';
import { documentViewNavigation, invalidateDocumentReadStateViews, isOverviewVisible } from './document-view-navigation';
import { createOperationId } from './operation-id';
import { mapApiProblem, problemFromUnknown } from './problem-mapping';

function normalReadsReady(client: QueryClient, documentId: string, versionId: string): boolean {
  const detail = client.getQueryData<DocumentDetail>(['document', documentId, 'published']);
  const version = client.getQueryData<{ versionId: string }>(['document-version', documentId, versionId, 'published']);
  if (detail?.documentId !== documentId || detail.currentVersionId !== versionId || detail.displayVersion.versionId !== versionId
    || detail.displayVersion.lifecycleState !== 'PUBLISHED' || version?.versionId !== versionId) return false;
  return [['document', documentId, 'published'], ['document-version', documentId, versionId, 'published'], ['document-version-files', documentId, versionId, 'published']]
    .every(key => { const query = client.getQueryState(key); return query?.status === 'success' && query.fetchStatus === 'idle' && !query.isInvalidated; });
}
async function recheckDocument(client: QueryClient, documentId: string): Promise<void> {
  const key = ['document', documentId, 'published'];
  const queryFn = client.getQueryCache().find({ queryKey: key, exact: true })?.options.queryFn;
  if (typeof queryFn === 'function') {
    await client.fetchQuery({ queryKey: key, queryFn, staleTime: 0, retry: false }).catch(() => undefined);
  }
}
export async function refreshCurrentReadState(client: QueryClient, target: Pick<ReadStateIntent, 'documentId' | 'versionId'>): Promise<void> {
  const owner = documentViewNavigation(client); owner.revokeCurrent(target.documentId);
  const key = currentReadStateKey(target.documentId, target.versionId);
  await client.cancelQueries({ queryKey: key, exact: true }, { revert: false });
  try {
    await client.fetchQuery({ queryKey: key, staleTime: 0, retry: false, queryFn: async ({ signal }) => {
      const value = await documentApi.getCurrentDocumentVersionReadState(target.documentId, target.versionId, { signal });
      if (signal.aborted || !validCurrentReadState(value, target.documentId, target.versionId)) throw new Error('現在の既読状態を確認できません。');
      return value;
    } });
    owner.qualify(target.documentId, target.versionId, client.getQueryData<CurrentReadState>(key));
  } catch (error) {
    if ([401, 404, 409].includes(problemFromUnknown(error)?.status ?? 0)) await recheckDocument(client, target.documentId);
    throw error;
  }
}
async function refreshOperationReads(client: QueryClient, intent: ReadStateIntent): Promise<void> {
  invalidateDocumentReadStateViews(client, intent.documentId);
  await Promise.all([
    refreshCurrentReadState(client, intent),
    ...['document', 'document-version', 'document-versions'].map(name => client.invalidateQueries({ queryKey: [name, intent.documentId] }, { throwOnError: true })),
    client.invalidateQueries({ queryKey: ['documents'] }, { throwOnError: true }),
  ]);
}
export function retryReadStateOperation(client: QueryClient, intent: ReadStateIntent): Promise<void> {
  return sendReadStateOperation({ store: documentReadStateOperations(client), intent,
    send: fixed => fixed.kind === 'VIEW' ? documentApi.recordDocumentVersionView(fixed.documentId, fixed.versionId, fixed.body) : documentApi.resetDocumentVersionReadState(fixed.documentId, fixed.versionId, fixed.body),
    invalidate: fixed => refreshOperationReads(client, fixed) });
}

export function useDocumentViewReadState({ document: detail, view, activeTab, workflow, detailReady, filesReady, overviewRef }: {
  document: DocumentDetail | undefined;
  view: 'published' | 'authoring';
  activeTab: string;
  workflow?: string;
  detailReady: boolean;
  filesReady: boolean;
  overviewRef: RefObject<HTMLElement | null>;
}) {
  const client = useQueryClient(); const owner = documentViewNavigation(client); const store = documentReadStateOperations(client);
  const opening = useSyncExternalStore(owner.subscribe, owner.get);
  const documentId = detail?.documentId ?? ''; const versionId = detail?.currentVersionId ?? '';
  const queryKey = currentReadStateKey(documentId, versionId);
  // The existing Query observer schedules read changes. Do not subscribe React directly to every cache update.
  const currentQuery = useQuery<CurrentReadState>({ queryKey, queryFn: skipToken, enabled: false });
  const qualified = useSyncExternalStore(owner.subscribe, () => owner.current(documentId, versionId));
  const operation = useSyncExternalStore(store.subscribe, () => store.get(documentId, versionId));
  const currentCache = client.getQueryState<CurrentReadState>(queryKey);
  const eligible = Boolean(detail && view === 'published' && activeTab === 'overview' && !workflow && versionId
    && detail.displayVersion.versionId === versionId && detail.displayVersion.lifecycleState === 'PUBLISHED');
  const currentState = eligible && detailReady && qualified === currentQuery.data && validCurrentReadState(qualified, documentId, versionId)
    && currentCache?.status === 'success' && currentCache.fetchStatus === 'idle' && !currentCache.isInvalidated && normalReadsReady(client, documentId, versionId)
    ? qualified : undefined;
  const latestProps = useRef({ detail, eligible, detailReady, filesReady }); latestProps.current = { detail, eligible, detailReady, filesReady };

  useEffect(() => {
    const live = owner.get();
    if (!eligible || !detailReady || !detail || !live || live.documentId !== documentId || live.suppressed || live.consumed) return;
    if (live.versionId && live.versionId !== versionId) { invalidateDocumentReadStateViews(client, documentId); return; }
    if (live.started) return;
    const fixedOperation = store.get(documentId, versionId);
    if (fixedOperation?.status === 'pending' || fixedOperation?.status === 'unknown') { owner.suppress(documentId); return; }
    const abort = new AbortController(); owner.revokeCurrent(documentId); owner.update(live, { versionId, abort, started: true });
    const fixed = owner.get()!;
    void client.fetchQuery({ queryKey: ['document-read-opening', documentId, versionId, fixed.openId], staleTime: 0, gcTime: 0, retry: false,
      queryFn: async ({ signal }) => {
        const cancel = () => abort.abort(); signal.addEventListener('abort', cancel, { once: true });
        try {
          const value = await documentApi.getCurrentDocumentVersionReadState(documentId, versionId, { signal: abort.signal });
          if (abort.signal.aborted || !validCurrentReadState(value, documentId, versionId)) throw new Error('現在の既読状態を確認できません。');
          return Object.freeze({ ...value });
        } finally { signal.removeEventListener('abort', cancel); }
      },
    }).then(token => {
      const active = owner.get();
      if (!active || active.openId !== fixed.openId || active.epoch !== fixed.epoch || active.suppressed || abort.signal.aborted) return;
      const cached = client.setQueryData<CurrentReadState>(queryKey, token)!;
      owner.qualify(documentId, versionId, cached); owner.update(active, { token });
    }).catch(error => {
      const active = owner.get();
      if (!active || active.openId !== fixed.openId || active.epoch !== fixed.epoch || abort.signal.aborted) return;
      owner.update(active, { error }); owner.suppress(documentId); owner.revokeCurrent(documentId);
      if ([401, 404, 409].includes(problemFromUnknown(error)?.status ?? 0)) void recheckDocument(client, documentId);
    });
  }, [client, detail, detailReady, documentId, eligible, opening, owner, store, versionId]);

  function onPublishedDetailDisplayed(openId: string): void {
    const live = owner.get(); const input = latestProps.current;
    if (!live || live.openId !== openId || live.suppressed || live.consumed || live.documentId !== documentId || live.versionId !== versionId
      || !validCurrentReadState(live.token, documentId, versionId) || !input.eligible || !input.detailReady || !input.filesReady
      || !owner.viewing(documentId) || !normalReadsReady(client, documentId, versionId) || !isOverviewVisible(overviewRef.current)) return;
    const previous = store.get(documentId, versionId);
    if (previous?.status === 'pending' || previous?.status === 'unknown') return;
    owner.update(live, { consumed: true });
    if (live.token.isRead || live.token.readStateRevision >= Number.MAX_SAFE_INTEGER) return;
    if (previous) store.clearSettled(documentId, versionId, previous);
    void retryReadStateOperation(client, { documentId, versionId, title: input.detail!.title, versionNo: input.detail!.displayVersion.versionNo,
      kind: 'VIEW', body: { operationId: createOperationId(), expectedReadStateRevision: live.token.readStateRevision } });
  }
  useEffect(() => {
    if (!eligible || !detailReady || !filesReady || !opening?.token || opening.suppressed || opening.consumed) return;
    const frame = requestAnimationFrame(() => onPublishedDetailDisplayed(opening.openId));
    const visible = () => { if (document.visibilityState === 'visible') onPublishedDetailDisplayed(opening.openId); };
    document.addEventListener('visibilitychange', visible);
    return () => { cancelAnimationFrame(frame); document.removeEventListener('visibilitychange', visible); };
  }, [eligible, detailReady, filesReady, opening, currentQuery.data]);
  useEffect(() => {
    const live = owner.get();
    if (eligible && live?.documentId === documentId && live.versionId && live.versionId !== versionId && !live.suppressed) invalidateDocumentReadStateViews(client, documentId);
  }, [client, documentId, eligible, opening, owner, versionId]);

  const unresolved = operation?.status === 'pending' || operation?.status === 'unknown';
  const maximum = Boolean(currentState && currentState.readStateRevision >= Number.MAX_SAFE_INTEGER);
  const canReset = Boolean(currentState?.isRead && !maximum && !unresolved);
  async function resetUnread(): Promise<void> {
    const liveQuery = client.getQueryState<CurrentReadState>(queryKey); const state = liveQuery?.data; const live = owner.get(); const previous = store.get(documentId, versionId);
    if (!canReset || !currentState || liveQuery?.status !== 'success' || liveQuery.fetchStatus !== 'idle' || liveQuery.isInvalidated
      || state !== currentState || owner.current(documentId, versionId) !== currentState || !validCurrentReadState(state, documentId, versionId)
      || !state.isRead || state.readStateRevision >= Number.MAX_SAFE_INTEGER || !owner.viewing(documentId) || !normalReadsReady(client, documentId, versionId)
      || live?.openId !== opening?.openId || live?.epoch !== opening?.epoch || previous?.status === 'pending' || previous?.status === 'unknown') return;
    owner.suppress(documentId);
    if (previous) store.clearSettled(documentId, versionId, previous);
    await retryReadStateOperation(client, { documentId, versionId, title: detail!.title, versionNo: detail!.displayVersion.versionNo,
      kind: 'RESET', body: { operationId: createOperationId(), expectedReadStateRevision: state.readStateRevision } });
  }
  async function refreshCurrentState(): Promise<void> {
    if (!eligible) return;
    owner.suppress(documentId); const live = owner.get(); if (live?.documentId === documentId) owner.update(live, { error: undefined });
    await refreshCurrentReadState(client, { documentId, versionId });
  }
  const error = currentState ? undefined : (opening?.documentId === documentId ? opening.error : undefined) ?? currentQuery.error;
  const problem = problemFromUnknown(error);
  return {
    currentState, unconfirmed: !currentState,
    loading: Boolean(eligible && opening?.started && !opening.token && !opening.error && !opening.suppressed) || currentQuery.isFetching,
    error, errorMessage: maximum ? maximumReadStateMessage : problem ? mapApiProblem(problem).message : error ? '現在の既読状態を確認できません。' : undefined,
    canReset, resetUnread, refreshCurrentState,
    retryOperation: async () => { const fixed = store.get(documentId, versionId); if (fixed?.status === 'unknown') await retryReadStateOperation(client, fixed.intent); },
  };
}
