import { render, screen, within } from '@testing-library/react';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { AppShell } from '../src/components/app-shell/AppShell';

test('app shell exposes skip link, navigation, main workspace, and context panel', async () => {
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/documents', component: () => <AppShell>
      <h1>文書管理</h1>
    </AppShell> });
  const router = createRouter({ routeTree: root.addChildren([route]), history: createMemoryHistory({ initialEntries: ['/documents'] }) });
  render(<RouterProvider router={router as never} />);
  await screen.findByRole('heading', { name: '文書管理', level: 1 });

  expect(screen.getByRole('link', { name: 'メインコンテンツへ' })).toHaveAttribute('href', '#main-content');
  expect(screen.getByRole('banner')).toBeInTheDocument();
  expect(screen.getByRole('navigation', { name: 'メインナビゲーション' })).toBeInTheDocument();
  const navigation = within(screen.getByRole('navigation', { name: 'メインナビゲーション' }));
  expect(navigation.getByRole('link', { name: '文書' })).toHaveAttribute('href', expect.stringContaining('view=published'));
  expect(navigation.getByRole('link', { name: '文書履歴' })).toHaveAttribute('href', expect.stringContaining('view=history'));
  expect(screen.getByRole('main', { name: '文書ワークスペース' })).toContainElement(
    screen.getByRole('heading', { name: '文書管理', level: 1 }),
  );
  expect(within(screen.getByRole('complementary', { name: '文脈情報' })).getByText('項目を選択すると詳細が表示されます')).toBeVisible();
});
