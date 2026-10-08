import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
const docId = '00000000-0000-4000-8000-000000000010'; const verId = '00000000-0000-4000-8000-000000000011';
const fileId = '00000000-0000-4000-8000-000000000012'; const repId = '00000000-0000-4000-8000-000000000013';
const pdf = readFileSync(new URL('../e2e-runtime/fixtures/pdf/base.pdf', import.meta.url));
const denied = { status: 'disabled', reason: 'permission' }; const available = { status: 'available' };
test('self-hosted PDF worker renders a fixed original under unchanged CSP, closes and reloads without cached original bytes', async ({ page }, testInfo) => {
  const origin = new URL(testInfo.project.use.baseURL!).origin;
  let downloads = 0; let readMutations = 0; const foreign: string[] = []; const violations: string[] = [];
  page.on('request', request => { if (!request.url().startsWith(origin)) foreign.push(request.url()); });
  await page.addInitScript(() => { (window as unknown as { violations: string[] }).violations = []; document.addEventListener('securitypolicyviolation', event => { (window as unknown as { violations: string[] }).violations.push(event.violatedDirective); }); });
  await page.route('**/v1/**', async route => {
    const url = new URL(route.request().url()); const path = url.pathname;
    if (route.request().method() !== 'GET') { readMutations++; await route.fulfill({ status: 403, json: {} }); return; }
    const summary = { versionId: verId, versionNo: 1, lifecycleState: 'PUBLISHED', updatedAt: '2026-10-08T00:00:00Z', fileSummary: { authoritativeItemCount: 1, totalSizeBytes: pdf.length, primary: { displayName: '原本.pdf', mediaType: 'application/pdf', sizeBytes: pdf.length } } };
    if (path.endsWith(`/files/${fileId}/${repId}`)) { downloads++; await route.fulfill({ contentType: 'application/pdf', body: pdf }); return; }
    if (path.endsWith('/files')) { await route.fulfill({ json: { items: [{ contentItemId: fileId, representationId: repId, ordinal: 0, role: 'authoritative', logicalPath: '原本.pdf', displayName: '原本.pdf', mediaType: 'application/pdf', sizeBytes: pdf.length }] } }); return; }
    if (path === `/v1/documents/${docId}`) { await route.fulfill({ json: { documentId: docId, title: 'ビューア合成文書', revision: 1, currentVersionId: verId, displayVersion: summary, metadata: {}, readState: { isRead: true, firstReadAt: null }, displayRevision: null, displayTimestamp: { kind: 'revisionCreatedAt', value: '2026-10-08T00:00:00Z' }, capabilities: { manageAccess: denied, updateMetadata: denied, createVersion: denied, moveDocument: denied, endPublication: denied, compareVersions: denied } } }); return; }
    if (path === `/v1/documents/${docId}/versions/${verId}`) { await route.fulfill({ json: { ...summary, currentPublicationScheduleId: null, capabilities: { download: available, withdraw: denied } } }); return; }
    if (path.includes('read-state') || path.endsWith('/session')) { await route.fulfill({ status: 403, json: { type: 'about:blank', title: 'denied', status: 403, code: 'FORBIDDEN', traceId: 'synthetic', retryable: false } }); return; }
    if (path.endsWith('/folders/root')) { await route.fulfill({ json: { folderId: docId, name: 'ルート', capabilities: {} } }); return; }
    await route.fulfill({ json: { items: [], nextCursor: null, capabilities: {} } });
  });
  await page.goto(`/documents/${docId}?view=published&tab=overview`);
  await page.getByRole('button', { name: '原本.pdfを表示' }).focus();
  await page.keyboard.press('Enter');
  // Canvas has no implicit role, use the exact accessible label.
  const pixels = page.locator('canvas[aria-label="PDF 1ページ目"]');
  await expect(pixels).toBeVisible(); await expect(page.getByText('原本を表示中…')).toBeHidden();
  await expect.poll(() => pixels.evaluate(element => { const target = element as HTMLCanvasElement; if (!target.width) return false; const data = target.getContext('2d')!.getImageData(0, 0, target.width, target.height).data; for (let i = 0; i < data.length; i += 4) if (data[i]! < 220 && data[i + 3]! > 0) return true; return false; })).toBe(true);
  expect(downloads).toBe(1); expect(readMutations).toBe(0); expect(foreign).toEqual([]);
  violations.push(...await page.evaluate(() => (window as unknown as { violations: string[] }).violations)); expect(violations).toEqual([]);
  await page.getByRole('button', { name: '原本表示を閉じる' }).focus(); await page.keyboard.press('Enter'); await expect(pixels).toHaveCount(0);
  await expect(page.getByRole('button', { name: '原本.pdfを表示' })).toBeFocused();
  await page.reload(); await expect(page.getByRole('button', { name: '原本.pdfを表示' })).toBeVisible(); expect(downloads).toBe(1);
});
