import { test, expect, type Page, type Request } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import {
  getDocument, getDocumentVersion, listDocuments, listDocumentVersions, type CommandsEndPublication, type CommandsWithdrawVersion,
  type ModelsEndPublicationResult, type ModelsWithdrawResult, type ModelsDocumentList, type ModelsHistory, type VersionList, type VersionDetail, type FileList,
} from '@knowledge-platform/document-api-client';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { hash, options, runtime } from './support';
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

async function verifyHistoryWorkspace(page: Page, context: Awaited<ReturnType<typeof runtime>>, snapshot: Awaited<ReturnType<typeof lifecycleSnapshot>>, ended: boolean) {
  const { documentId } = snapshot;
  const readState = async (baseUrl: string) => {
    const common = options(baseUrl), path = { documentId };
    const list = (await listDocuments({ ...common, query: { view: 'history', pageSize: 100 } })).data;
    expect(list.view).toBe('history');
    if (list.view !== 'history') throw Error('Expected the history projection');
    const document = list.items.find(item => item.documentId === documentId);
    expect(document).toBeDefined();
    const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
    return { document: document!, versions: versions.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt })) };
  };
  const before = await readState(context.human), agentBefore = await readState(context.agent);
  // Version summaries omit title; the current HistoryDocument projection owns the representative title.
  const title = before.document.title;
  expect(before.document.ended).toBe(ended);
  expect(before.document.lifecycleState).toBe(ended ? 'published' : 'withdrawn');
  // Normal-detail 404 may leave both refusal markers. Recovery uses fresh GUI history reads.
  await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
  const forbidden: string[] = [];
  const recordRequest = (request: Request) => {
    const url = new URL(request.url());
    if (!url.pathname.startsWith('/v1/')) return;
    if (url.origin !== context.human || request.method() !== 'GET' || url.pathname === `/v1/documents/${documentId}`
      || url.pathname.startsWith(`/v1/documents/${documentId}/versions`) && url.searchParams.get('purpose') !== 'history') {
      forbidden.push(`${request.method()} ${url.pathname}`);
    }
  };
  page.on('request', recordRequest);
  const listResponse = (filtered: boolean) => page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === context.human && url.pathname === '/v1/documents' && url.searchParams.get('view') === 'history'
      && (!filtered || url.searchParams.get('titleContains') === title) && response.request().method() === 'GET';
  });
  const navigationRead = listResponse(false);
  await page.getByRole('navigation', { name: 'メインナビゲーション', exact: true }).getByRole('link', { name: '文書履歴', exact: true }).click();
  expect((await navigationRead).status()).toBe(200);
  await expect(page.getByRole('heading', { name: '文書履歴', exact: true, level: 1 })).toBeVisible();
  await page.getByRole('searchbox', { name: '文書名で絞り込み', exact: true }).fill(title);
  const filteredRead = listResponse(true);
  await page.getByRole('button', { name: '絞り込む', exact: true }).click();
  const filteredResult = await filteredRead;
  expect(filteredResult.status()).toBe(200);
  const list = await filteredResult.json() as ModelsDocumentList;
  expect(list.items.find(item => item.documentId === documentId)).toEqual(before.document);
  // The title button also contains its unread badge; the existing ID identifies the exact row.
  await page.getByRole('table', { name: '文書一覧', exact: true }).locator(`button[data-document-id="${documentId}"]`).click();
  const workspace = page.getByRole('region', { name: '選択した文書の履歴', exact: true });
  await expect(workspace).toBeVisible();
  await expect(workspace.locator('dt').filter({ hasText: /^代表版の文書名$/ }).locator('+ dd')).toHaveText(title);
  await expect(workspace.locator('dt').filter({ hasText: /^文書の公開終了$/ }).locator('+ dd')).toHaveText(ended ? '公開終了済み' : '公開終了していません');
  await expect(workspace.locator('dt').filter({ hasText: /^代表版の状態$/ }).locator('+ dd')).toHaveText(ended ? 'PUBLISHED' : 'WITHDRAWN');
  const historyResponse = (suffix: string) => page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/versions${suffix}`
      && url.searchParams.get('purpose') === 'history' && response.request().method() === 'GET';
  });
  await workspace.getByRole('button', { name: 'コンテンツ版の履歴を開く', exact: true }).click();
  const content = workspace.getByRole('region', { name: 'コンテンツ版の履歴（閲覧専用）', exact: true });
  const restart = content.getByRole('button', { name: 'コンテンツ版の履歴を最初から読み直す', exact: true });
  await expect(restart).toBeEnabled();
  const versionRead = historyResponse('');
  await restart.click();
  const versionResult = await versionRead;
  expect(versionResult.status()).toBe(200);
  expect(Object.fromEntries(new URL(versionResult.url()).searchParams)).toEqual({ purpose: 'history', pageSize: '100' });
  const versions = await versionResult.json() as VersionList;
  expect(versions.items.map(version => version.versionId)).toEqual(snapshot.versions.map(version => version.versionId));
  expect(versions.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt }))).toEqual(before.versions);
  expect(versions.nextCursor).toBeNull();
  const oldVersion = snapshot.versions.find(version => version.versionNo === 1)!;
  const selection = content.getByRole('combobox', { name: '履歴のコンテンツ版を選択', exact: true });
  await expect(selection).toHaveValue('');
  const detailRead = historyResponse(`/${oldVersion.versionId}`), filesRead = historyResponse(`/${oldVersion.versionId}/files`);
  await selection.selectOption(oldVersion.versionId);
  const detailResult = await detailRead, filesResult = await filesRead;
  expect(detailResult.status()).toBe(200); expect(filesResult.status()).toBe(200);
  const detail = await detailResult.json() as VersionDetail, files = await filesResult.json() as FileList;
  expect(detail).toMatchObject({ versionId: oldVersion.versionId, versionNo: 1, lifecycleState: ended ? 'published' : 'withdrawn', isCurrent: false,
    firstReadAt: before.versions.find(version => version.versionId === oldVersion.versionId)!.firstReadAt });
  expect(detail.capabilities.download.status).toBe('available');
  const selected = content.getByRole('region', { name: '選択したコンテンツ版の詳細', exact: true });
  await expect(selected.getByRole('heading', { name: 'Version 1', exact: true })).toBeVisible();
  await expect(selected).toContainText(detail.title);
  const originals = files.items.filter(file => file.role === 'AUTHORITATIVE');
  expect(originals.length).toBeGreaterThan(0);
  await expect(selected.getByRole('button')).toHaveCount(originals.length);
  for (const file of originals) {
    const downloadRead = historyResponse(`/${oldVersion.versionId}/files/${file.contentItemId}/${file.representationId}`);
    const download = page.waitForEvent('download');
    await selected.getByRole('button', { name: `履歴の原本を取得: ${file.displayName}`, exact: true }).click();
    expect((await downloadRead).status()).toBe(200);
    const saved = await download, bytes = await readFile((await saved.path())!);
    expect(saved.suggestedFilename()).toBe(file.displayName);
    expect(bytes.byteLength).toBe(file.sizeBytes);
    expect(hash(bytes)).toBe(oldVersion.files.find(item => item.contentItemId === file.contentItemId && item.representationId === file.representationId)!.hash);
  }
  const events = workspace.getByRole('region', { name: '変更履歴', exact: true });
  const eventRestart = events.getByRole('button', { name: '変更履歴を最初から読み直す', exact: true });
  await expect(eventRestart).toBeEnabled();
  const eventRead = page.waitForResponse(response => new URL(response.url()).origin === context.human
    && new URL(response.url()).pathname === `/v1/documents/${documentId}/history` && response.request().method() === 'GET');
  await eventRestart.click();
  const eventResult = await eventRead;
  expect(eventResult.status()).toBe(200);
  expect([...new URL(eventResult.url()).searchParams]).toEqual([['pageSize', '100']]);
  const history = await eventResult.json() as ModelsHistory;
  expect(history.nextCursor).toBeNull();
  expect(history.items.filter(item => ['document.version.published', 'document.version.withdrawn', 'document.publication.ended'].includes(item.actionCode))
    .map(item => ({ sourceKey: item.sourceKey, actionCode: item.actionCode, actor: item.actor?.principalId,
      occurredAt: item.occurredAt, details: item.details, provenanceQuality: item.provenanceQuality }))).toEqual(snapshot.history);
  await expect(events.getByRole('listitem')).toHaveCount(history.items.length);
  await expect(events.getByRole('listitem').locator('strong')).toHaveText(history.items.map(item => item.actionCode));
  await expect(events.getByRole('button', { name: '変更履歴をさらに表示', exact: true })).toBeHidden();
  await expect(selection).toHaveValue(oldVersion.versionId);
  expect(new URL(page.url()).pathname).toBe('/documents');
  expect(new URL(page.url()).searchParams.get('view')).toBe('history');
  expect(new URL(page.url()).searchParams.get('selectedDocumentId')).toBe(documentId);
  expect(await readState(context.human)).toEqual(before);
  expect(await readState(context.agent)).toEqual(agentBefore);
  // This includes the original published-detail 404, Version/Revision IDs and authoritative hashes.
  expect(await lifecycleSnapshot(context.human, documentId)).toEqual(snapshot);
  expect(await lifecycleSnapshot(context.agent, documentId)).toEqual(snapshot);
  expect(forbidden).toEqual([]);
  page.off('request', recordRequest);
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
  const lifecycle = page.getByRole('region', { name: '公開状態の操作', exact: true });
  await expect(lifecycle.getByRole('status')).toHaveText('版を取下げました。直前の公開版へ復帰しました。');
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
  await expect(lifecycle.getByRole('status')).toHaveText('版を取下げました。現行の公開版はありません。');
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
  await verifyHistoryWorkspace(page, context, withdrawn, false);
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
  const lifecycle = page.getByRole('region', { name: '公開状態の操作', exact: true });
  await expect(lifecycle.getByRole('status')).toHaveText('文書の公開を終了しました。原本と過去版は保持されています。');
  // The refresh is expected to fail closed after T10; the successful operation notice must survive it.
  const authoring = await getDocument({ ...common, throwOnError: false, path, query: { view: 'authoring' } });
  expect(authoring.response?.status).toBe(404); expect(authoring.error?.code).toBe('DOCUMENT_NOT_FOUND');
  await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
  await expect(lifecycle.getByRole('status')).toHaveText('文書の公開を終了しました。原本と過去版は保持されています。');
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
  await verifyHistoryWorkspace(page, context, ended, true);
  await saveLifecycleSnapshot(context, 'ended', ended);
  completed('gui-lifecycle-snapshot-saved');
});
