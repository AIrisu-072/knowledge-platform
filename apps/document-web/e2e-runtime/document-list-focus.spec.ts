import { test, expect } from '@playwright/test';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { runtime } from './support';

// 合成fixtureの実GETを保留するだけ。応答本文や認可結果は置き換えない。
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });

test('遅れた実一覧応答は登録ボタンのフォーカスを保持しEnterとキャンセル復帰を妨げない', async ({ page }) => {
  const context = await runtime();
  const sharedFolderId = context.manifest.folders.shared.folderId;
  let releaseList!: () => void;
  const listGate = new Promise<void>(resolve => { releaseList = resolve; });
  let createRequests = 0;
  page.on('request', req => { if (new URL(req.url()).pathname === '/v1/documents' && req.method() === 'POST') createRequests++; });
  await page.route('**/v1/documents?**', async route => {
    if (new URL(route.request().url()).searchParams.get('folderId') !== sharedFolderId) return route.continue();
    await listGate;
    await route.continue();
  });
  try {
    await page.goto('/documents?view=published');
    const trigger = page.getByRole('button', { name: '文書を登録', exact: true });
    await expect(trigger).toBeEnabled();
    await page.getByRole('button', { name: 'PoC Shared', exact: true }).click();
    await expect(page).toHaveURL(url => url.searchParams.get('folderId') === sharedFolderId);
    await expect(trigger).toBeEnabled();
    await trigger.focus();
    await expect(trigger).toBeFocused();
    releaseList();
    await expect(page.locator('button[data-document-id]').filter({ hasText: '通達サンプル' })).toBeVisible();
    // Flush the response's layout/effect frame; no fixed timeout or relaxed oracle.
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(trigger).toBeFocused();
    await trigger.press('Enter');
    const dialog = page.getByRole('dialog', { name: '文書を登録', exact: true });
    await expect(dialog.getByText('PoC Shared', { exact: true })).toBeVisible();
    await dialog.getByLabel('文書名', { exact: true }).fill('取消のみの合成データ');
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
    expect(createRequests).toBe(0);
  } finally { releaseList(); }
});
