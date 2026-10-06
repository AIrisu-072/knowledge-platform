import { useEffect, useMemo } from 'react';
import { hashKey, useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { documentApi, type RevisionComparisonResponse } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';
import { denyDocumentRevisionReads } from './use-document-revisions';

type Continuation = { cursor: string; header: string } | undefined;

// A page carries fragments and unverified regions, but its comparison facts must
// describe the same result. Each disclosure has its own audit IDs.
function comparisonHeader(page: RevisionComparisonResponse): string {
  return hashKey([page.projection, page.pageSize, page.baseRevision, page.targetRevision,
    page.contentComparisonStatus, page.verdict, page.coverage, page.resultDigest,
    page.metadataComparisonStatus, page.baseMetadataSnapshotDigest,
    page.targetMetadataSnapshotDigest, page.metadataChanges]);
}

function canRetryContinuation(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  return !problem || problem.status >= 500 && problem.retryable;
}

export function useDocumentComparison(documentId: string, baseRevisionId: string | undefined,
  targetRevisionId: string | undefined, enabled: boolean) {
  const client = useQueryClient();
  // Existing document/pair invalidation prefixes cover this separate page cache.
  const queryKey = useMemo(() => ['revision-comparison', documentId, baseRevisionId,
    targetRevisionId, 'display', 50, 'pages'] as const, [documentId, baseRevisionId, targetRevisionId]);
  useEffect(() => () => {
    const state = client.getQueryState(queryKey);
    void client.cancelQueries({ queryKey, exact: true }, { revert: false });
    // Pair/document changes and route exits discard the page sequence. Preserve
    // terminal errors so returning cannot bypass an explicit restart or refusal.
    if (!state?.error || state.fetchMeta?.fetchMore?.direction === 'forward' && canRetryContinuation(state.error)) {
      client.removeQueries({ queryKey, exact: true });
    }
  }, [client, queryKey]);
  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as Continuation,
    queryFn: async ({ pageParam, signal }) => {
      try {
        const page = await documentApi.compareDocumentRevisions(documentId, {
          baseRevisionId: baseRevisionId!, targetRevisionId: targetRevisionId!,
          projection: 'display', pageSize: 50,
          ...(pageParam ? { cursor: pageParam.cursor } : {}),
        });
        if (page.baseRevision.revisionId !== baseRevisionId || page.targetRevision.revisionId !== targetRevisionId
          || page.projection !== 'display' || page.pageSize !== 50
          || pageParam && comparisonHeader(page) !== pageParam.header) {
          throw { type: 'about:blank', title: 'Comparison input changed', status: 409,
            code: 'STALE_COMPARISON_INPUT', traceId: '', retryable: false };
        }
        return page;
      } catch (error) {
        // A cancelled older request cannot invalidate a newer pair or fresh read.
        if (!signal.aborted) denyDocumentRevisionReads(client, documentId, error);
        throw error;
      }
    },
    getNextPageParam: (lastPage, pages): Continuation => typeof lastPage.nextCursor === 'string' && lastPage.nextCursor.length > 0
      ? { cursor: lastPage.nextCursor, header: comparisonHeader(pages[0]!) } : undefined,
    retry: false,
    // Refused/stale reads stay blocked. A retryable continuation remains active
    // so existing metadata/publication invalidation can read a fresh head.
    enabled: (current) => enabled && (!current.state.error
      || current.state.fetchMeta?.fetchMore?.direction === 'forward' && canRetryContinuation(current.state.error)),
  });
  const continuationError = query.isFetchNextPageError && canRetryContinuation(query.error);
  const busy = query.isFetching;
  // A refetch rebuilds the loaded sequence atomically from its fresh first page.
  // Never show its new header alongside an earlier tail or disclose stale pages.
  const readable = enabled && (!query.error || continuationError) && !(busy && !query.isFetchingNextPage);
  const comparison = useMemo(() => {
    const pages = query.data?.pages;
    if (!readable || !pages?.length) return undefined;
    return { ...pages[0]!, displayItems: pages.flatMap(page => page.displayItems),
      unverifiedRegions: pages.flatMap(page => page.unverifiedRegions), nextCursor: pages.at(-1)!.nextCursor };
  }, [query.data, readable]);
  const canContinue = Boolean(readable && query.hasNextPage);
  function loadMore() {
    if (!canContinue || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void query.fetchNextPage({ cancelRefetch: false });
  }
  function restart() {
    if (!enabled || client.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    void client.cancelQueries({ queryKey, exact: true }, { revert: false });
    void client.resetQueries({ queryKey, exact: true });
  }
  return { comparison, busy, initialLoading: query.isPending, adding: query.isFetchingNextPage,
    error: query.error, continuationError, canContinue, loadMore, restart };
}

export type DocumentComparisonRead = ReturnType<typeof useDocumentComparison>;
