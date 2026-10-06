import { useMemo } from 'react';
import { useInfiniteQuery, useQueryClient, type InfiniteData, type QueryClient } from '@tanstack/react-query';
import { documentApi, type DocumentRevisionSummary, type DocumentRevisionPage } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

type RevisionPages = InfiniteData<DocumentRevisionPage> & { denial?: NonNullable<ReturnType<typeof problemFromUnknown>> };
const revisionPagesKey = (documentId: string) => ['document-revisions', documentId, 'pages'] as const;

// Capture a refusal at the API boundary, before automatic retries or navigation
// can replace its error. The denial belongs to this document's page sequence,
// so a different pair or remounted route cannot revive an older successful read.
export function denyDocumentRevisionReads(client: QueryClient, documentId: string, error: unknown) {
  const problem = problemFromUnknown(error);
  if (!problem || ![401, 403, 404].includes(problem.status)) return;
  const queryKey = revisionPagesKey(documentId);
  void client.cancelQueries({ queryKey, exact: true }, { revert: false });
  client.setQueryData<RevisionPages>(queryKey, { pages: [], pageParams: [], denial: problem });
  void client.cancelQueries({ queryKey: ['revision-comparison', documentId] }, { revert: false });
  client.removeQueries({ queryKey: ['revision-comparison', documentId] });
}

function requiresRestart(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  return Boolean(problem && ([401, 403, 404].includes(problem.status)
    || problem.code === 'CURSOR_STALE' || problem.code === 'VALIDATION_FAILED'));
}

export function useDocumentRevisions(documentId: string, enabled: boolean) {
  const client = useQueryClient();
  // Keep infinite pages separate from ordinary single-page reads. Existing read
  // invalidation/reset prefixes still cover this document's entire page sequence.
  const queryKey = revisionPagesKey(documentId);
  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => documentApi.listDocumentRevisions(documentId, pageParam),
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined,
    // Continuation failures are recovered by the explicit retry/restart controls.
    // In particular, never automatically resend a refused or expired cursor.
    retry: false,
    enabled: enabled && !client.getQueryData<RevisionPages>(queryKey)?.denial && !requiresRestart(client.getQueryState(queryKey)?.error),
  });
  const denial = (query.data as RevisionPages | undefined)?.denial;
  const error = denial ?? query.error;
  const problem = problemFromUnknown(error);
  const continuationError = query.isFetchNextPageError && !requiresRestart(error)
    && (!problem || problem.status >= 500 && problem.retryable);
  const readable = !error || continuationError;
  const revisions = useMemo(() => {
    const unique = new Map<string, DocumentRevisionSummary>();
    if (readable) for (const page of query.data?.pages ?? []) {
      for (const revision of page.items) if (!unique.has(revision.revisionId)) unique.set(revision.revisionId, revision);
    }
    return [...unique.values()];
  }, [query.data, readable]);
  const busy = query.isFetching;
  const ready = !denial && query.isSuccess && !(busy && !query.isFetchingNextPage);
  const canContinue = Boolean(query.hasNextPage && readable);
  function loadMore() {
    // Consult the live cache too: two events in one React batch share old props.
    if (!canContinue || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void query.fetchNextPage({ cancelRefetch: false });
  }
  function resetRead() {
    void client.cancelQueries({ queryKey, exact: true }, { revert: false });
    // Discard only comparison reads, including late results, before restarting.
    // Unresolved operations, immutable uploads and provider state stay intact.
    void client.cancelQueries({ queryKey: ['revision-comparison', documentId] }, { revert: false });
    client.removeQueries({ queryKey: ['revision-comparison', documentId] });
    void client.resetQueries({ queryKey, exact: true });
  }
  function restart() {
    if (client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    resetRead();
  }
  return { revisions, ready, busy, error, initialLoading: query.isPending,
    adding: query.isFetchingNextPage, continuationError, canContinue, loadMore, restart };
}

export type DocumentRevisionsRead = ReturnType<typeof useDocumentRevisions>;
