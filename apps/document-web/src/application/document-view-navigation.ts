import { notifyManager, type QueryClient } from '@tanstack/react-query';
import type { AnyRouter } from '@tanstack/react-router';

export type DocumentReadOpening = Readonly<{
  openId: string;
  documentId: string;
  epoch: number;
  consumed: boolean;
  suppressed: boolean;
  versionId?: string;
  started?: boolean;
  token?: unknown;
  error?: unknown;
  abort?: AbortController;
}>;

export function isOverviewVisible(element: HTMLElement | null): boolean {
  if (!element?.isConnected || document.visibilityState !== 'visible') return false;
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (node.hidden || node.getAttribute('aria-hidden') === 'true' || style.display === 'none'
      || style.visibility === 'hidden' || style.visibility === 'collapse' || style.opacity === '0'
      || style.contentVisibility === 'hidden') return false;
  }
  return true;
}

function createNavigation() {
  let opening: DocumentReadOpening | undefined;
  let previousDocument: string | undefined;
  let overviewDocument: string | undefined;
  let generation = 0;
  let epoch = 0;
  const listeners = new Set<() => void>();
  const qualifications = new Map<string, { documentId: string; snapshot: unknown }>();
  const key = (documentId: string, versionId: string) => JSON.stringify([documentId, versionId]);
  const notify = notifyManager.batchCalls((listener: () => void) => listener());
  const emit = () => listeners.forEach(listener => notify(listener));
  return {
    get: () => opening,
    viewing: (documentId: string) => overviewDocument === documentId,
    visit: (documentId: string | undefined, eligible: boolean) => { overviewDocument = eligible ? documentId : undefined; },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    current: (documentId: string, versionId: string) => qualifications.get(key(documentId, versionId))?.snapshot,
    qualify: (documentId: string, versionId: string, snapshot: unknown) => {
      qualifications.set(key(documentId, versionId), { documentId, snapshot }); emit();
    },
    revokeCurrent: (documentId?: string) => {
      for (const [target, value] of qualifications) if (!documentId || value.documentId === documentId) qualifications.delete(target);
      emit();
    },
    update: (expected: DocumentReadOpening, patch: Partial<DocumentReadOpening>) => {
      if (opening !== expected) return false;
      opening = { ...opening, ...patch }; emit(); return true;
    },
    suppress: (documentId?: string) => {
      if (!opening || documentId && opening.documentId !== documentId) return;
      opening.abort?.abort(); opening = { ...opening, epoch: ++epoch, suppressed: true }; emit();
    },
    resolve: (documentId: string | undefined, eligible: boolean) => {
      overviewDocument = eligible ? documentId : undefined;
      if (documentId && documentId !== previousDocument && eligible) {
        opening?.abort?.abort();
        opening = { documentId, openId: String(++generation), epoch: ++epoch, consumed: false, suppressed: false }; emit();
      }
      previousDocument = documentId;
    },
  };
}
export type DocumentViewNavigation = ReturnType<typeof createNavigation>;
const owners = new WeakMap<QueryClient, DocumentViewNavigation>();
export function documentViewNavigation(client: QueryClient): DocumentViewNavigation {
  let owner = owners.get(client);
  if (!owner) { owner = createNavigation(); owners.set(client, owner); }
  return owner;
}
function destination(location: { pathname: string; search: Record<string, unknown> }) {
  const documentId = /^\/documents\/([^/]+)\/?$/.exec(location.pathname)?.[1];
  return { documentId, eligible: Boolean(documentId && (location.search.view ?? 'published') === 'published'
    && (location.search.tab ?? 'overview') === 'overview' && !location.search.workflow) };
}
const installed = new WeakMap<QueryClient, () => void>();
export function installDocumentViewNavigation(router: AnyRouter, client: QueryClient): () => void {
  const existing = installed.get(client); if (existing) return existing;
  const owner = documentViewNavigation(client);
  const resolved = router.subscribe('onResolved', event => {
    const next = destination(event.toLocation); owner.resolve(next.documentId, next.eligible);
  });
  const history = router.history.subscribe(({ location }: { location: { pathname: string; search: string } }) => {
    const next = destination({ pathname: location.pathname, search: router.options.parseSearch!(location.search) });
    owner.visit(next.documentId, next.eligible);
    const opening = owner.get();
    if (opening && (next.documentId !== opening.documentId || !next.eligible)) owner.suppress();
  });
  const normalReads = new Set(['document', 'document-version', 'document-version-files']);
  const queries = client.getQueryCache().subscribe(event => {
    const [name, documentId, versionId] = event.query.queryKey;
    const revoked = event.type === 'removed' || event.type === 'updated' && (event.action.type === 'invalidate'
      || event.action.type === 'error' || event.action.type === 'setState' && event.query.state.status !== 'success');
    if (name === 'document-current-read-state' && typeof documentId === 'string' && revoked) {
      owner.suppress(documentId); owner.revokeCurrent(documentId); return;
    }
    const opening = owner.get();
    if (!opening || opening.suppressed || typeof name !== 'string' || !normalReads.has(name) || documentId !== opening.documentId
      || name !== 'document' && opening.versionId && versionId !== opening.versionId) return;
    if (revoked || event.type === 'updated' && event.action.type === 'fetch' && opening.started && !opening.consumed && event.query.state.data !== undefined) owner.suppress(opening.documentId);
  });
  const stop = () => { resolved(); history(); queries(); owner.suppress(); installed.delete(client); };
  installed.set(client, stop); return stop;
}
export function invalidateDocumentReadStateViews(client: QueryClient, documentId?: string): void {
  const owner = documentViewNavigation(client); owner.suppress(documentId); owner.revokeCurrent(documentId);
  const filter = { predicate: (query: { queryKey: readonly unknown[] }) => ['document-read-opening', 'document-current-read-state'].includes(String(query.queryKey[0])) && (!documentId || query.queryKey[1] === documentId) };
  void client.invalidateQueries({ ...filter, refetchType: 'none' });
  void client.cancelQueries(filter, { revert: false });
}
