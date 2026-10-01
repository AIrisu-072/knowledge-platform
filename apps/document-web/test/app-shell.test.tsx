import { render, screen, within } from '@testing-library/react';
import { expect, test } from '@jest/globals';
import { AppShell } from '../src/components/app-shell/AppShell';

test('app shell exposes skip link, navigation, main workspace, and context panel', () => {
  render(
    <AppShell>
      <h1>文書管理</h1>
    </AppShell>,
  );

  expect(screen.getByRole('link', { name: 'メインコンテンツへ' })).toHaveAttribute('href', '#main-content');
  expect(screen.getByRole('banner')).toBeInTheDocument();
  expect(screen.getByRole('navigation', { name: 'メインナビゲーション' })).toBeInTheDocument();
  expect(screen.getByRole('main', { name: '文書ワークスペース' })).toContainElement(
    screen.getByRole('heading', { name: '文書管理', level: 1 }),
  );
  expect(within(screen.getByRole('complementary', { name: '文脈情報' })).getByText('項目を選択すると詳細が表示されます')).toBeVisible();
});
