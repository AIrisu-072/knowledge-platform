import { useEffect, useRef, useState } from 'react';
import { hashKey, skipToken, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { documentApi, type DiffDisplayProjection, type DocumentDetail, type VersionDetail, type VersionList } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

const refusalKey = (documentId: string) => ['document-working-comparison-refusal', documentId] as const;
const stale = () => ({ type: 'about:blank', title: 'Comparison input changed', status: 409,
  code: 'STALE_COMPARISON_INPUT', traceId: '', retryable: false });
const documentKey = (id: string) => ['document', id, 'authoring'] as const;
const versionKey = (id: string, versionId: string) => ['document-version', id, versionId, 'authoring'] as const;
const versionsKey = (id: string) => ['document-versions', id, 'authoring'] as const;
function documentFacts(document: DocumentDetail) {
  return [document.documentId, document.revision, document.currentVersionId, document.capabilities.compareVersions];
}
function versionFacts(version: VersionDetail) {
  return [version.versionId, version.versionNo, version.title, version.updatedAt, version.lifecycleState,
    version.isCurrent, version.baseVersionId, version.metadata, version.fileSummary, version.capabilities];
}
function readable<T>(client: QueryClient, key: readonly unknown[]): T | undefined {
  const state = client.getQueryState<T>(key);
  return state?.status === 'success' && !state.error && !state.isInvalidated && state.fetchStatus === 'idle' ? state.data : undefined;
}
export function workingComparisonSource(client: QueryClient, documentId: string, versionId: string) {
  const document = readable<DocumentDetail>(client, documentKey(documentId));
  const working = readable<VersionDetail>(client, versionKey(documentId, versionId));
  const versions = readable<VersionList>(client, versionsKey(documentId));
  if (!document?.currentVersionId || document.documentId !== documentId || document.capabilities.compareVersions?.status !== 'available'
    || working?.versionId !== versionId || working.lifecycleState !== 'working'
    || !versions?.items.some(version => version.versionId === versionId && version.lifecycleState === 'working')) return undefined;
  return { document, working, baseVersionId: document.currentVersionId,
    signature: hashKey([documentFacts(document), versionFacts(working)]) };
}
type Source = NonNullable<ReturnType<typeof workingComparisonSource>>;
type Canonical = { base: VersionDetail; target: VersionDetail; signature: string };
type ReadState = { pages: DiffDisplayProjection[]; canonical?: Canonical; busy: boolean; error?: unknown; continuationError?: boolean };
function header(page: DiffDisplayProjection) { return hashKey([page.projection, page.pageSize, page.verdict, page.coverage, page.resultDigest]); }
function retryable(error: unknown) { const problem = problemFromUnknown(error); return !problem || problem.status >= 500 && problem.retryable; }

// Results remain local to the opened panel. Only refusal survives a close or a
// normal read reset; no Version result can collide with a Revision pair cache.
export function useWorkingComparison(documentId: string, versionId: string) {
  const client = useQueryClient();
  useQuery<{ error: unknown } | null>({ queryKey: refusalKey(documentId), queryFn: skipToken, initialData: null, gcTime: Infinity });
  const [state, setState] = useState<ReadState>({ pages: [], busy: false });
  const live = useRef(state); const generation = useRef(0); const mounted = useRef(false); const refreshing = useRef(false);
  const source = useRef<Source | undefined>(undefined);
  function update(next: ReadState) { live.current = next; if (mounted.current) setState(next); }
  function stop(error: unknown) {
    generation.current++;
    const previous = client.getQueryData<{ error: unknown }>(refusalKey(documentId));
    if (!previous) client.setQueryData(refusalKey(documentId), { error });
    update({ pages: [], busy: false, error: previous?.error ?? error });
  }
  function current(ticket: number, baseline: Source) {
    return mounted.current && ticket === generation.current
      && workingComparisonSource(client, documentId, versionId)?.signature === baseline.signature;
  }
  async function canonical(baseline: Source): Promise<Canonical> {
    const [document, target, base] = await Promise.all([
      documentApi.getDocument(documentId, 'authoring'),
      documentApi.getDocumentVersion(documentId, versionId, 'authoring'),
      documentApi.getDocumentVersion(documentId, baseline.baseVersionId, 'published'),
    ]);
    if (hashKey([documentFacts(document), versionFacts(target)]) !== baseline.signature
      || target.versionId !== versionId || target.lifecycleState !== 'working'
      || base.versionId !== baseline.baseVersionId || base.lifecycleState !== 'published' || !base.isCurrent) throw stale();
    return { base, target, signature: hashKey([documentFacts(document), versionFacts(target), versionFacts(base)]) };
  }
  async function fetchPage(tail: boolean) {
    if (live.current.busy) return;
    const baseline = tail ? source.current : workingComparisonSource(client, documentId, versionId);
    if (!baseline) { stop(stale()); return; }
    const previous = tail ? live.current : { pages: [], busy: false };
    const cursor = previous.pages.at(-1)?.nextCursor;
    if (tail && !cursor) return;
    source.current = baseline; const ticket = ++generation.current;
    update({ ...previous, busy: true, error: undefined, continuationError: false });
    let comparing = false;
    try {
      const before = await canonical(baseline);
      if (!current(ticket, baseline)) return;
      if (previous.canonical && before.signature !== previous.canonical.signature) throw stale();
      comparing = true;
      const page = await documentApi.compareDocumentVersions(documentId, { baseVersionId: baseline.baseVersionId,
        targetVersionId: versionId, profile: 'document-diff-v0', projection: 'display', pageSize: 50, ...(cursor ? { cursor } : {}) });
      comparing = false;
      if (!current(ticket, baseline)) return;
      if (page.projection !== 'display' || page.pageSize !== 50 || previous.pages.length && header(page) !== header(previous.pages[0]!)) throw stale();
      // The API has no expectedRevision or returned pair. Re-read at disclosure,
      // without claiming an atomic current-version guarantee across these calls.
      const after = await canonical(baseline);
      if (!current(ticket, baseline)) return;
      if (after.signature !== before.signature) throw stale();
      update({ pages: [...previous.pages, page], canonical: after, busy: false });
    } catch (error) {
      if (!current(ticket, baseline)) return;
      if (tail && comparing && retryable(error)) update({ ...previous, busy: false, error, continuationError: true });
      else stop(error);
    }
  }
  useEffect(() => {
    mounted.current = true;
    const unsubscribe = client.getQueryCache().subscribe(event => {
      if (refreshing.current || !['updated', 'removed'].includes(event.type)) return;
      const key = event.query.queryKey;
      // All existing metadata/WORKING/publication/move/ACL refresh paths touch
      // these reads. A synchronous generation change rejects even same-tick replies.
      if (key[1] !== documentId || !['document', 'document-versions', 'document-version', 'document-access-policy'].includes(String(key[0]))) return;
      if (key[0] === 'document-versions' && key[2] === 'history' || key[0] === 'document-version' && key[3] === 'history') return;
      stop(event.query.state.error ?? stale());
    });
    const refusal = client.getQueryData<{ error: unknown }>(refusalKey(documentId));
    if (refusal) update({ pages: [], busy: false, error: refusal.error }); else void fetchPage(false);
    return () => { mounted.current = false; generation.current++; unsubscribe(); };
  }, [client, documentId, versionId]);
  async function restart() {
    if (live.current.busy) return;
    const ticket = ++generation.current; refreshing.current = true;
    update({ pages: [], busy: true });
    try {
      await Promise.all([
        client.fetchQuery({ queryKey: documentKey(documentId), queryFn: () => documentApi.getDocument(documentId, 'authoring'), staleTime: 0, retry: false }),
        client.fetchQuery({ queryKey: versionKey(documentId, versionId), queryFn: () => documentApi.getDocumentVersion(documentId, versionId, 'authoring'), staleTime: 0, retry: false }),
        client.fetchQuery({ queryKey: versionsKey(documentId), queryFn: () => documentApi.listDocumentVersions(documentId, 'authoring'), staleTime: 0, retry: false }),
      ]);
      if (!mounted.current || generation.current !== ticket) return;
      client.removeQueries({ queryKey: refusalKey(documentId), exact: true });
      update({ pages: [], busy: false }); refreshing.current = false;
      void fetchPage(false);
    } catch (error) { if (mounted.current && generation.current === ticket) stop(error); }
    finally { refreshing.current = false; }
  }
  const first = state.pages[0];
  const comparison = first ? { ...first, items: state.pages.flatMap(page => page.items), unverifiedRegions: state.pages.flatMap(page => page.unverifiedRegions), nextCursor: state.pages.at(-1)!.nextCursor } : undefined;
  return { ...state, comparison, canContinue: Boolean(comparison?.nextCursor), loadMore: () => void fetchPage(true), restart: () => void restart() };
}
