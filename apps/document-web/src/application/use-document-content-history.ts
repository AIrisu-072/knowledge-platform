import { useEffect, useMemo } from 'react';
import { skipToken, useInfiniteQuery, useQuery, useQueryClient, type QueryClient, type InfiniteData } from '@tanstack/react-query';
import { documentApi, type Version, type VersionDetail, type FileList, type VersionList } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

const pagesKey = (documentId: string) => ['document-versions', documentId, 'history', 'pages'] as const;
const refusalKey = (documentId: string) => ['document-content-history-refusal', documentId] as const;
const detailKey = (documentId: string, versionId: string) => ['document-version', documentId, versionId, 'history'] as const;
const filesKey = (documentId: string, versionId: string) => ['document-version-files', documentId, versionId, 'history'] as const;
type Refusal = { error: unknown } | null;

function historyReadKey(key: readonly unknown[], documentId: string) {
  return key[1] === documentId && (key[0] === 'document-versions' && key[2] === 'history'
    || (key[0] === 'document-version' || key[0] === 'document-version-files') && key[3] === 'history');
}
function discardContentHistoryReads(client: QueryClient, documentId: string) {
  const filter = { predicate: (query: { queryKey: readonly unknown[] }) => historyReadKey(query.queryKey, documentId) };
  void client.cancelQueries(filter, { revert: false });
  client.removeQueries(filter);
}
function stopContentHistoryReads(client: QueryClient, documentId: string, error: unknown) {
  // 拒否だけを別keyに保持し、通常のread resetでも明示再読取を省略させない。
  client.setQueryData<Refusal>(refusalKey(documentId), () => ({ error }));
  discardContentHistoryReads(client, documentId);
}
export function denyDocumentContentHistoryReads(client: QueryClient, documentId: string, error: unknown) {
  const problem = problemFromUnknown(error);
  if (problem && [401, 403, 404].includes(problem.status)) stopContentHistoryReads(client, documentId, error);
}
function selectionReadable(client: QueryClient, documentId: string, versionId: string) {
  const list = client.getQueryState<InfiniteData<VersionList>>(pagesKey(documentId));
  return !client.getQueryData<Refusal>(refusalKey(documentId)) && Boolean(list?.data) && !list?.isInvalidated
    && !(list?.fetchStatus === 'fetching' && list.fetchMeta?.fetchMore?.direction !== 'forward')
    && Boolean(list?.data?.pages.some(page => page.items.some(version => version.versionId === versionId)));
}
function retryable(error: unknown) {
  const problem = problemFromUnknown(error);
  return !problem || problem.status >= 500 && problem.retryable;
}

export function useDocumentContentHistory(documentId: string, isDocumentReadable?: () => boolean) {
  const client = useQueryClient();
  const refusal = useQuery<Refusal>({ queryKey: refusalKey(documentId), queryFn: skipToken, initialData: null, gcTime: Infinity });
  const queryKey = pagesKey(documentId);
  useEffect(() => () => discardContentHistoryReads(client, documentId), [client, documentId]);
  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam, signal }) => {
      try { return await documentApi.listDocumentVersions(documentId, 'history', pageParam); }
      catch (error) {
        const adding = client.getQueryState(queryKey)?.fetchMeta?.fetchMore?.direction === 'forward';
        if (!signal.aborted && !(adding && pageParam !== undefined && retryable(error))) stopContentHistoryReads(client, documentId, error);
        throw error;
      }
    },
    getNextPageParam: last => last.nextCursor ?? undefined,
    retry: false,
    enabled: () => !refusal.data && isDocumentReadable?.() !== false,
  });
  const continuationError = query.isFetchNextPageError && retryable(query.error);
  const readable = !refusal.data && (!query.error || continuationError) && !(query.isFetching && !query.isFetchingNextPage);
  const versions = useMemo(() => {
    const unique = new Map<string, Version>();
    if (readable) for (const page of query.data?.pages ?? []) for (const version of page.items) {
      if (!unique.has(version.versionId)) unique.set(version.versionId, version);
    }
    return [...unique.values()];
  }, [query.data, readable]);
  const ready = readable && Boolean(query.data);
  const canContinue = ready && Boolean(query.hasNextPage);
  function loadMore() {
    if (!canContinue || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void query.fetchNextPage({ cancelRefetch: false });
  }
  function restart() {
    if (client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void client.cancelQueries({ queryKey, exact: true }, { revert: false });
    // 一覧を先に未取得へ戻し、旧選択のobserverがdetailを再送しないようにする。
    void client.resetQueries({ queryKey, exact: true });
    for (const prefix of ['document-version', 'document-version-files']) {
      const filter = { predicate: (query: { queryKey: readonly unknown[] }) => query.queryKey[0] === prefix && historyReadKey(query.queryKey, documentId) };
      void client.cancelQueries(filter, { revert: false });
      client.removeQueries(filter);
    }
    client.setQueryData<Refusal>(refusalKey(documentId), null);
  }
  return { versions, ready, busy: query.isFetching, initialLoading: query.isPending && !refusal.data,
    adding: query.isFetchingNextPage, error: refusal.data?.error ?? query.error,
    continuationError, canContinue, loadMore, restart };
}

export function useContentHistoryVersion(documentId: string, versionId: string) {
  const client = useQueryClient();
  const versionKey = useMemo(() => detailKey(documentId, versionId), [documentId, versionId]);
  const versionFilesKey = useMemo(() => filesKey(documentId, versionId), [documentId, versionId]);
  useEffect(() => () => {
    for (const queryKey of [versionKey, versionFilesKey]) {
      void client.cancelQueries({ queryKey, exact: true }, { revert: false });
      client.removeQueries({ queryKey, exact: true });
    }
  }, [client, versionKey, versionFilesKey]);
  const detail = useQuery({
    queryKey: versionKey,
    queryFn: async ({ signal }) => {
      try {
        const result = await documentApi.getDocumentVersion(documentId, versionId, 'history');
        if (result.versionId !== versionId) throw { type: 'about:blank', title: 'Version changed', status: 409,
          code: 'STALE_VERSION', traceId: '', retryable: false };
        return result;
      } catch (error) { if (!signal.aborted) stopContentHistoryReads(client, documentId, error); throw error; }
    },
    retry: false,
    enabled: () => selectionReadable(client, documentId, versionId),
  });
  const files = useQuery({
    queryKey: versionFilesKey,
    queryFn: async ({ signal }) => {
      try { return await documentApi.listVersionFiles(documentId, versionId, 'history'); }
      catch (error) { if (!signal.aborted) stopContentHistoryReads(client, documentId, error); throw error; }
    },
    retry: false,
    enabled: () => detail.isSuccess && !detail.isFetching && selectionReadable(client, documentId, versionId),
  });
  const ready = detail.isSuccess && !detail.isFetching && files.isSuccess && !files.isFetching;
  return { detail: detail.isSuccess && !detail.isFetching ? detail.data : undefined,
    files: ready ? files.data : undefined, busy: detail.isPending || detail.isFetching || files.isPending || files.isFetching,
    stop: (error: unknown) => stopContentHistoryReads(client, documentId, error),
    // cache状態は非同期React描画より先に変わるため、Blob保存直前にも同じ対象を検査する。
    downloadTarget: () => {
      const list = client.getQueryState(pagesKey(documentId));
      const version = client.getQueryState<VersionDetail>(versionKey); const originals = client.getQueryState<FileList>(versionFilesKey);
      if (client.getQueryData<Refusal>(refusalKey(documentId)) || !list?.data || list.isInvalidated
        || list.fetchStatus === 'fetching' && list.fetchMeta?.fetchMore?.direction !== 'forward'
        || !version || version.status !== 'success' || version.isInvalidated || version.fetchStatus !== 'idle'
        || !originals || originals.status !== 'success' || originals.isInvalidated || originals.fetchStatus !== 'idle') return null;
      return { version: version.data, files: originals.data };
    },
  };
}
