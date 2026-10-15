import { createElement } from 'react';
import { act, render } from '@testing-library/react';
import { QueryClient } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { documentViewNavigation, installDocumentViewNavigation, isOverviewVisible } from '../src/application/document-view-navigation';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';

function navigation(entry: string) {
  const root = createRootRoute({ component: Outlet });
  const home = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch });
  const detail = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch });
  const other = createRoute({ getParentRoute: () => root, path: '/tasks' });
  const router = createRouter({ routeTree: root.addChildren([home, detail, other]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: 15_000, retry: 1, refetchOnWindowFocus: false } } });
  const stop = installDocumentViewNavigation(router, client);
  render(createElement(RouterProvider, { router: router as never }));
  return { router, client, stop, owner: documentViewNavigation(client) };
}
const containers: HTMLElement[] = [];
function overview() { const ancestor = document.createElement('section'); const element = document.createElement('div'); ancestor.append(element); document.body.append(ancestor); containers.push(ancestor); return { ancestor, element }; }
afterEach(() => { containers.splice(0).forEach(node => node.remove()); Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }); });

test('connected_visible_overview_is_eligible_but_detached_is_not', () => {
  const { element } = overview(); expect(isOverviewVisible(element)).toBe(true); element.remove(); expect(isOverviewVisible(element)).toBe(false);
});
test.each(['none', 'visibility', 'opacity', 'hidden', 'aria'] as const)('nonvisible_ancestor_%s_stops_display', mode => {
  const { element, ancestor } = overview();
  if (mode === 'none') ancestor.style.display = 'none'; if (mode === 'visibility') ancestor.style.visibility = 'hidden'; if (mode === 'opacity') ancestor.style.opacity = '0'; if (mode === 'hidden') ancestor.hidden = true; if (mode === 'aria') ancestor.setAttribute('aria-hidden', 'true');
  expect(isOverviewVisible(element)).toBe(false);
});
test('hidden_document_stops_display_without_another_opening', () => {
  const { element } = overview(); Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }); expect(isOverviewVisible(element)).toBe(false);
});
test('initial_resolved_published_overview_creates_one_opening', async () => {
  const h = navigation('/documents/doc?view=published&tab=overview'); await act(async () => { await h.router.load(); });
  expect(h.owner.get()?.documentId).toBe('doc'); const first = h.owner.get()!;
  await act(async () => { await h.router.load(); }); expect(h.owner.get()!.openId).toBe(first.openId); h.stop(); h.client.clear();
});
test.each(['versions', 'history', 'compare'] as const)('direct_%s_and_same_doc_overview_do_not_issue_a_display_opening', async tab => {
  const h = navigation(`/documents/doc?view=published&tab=${tab}`); await act(async () => { await h.router.load(); });
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: 'doc' }, search: validateDetailSearch({ view: 'published', tab: 'overview' }) }); });
  expect(h.owner.get()).toBeUndefined(); h.stop(); h.client.clear();
});
test('leaving_and_reentering_same_history_entry_gets_a_new_opening', async () => {
  const h = navigation('/documents/doc?view=published'); await act(async () => { await h.router.load(); }); const first = h.owner.get()!;
  await act(async () => { await h.router.navigate({ to: '/documents', search: validateListSearch({}) }); });
  expect(h.owner.get()?.suppressed).toBe(true);
  await act(async () => { await h.router.navigate({ to: '/documents/$documentId', params: { documentId: 'doc' }, search: validateDetailSearch({ view: 'published' }) }); });
  expect(h.owner.get()!.openId).not.toBe(first.openId); h.stop(); h.client.clear();
});
test('unrelated_route_with_document_like_search_does_not_create_an_opening', async () => {
  const h = navigation('/tasks?documentId=doc&view=published&tab=overview'); await act(async () => { await h.router.load(); }); expect(h.owner.get()).toBeUndefined(); h.stop(); h.client.clear();
});
