import { createRootRoute, createRoute, createRouter, Outlet, redirect, RouterProvider } from '@tanstack/react-router';
import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { DocumentHomePage } from './routes/DocumentHomePage';
import { validateDetailSearch, validateListSearch } from './application/search-state';
import './design-system/global.css';

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
const routeTree = rootRoute.addChildren([indexRoute, documentsRoute, documentDetailRoute]);
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
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
