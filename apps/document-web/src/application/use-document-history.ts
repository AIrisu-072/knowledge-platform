import { useEffect, useMemo } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { documentApi, type History } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

const pagesKey = (documentId: string) => ['document-history', documentId, 'pages'] as const;
// A reset of content after a move must not erase a refusal. This read barrier
// contains no history rows and is cleared only by the explicit restart control.
const refusalKey = (documentId: string) => ['document-history-refusal', documentId] as const;
type Refusal = { error: unknown } | null;

export function denyDocumentHistoryReads(client: QueryClient, documentId: string, error: unknown) {
  const problem = problemFromUnknown(error);
  if (!problem || ![401, 403, 404].includes(problem.status)) return;
  client.setQueryData<Refusal>(refusalKey(documentId), () => ({ error }));
  void client.cancelQueries({ queryKey: pagesKey(documentId), exact: true }, { revert: false });
  client.removeQueries({ queryKey: pagesKey(documentId), exact: true });
}

function canRetryContinuation(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  return !problem || problem.status >= 500 && problem.retryable;
}

export function useDocumentHistory(documentId: string, enabled: boolean, isDocumentReadable?: () => boolean) {
  const client = useQueryClient();
  const queryKey = useMemo(() => pagesKey(documentId), [documentId]);
  const refusal = useQuery<Refusal>({ queryKey: refusalKey(documentId), initialData: null,
    enabled: false, staleTime: Infinity, gcTime: Infinity });
  useEffect(() => {
    if (!enabled) return;
    return () => {
      // Successful pages/cursors expire on tab, document and route exit. The
      // independent refusal survives navigation until a deliberate fresh read.
      void client.cancelQueries({ queryKey, exact: true }, { revert: false });
      client.removeQueries({ queryKey, exact: true });
    };
  }, [client, queryKey, enabled]);
  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam, signal }) => {
      try { return await documentApi.getDocumentHistory(documentId, pageParam); }
      catch (error) {
        // Aborted old requests cannot block a restarted or different read.
        if (!signal.aborted && (pageParam === undefined || !canRetryContinuation(error))) {
          client.setQueryData<Refusal>(refusalKey(documentId), () => ({ error }));
        }
        throw error;
      }
    },
    getNextPageParam: lastPage => lastPage.nextCursor ?? undefined,
    retry: false,
    // Retryable continuation errors remain active for existing invalidations.
    // All other failures require the explicit restart, including after reset.
    enabled: () => enabled && isDocumentReadable?.() !== false && !client.getQueryData<Refusal>(refusalKey(documentId)),
  });
  const error = refusal.data?.error ?? query.error;
  const continuationError = !refusal.data && query.isFetchNextPageError && canRetryContinuation(error);
  const busy = query.isFetching;
  const readable = enabled && (!error || continuationError) && !(busy && !query.isFetchingNextPage);
  const entries = useMemo(() => {
    const unique = new Map<string, History['items'][number]>();
    if (readable) for (const page of query.data?.pages ?? []) {
      // Offset pages can overlap after concurrent writes. Keep server order and
      // the first record of each tuple; sourceKey alone is not its identity.
      for (const entry of page.items) {
        const identity = JSON.stringify([entry.sourceKind, entry.sourceKey]);
        if (!unique.has(identity)) unique.set(identity, entry);
      }
    }
    return [...unique.values()];
  }, [query.data, readable]);
  const canContinue = Boolean(readable && query.hasNextPage);
  function loadMore() {
    if (!canContinue || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void query.fetchNextPage({ cancelRefetch: false });
  }
  function restart() {
    if (!enabled || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void client.cancelQueries({ queryKey, exact: true }, { revert: false });
    client.setQueryData<Refusal>(refusalKey(documentId), null);
    void client.resetQueries({ queryKey, exact: true });
  }
  return { entries, error, busy, initialLoading: !error && query.isPending,
    empty: readable && query.isSuccess && !entries.length && !query.hasNextPage,
    adding: query.isFetchingNextPage, continuationError, canContinue, loadMore, restart };
}

export type DocumentHistoryRead = ReturnType<typeof useDocumentHistory>;
