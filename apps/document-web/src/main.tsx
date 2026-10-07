import { createRootRoute, createRoute, createRouter, Outlet, redirect, RouterProvider } from '@tanstack/react-router';
import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { OrganizationProvider } from './application/organization-context';
import { validateTaskSearch } from './application/work-workspace';
import { DocumentHomePage } from './routes/DocumentHomePage';
import { DocumentListRouteError } from './routes/DocumentListRouteError';
import { validateDetailSearch, validateListSearch } from './application/search-state';
import './design-system/global.css';

const LazyTaskHomePage = lazy(() => import('./routes/TaskHomePage').then(({ TaskHomePage }) => ({ default: TaskHomePage })));
const LazyOrganizationSearchPage = lazy(() => import('./routes/TaskHomePage').then(({ OrganizationSearchPage }) => ({ default: OrganizationSearchPage })));
const LazyOrganizationResponsibilitiesPage = lazy(() => import('./routes/OrganizationResponsibilitiesPage').then(({ OrganizationResponsibilitiesPage }) => ({ default: OrganizationResponsibilitiesPage })));
const LazyDocumentDetailPage = lazy(() => import('./routes/DocumentDetailPage').then(({ DocumentDetailPage }) => ({ default: DocumentDetailPage })));

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error('Missing application root element');
}

const rootRoute = createRootRoute({ component: Outlet });
const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  beforeLoad: () => {
    throw redirect({ to: '/documents', search: validateListSearch({}) });
  },
});
const documentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/documents',
  validateSearch: validateListSearch,
  component: DocumentHomePage,
  errorComponent: DocumentListRouteError,
});
const documentDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/documents/$documentId',
  validateSearch: validateDetailSearch,
  component: () => (
    <Suspense fallback={<p role="status">文書の詳細を読み込み中…</p>}>
      <LazyDocumentDetailPage />
    </Suspense>
  ),
});
const tasksRoute = createRoute({ getParentRoute: () => rootRoute, path: '/tasks', validateSearch: validateTaskSearch, component: () => <Suspense fallback={<p role="status">タスクを読み込み中…</p>}><LazyTaskHomePage /></Suspense> });
const searchRoute = createRoute({ getParentRoute: () => rootRoute, path: '/search', component: () => <Suspense fallback={<p role="status">検索画面を読み込み中…</p>}><LazyOrganizationSearchPage /></Suspense> });
// Reached from the task header only; never a primary navigation entry.
const responsibilitiesRoute = createRoute({ getParentRoute: () => rootRoute, path: '/organization/responsibilities', component: () => <Suspense fallback={<p role="status">担当と委任を読み込み中…</p>}><LazyOrganizationResponsibilitiesPage /></Suspense> });
const routeTree = rootRoute.addChildren([indexRoute, documentsRoute, documentDetailRoute, tasksRoute, searchRoute, responsibilitiesRoute]);
const router = createRouter({ routeTree });
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

createRoot(rootElement).render(
  <StrictMode>
    <OrganizationProvider><QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider></OrganizationProvider>
  </StrictMode>,
);
