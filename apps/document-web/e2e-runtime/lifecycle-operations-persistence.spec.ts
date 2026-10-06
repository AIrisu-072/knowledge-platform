import { test, expect, type Page, type Request } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { getDocument, getSession, listDocuments, listDocumentVersions, type ModelsDocumentList, type ModelsHistory, type VersionList, type VersionDetail, type FileList } from '@knowledge-platform/document-api-client';
import { hash, options, runtime } from './support';
import { lifecycleSnapshot, lifecycleStatePath, type LifecycleState } from './lifecycle-support';

// Restart proof for this slice has the same no-recording boundary as its journey.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
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

test('両HTTP再起動後もGUI取下げ・公開終了の公開不可と履歴・原本・操作IDが保持される', async ({ page }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(lifecycleStatePath(context), 'utf8')) as LifecycleState;
  expect(state.documents.map(item => item.key).sort()).toEqual(['ended', 'withdrawn']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  for (const { key, snapshot } of state.documents) {
    expect(snapshot.published).toEqual({ status: 404, code: 'DOCUMENT_NOT_FOUND' });
    expect(await lifecycleSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    expect(await lifecycleSnapshot(context.agent, snapshot.documentId)).toEqual(snapshot);
    if (key === 'ended') {
      const detail = await getDocument({ ...options(context.human), throwOnError: false,
        path: { documentId: snapshot.documentId }, query: { view: 'authoring' } });
      expect(detail.response?.status).toBe(404); expect(detail.error?.code).toBe('DOCUMENT_NOT_FOUND');
    }
    await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
    await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
    await expect(page.getByRole('complementary', { name: '原本と版' })).toHaveCount(0);
    await verifyHistoryWorkspace(page, context, snapshot, key === 'ended');
  }
  test.info().annotations.push({ type: 'runtime-completed', description: 'gui-lifecycle-restart-verified' });
});
