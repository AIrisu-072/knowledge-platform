import { test, expect, type Page } from '@playwright/test';
import {
  getDocument, getDocumentVersion, type CommandsEndPublication, type CommandsWithdrawVersion,
  type ModelsEndPublicationResult, type ModelsWithdrawResult,
} from '@knowledge-platform/document-api-client';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { options, runtime } from './support';
import { createLifecycleFixture, lifecycleSnapshot, saveLifecycleSnapshot } from './lifecycle-support';

// This additional acceptance slice never records images, trace or video, even on failure.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });
const uuidV7Pattern = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

async function checkReasonAndCancel(page: Page, action: string, title: string, confirm: string) {
  const open = page.getByRole('button', { name: action, exact: true });
  await expect(open).toBeEnabled();
  await open.press('Enter');
  const dialog = page.getByRole('dialog', { name: title, exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('button', { name: confirm, exact: true })).toBeDisabled();
  await dialog.getByLabel('理由', { exact: true }).fill('   ');
  await expect(dialog.getByRole('button', { name: confirm, exact: true })).toBeDisabled();
  await dialog.getByLabel('理由', { exact: true }).fill('取消確認専用の合成理由');
  await expect(dialog.getByRole('button', { name: confirm, exact: true })).toBeEnabled();
  await dialog.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
  await expect(dialog).toBeHidden();
  await expect(open).toBeFocused();
}

async function confirmOperation(page: Page, action: string, title: string, confirm: string, reason: string, pathname: string) {
  await page.getByRole('button', { name: action, exact: true }).press('Enter');
  const dialog = page.getByRole('dialog', { name: title, exact: true });
  await expect(dialog.getByLabel('理由', { exact: true })).toHaveValue('');
  await dialog.getByLabel('理由', { exact: true }).fill(reason);
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === pathname && response.request().method() === 'POST');
  await dialog.getByRole('button', { name: confirm, exact: true }).press('Enter');
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  expect(response.request().postDataJSON().reason).toBe(reason);
  expect(response.request().postDataJSON().operationId).toMatch(uuidV7Pattern);
  await expect(dialog).toBeHidden();
  return response;
}

test('GUI現行公開版の取下げは直前の公開版へ復帰し、最後の公開版取下げ後は公開なしとなる', async ({ page }) => {
  const context = await runtime(), common = options(context.human);
  const fixture = await createLifecycleFixture(context, '【合成データ】GUI版取下げ');
  const { documentId, firstVersionId, secondVersionId } = fixture, path = { documentId };
  const before = await lifecycleSnapshot(context.human, documentId);
  expect(before.versions.map(version => version.files[0]!.hash)).toEqual(fixture.hashes);
  expect(before.published.currentVersionId).toBe(secondVersionId);
  expect((await getDocumentVersion({ ...common, path: { ...path, versionId: secondVersionId }, query: { purpose: 'published' } })).data.capabilities.withdraw.status).toBe('available');
  completed('gui-lifecycle-fixture-ready');
  const withdrawals: CommandsWithdrawVersion[] = [], origins = new Set<string>();
  page.on('request', req => {
    const url = new URL(req.url());
    if (url.pathname.startsWith('/v1/')) origins.add(url.origin);
    if (url.pathname.startsWith(`/v1/documents/${documentId}/versions/`) && url.pathname.endsWith(':withdraw') && req.method() === 'POST') withdrawals.push(req.postDataJSON());
  });
  await page.goto(`/documents/${documentId}?view=published&tab=versions&versionId=${secondVersionId}`);
  await checkReasonAndCancel(page, '選択版を取下げ', '版の取下げを確認', '取下げを確定');
  expect(withdrawals).toHaveLength(0);
  expect(await lifecycleSnapshot(context.human, documentId)).toEqual(before);
  completed('gui-lifecycle-cancel-verified');
  const fallbackResponse = await confirmOperation(page, '選択版を取下げ', '版の取下げを確認', '取下げを確定', '【合成データ】第二版を取下げて初版へ戻す', `/v1/documents/${documentId}/versions/${secondVersionId}:withdraw`);
  const fallbackResult = await fallbackResponse.json() as ModelsWithdrawResult;
  expect(fallbackResult).toMatchObject({ operationId: withdrawals[0]!.operationId, documentId, targetVersionId: secondVersionId,
    formerCurrentVersionId: secondVersionId, resultingCurrentVersionId: firstVersionId, resultingRevision: before.published.revision! + 1 });
  await expect(page.getByRole('status')).toContainText('版を取下げました。');
  const fallback = await lifecycleSnapshot(context.human, documentId);
  expect(withdrawals).toHaveLength(1);
  expect(withdrawals[0]!.expectedRevision).toBe(before.published.revision);
  expect(fallback.published.currentVersionId).toBe(firstVersionId);
  expect(fallback.versions.map(version => version.lifecycleState)).toEqual(['withdrawn', 'published']);
  expect(fallback.revisions).toHaveLength(before.revisions.length + 1);
  expect(fallback.revisions[0]).toMatchObject({ documentVersionId: firstVersionId, major: 3, minor: 0, sourceKind: 'withdrawFallback' });
  expect(fallback.history.filter(item => item.actionCode === 'document.version.withdrawn')).toEqual([
    expect.objectContaining({ sourceKey: `version_operation:${fallbackResult.operationId}`, actor: 'poc-human', provenanceQuality: 'operationLedger' }),
  ]);
  expect(await lifecycleSnapshot(context.agent, documentId)).toEqual(fallback);
  completed('gui-withdraw-fallback-verified');

  await page.goto(`/documents/${documentId}?view=published&tab=versions&versionId=${firstVersionId}`);
  const nullResponse = await confirmOperation(page, '選択版を取下げ', '版の取下げを確認', '取下げを確定', '【合成データ】残る初版も取下げる', `/v1/documents/${documentId}/versions/${firstVersionId}:withdraw`);
  const nullResult = await nullResponse.json() as ModelsWithdrawResult;
  expect(nullResult).toMatchObject({ operationId: withdrawals[1]!.operationId, documentId, targetVersionId: firstVersionId,
    formerCurrentVersionId: firstVersionId, resultingCurrentVersionId: null, resultingRevision: fallback.published.revision! + 1 });
  await expect(page.getByRole('status')).toContainText('版を取下げました。');
  expect(withdrawals).toHaveLength(2);
  expect(withdrawals[1]!.expectedRevision).toBe(fallback.published.revision);
  expect(new Set(withdrawals.map(item => item.operationId)).size).toBe(2);
  const withdrawn = await lifecycleSnapshot(context.human, documentId);
  expect(withdrawn.published).toEqual({ status: 404, code: 'DOCUMENT_NOT_FOUND' });
  expect(withdrawn.versions.map(version => version.lifecycleState)).toEqual(['withdrawn', 'withdrawn']);
  expect(withdrawn.versions.every(version => !version.isCurrent)).toBe(true);
  expect(withdrawn.versions.map(version => version.files)).toEqual(before.versions.map(version => version.files));
  expect(withdrawn.revisions).toEqual(fallback.revisions);
  expect(withdrawn.history.filter(item => item.actionCode === 'document.version.withdrawn').map(item => item.sourceKey).sort()).toEqual([
    `version_operation:${fallbackResult.operationId}`, `version_operation:${nullResult.operationId}`,
  ].sort());
  expect(withdrawn.history.filter(item => item.actionCode === 'document.version.published')).toEqual(before.history);
  expect(withdrawn.history.filter(item => item.actionCode === 'document.publication.ended')).toHaveLength(0);
  expect(await lifecycleSnapshot(context.agent, documentId)).toEqual(withdrawn);
  expect(origins).toEqual(new Set([context.human]));
  completed('gui-withdraw-null-verified');
  await saveLifecycleSnapshot(context, 'withdrawn', withdrawn);
  completed('gui-lifecycle-snapshot-saved');
});

test('GUI公開終了は過去PUBLISHED版と原本を保持し、通常readを両profileから隠す', async ({ page }) => {
  const context = await runtime(), common = options(context.human);
  const fixture = await createLifecycleFixture(context, '【合成データ】GUI文書公開終了');
  const { documentId, secondVersionId } = fixture, path = { documentId };
  const before = await lifecycleSnapshot(context.human, documentId);
  expect(before.versions.map(version => version.files[0]!.hash)).toEqual(fixture.hashes);
  expect((await getDocument({ ...common, path, query: { view: 'published' } })).data.capabilities.endPublication.status).toBe('available');
  completed('gui-lifecycle-fixture-ready');
  const endings: CommandsEndPublication[] = [], origins = new Set<string>();
  page.on('request', req => {
    const url = new URL(req.url());
    if (url.pathname.startsWith('/v1/')) origins.add(url.origin);
    if (url.pathname === `/v1/documents/${documentId}:end-publication` && req.method() === 'POST') endings.push(req.postDataJSON());
  });
  await page.goto(`/documents/${documentId}?view=published&tab=versions&versionId=${secondVersionId}`);
  await checkReasonAndCancel(page, '公開を終了', '文書の公開終了を確認', '公開終了を確定');
  expect(endings).toHaveLength(0);
  expect(await lifecycleSnapshot(context.human, documentId)).toEqual(before);
  completed('gui-lifecycle-cancel-verified');
  const response = await confirmOperation(page, '公開を終了', '文書の公開終了を確認', '公開終了を確定', '【合成データ】文書全体の公開を終了する', `/v1/documents/${documentId}:end-publication`);
  const result = await response.json() as ModelsEndPublicationResult;
  expect(result).toMatchObject({ operationId: endings[0]!.operationId, documentId, formerCurrentVersionId: secondVersionId,
    resultingCurrentVersionId: null, resultingDocumentRevision: before.published.revision! + 1 });
  expect(endings).toHaveLength(1);
  expect(endings[0]).toMatchObject({ expectedRevision: before.published.revision, expectedCurrentVersionId: secondVersionId });
  await expect(page.getByRole('status')).toContainText('文書の公開を終了しました。');
  // The refresh is expected to fail closed after T10; the successful operation notice must survive it.
  const authoring = await getDocument({ ...common, throwOnError: false, path, query: { view: 'authoring' } });
  expect(authoring.response?.status).toBe(404); expect(authoring.error?.code).toBe('DOCUMENT_NOT_FOUND');
  await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
  await expect(page.getByRole('status')).toContainText('文書の公開を終了しました。');
  const ended = await lifecycleSnapshot(context.human, documentId);
  expect(ended.published).toEqual({ status: 404, code: 'DOCUMENT_NOT_FOUND' });
  expect(ended.versions.map(version => version.lifecycleState)).toEqual(['published', 'published']);
  expect(ended.versions.every(version => !version.isCurrent)).toBe(true);
  expect(ended.versions.map(({ isCurrent: _isCurrent, ...version }) => version)).toEqual(before.versions.map(({ isCurrent: _isCurrent, ...version }) => version));
  expect(ended.revisions).toEqual(before.revisions);
  expect(ended.history.filter(item => item.actionCode === 'document.version.published')).toEqual(before.history);
  expect(ended.history.filter(item => item.actionCode === 'document.version.withdrawn')).toHaveLength(0);
  expect(ended.history.filter(item => item.actionCode === 'document.publication.ended')).toEqual([
    expect.objectContaining({ sourceKey: `publication_end:${result.operationId}`, actor: 'poc-human', provenanceQuality: 'operationLedger' }),
  ]);
  expect(await lifecycleSnapshot(context.agent, documentId)).toEqual(ended);
  expect(endings).toHaveLength(1);
  expect(origins).toEqual(new Set([context.human]));
  completed('gui-publication-end-verified');
  await saveLifecycleSnapshot(context, 'ended', ended);
  completed('gui-lifecycle-snapshot-saved');
});
