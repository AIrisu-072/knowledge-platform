import { test, expect, type Locator, type Page, type Response } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, getDocumentVersion,
  listDocumentRevisions, listDocumentVersions, listVersionFiles, publishVersion,
  type CommandsMetadataPatch, type MutationResult, type PublishedDocument,
} from '@knowledge-platform/document-api-client';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { hash, options, persistedSnapshot, runtime, saveSnapshot, uuidV7, type PersistedState, type RuntimeContext } from './support';

// 両phaseで合成metadataを表示するため、画像・trace・videoは記録しない。
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });

// 既存のowned snapshot helperを非公開sidecarへ再利用する。
// 元のpersistence testが添付するstateには、このmetadataを含めない。
const metadataContext = (context: RuntimeContext): RuntimeContext => ({ ...context, statePath: `${context.statePath}.metadata-editor` });
const privatelyEqual = (actual: unknown, expected: unknown) => expect(isDeepStrictEqual(actual, expected)).toBe(true);
const inputEquals = async (field: Locator, value: string) => {
  await expect.poll(async () => (await field.inputValue()) === value).toBe(true);
};
const fillPrivate = async (field: Locator, value: string) => {
  try { await field.fill(value); }
  catch { throw new Error('合成metadataの入力操作に失敗しました'); }
};
const editor = (page: Page) => page.getByRole('dialog', { name: 'メタデータを編集', exact: true });
type MetadataListFilters = { documentType: string; owningDepartment: string; category: string };
type CreatedListRange = { createdFrom: string | null; createdBefore: string | null };
const listTitle = 'Synthetic metadata GUI acceptance';
const createdListRange = (params: URLSearchParams): CreatedListRange => ({ createdFrom: params.get('createdFrom'), createdBefore: params.get('createdBefore') });
const waitCreatedList = (page: Page, range: CreatedListRange) => page.waitForResponse(result => {
  const url = new URL(result.url());
  return url.pathname === '/v1/documents' && result.request().method() === 'GET'
    && url.searchParams.get('view') === 'published' && url.searchParams.get('titleContains') === listTitle
    && isDeepStrictEqual(createdListRange(url.searchParams), range);
});
async function verifyCreatedInput(page: Page, label: string, raw: string, local: string | null) {
  const field = page.getByLabel(label, { exact: true });
  await inputEquals(field, local ?? raw);
  await expect(field).toHaveAttribute('type', local === null ? 'text' : 'datetime-local');
  if (local === null) await expect(field).toHaveAttribute('readonly', '');
  else {
    await expect(field).toHaveAttribute('step', '60');
    await expect(field).toBeEditable();
  }
}

async function verifyCreatedList(page: Page, response: Response, range: CreatedListRange, documents: { documentId: string; createdAt: string }[]) {
  expect(response.status()).toBe(200);
  const params = new URL(response.url()).searchParams;
  privatelyEqual(createdListRange(params), range);
  expect(params.get('view')).toBe('published');
  expect(params.get('titleContains') === listTitle).toBe(true);
  expect(params.has('cursor')).toBe(false);
  const body = await response.json() as { view: string; items: PublishedDocument[] };
  expect(body.view).toBe('published');
  privatelyEqual(body.items.map(item => ({ documentId: item.documentId, createdAt: item.createdAt })), documents);
  if (documents.length === 0) {
    await expect(page.getByRole('heading', { name: '文書がありません', exact: true })).toBeVisible();
    await expect(page.getByRole('table', { name: '文書一覧', exact: true })).toBeHidden();
  } else {
    await expect(page.getByRole('table', { name: '文書一覧', exact: true }).getByRole('row')).toHaveCount(documents.length + 1);
    for (const { documentId } of documents) await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeVisible();
  }
}

async function fillMetadataListFilters(page: Page, filters: MetadataListFilters) {
  await fillPrivate(page.getByLabel('文書種別', { exact: true }), filters.documentType);
  await fillPrivate(page.getByLabel('所管部署', { exact: true }), filters.owningDepartment);
  await fillPrivate(page.getByLabel('カテゴリ', { exact: true }), filters.category);
}

async function readFilteredList(page: Page, filters: MetadataListFilters, documentIds: string[], reset = false) {
  const response = page.waitForResponse(result => new URL(result.url()).pathname === '/v1/documents'
    && result.request().method() === 'GET');
  await page.getByRole('button', { name: reset ? '属性の絞り込みを解除' : '絞り込む', exact: true }).press('Enter');
  const result = await response;
  expect(result.status()).toBe(200);
  const params = new URL(result.url()).searchParams;
  privatelyEqual(Object.fromEntries(['documentType', 'owningDepartment', 'category'].map(key => [key, params.get(key)])), {
    documentType: filters.documentType || null, owningDepartment: filters.owningDepartment || null, category: filters.category || null,
  });
  expect(params.get('titleContains') === listTitle).toBe(true);
  expect(params.has('cursor')).toBe(false);
  const body = await result.json() as { items: { documentId: string }[] };
  privatelyEqual(body.items.map(item => item.documentId), documentIds);
  if (documentIds.length === 0) {
    await expect(page.getByRole('heading', { name: '文書がありません', exact: true })).toBeVisible();
    await expect(page.getByRole('table', { name: '文書一覧', exact: true })).toBeHidden();
  } else {
    await expect(page.getByRole('table', { name: '文書一覧', exact: true }).getByRole('row')).toHaveCount(documentIds.length + 1);
    for (const documentId of documentIds) await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeVisible();
  }
}

async function readUnreadPublishedList(page: Page, documentId: string, currentVersionId: string, range: CreatedListRange = { createdFrom: null, createdBefore: null }) {
  await page.getByRole('checkbox', { name: '未読のみ', exact: true }).check();
  const response = page.waitForResponse(result => new URL(result.url()).pathname === '/v1/documents'
    && result.request().method() === 'GET' && new URL(result.url()).searchParams.get('unreadOnly') === 'true');
  await page.getByRole('button', { name: '絞り込む', exact: true }).press('Enter');
  const result = await response;
  expect(result.status()).toBe(200);
  const params = new URL(result.url()).searchParams;
  expect(params.get('view')).toBe('published');
  expect(params.get('unreadOnly')).toBe('true');
  expect(params.get('titleContains') === listTitle).toBe(true);
  expect(params.has('cursor')).toBe(false);
  privatelyEqual(createdListRange(params), range);
  const body = await result.json() as { view: string; items: PublishedDocument[] };
  expect(body.view).toBe('published');
  privatelyEqual(body.items.map(item => ({ documentId: item.documentId, currentVersionId: item.currentVersionId, readState: item.readState })),
    [{ documentId, currentVersionId, readState: { isRead: false, firstReadAt: null } }]);
  await expect(page.getByRole('table', { name: '文書一覧', exact: true }).getByRole('row')).toHaveCount(2);
  await expect(page.locator(`[data-document-id="${documentId}"]`).getByText('未読', { exact: true })).toBeVisible();
}

async function saveMetadata(page: Page, documentId: string, reason: string) {
  const dialog = editor(page);
  await fillPrivate(dialog.getByLabel('変更理由', { exact: true }), reason);
  const response = page.waitForResponse(result => new URL(result.url()).pathname === `/v1/documents/${documentId}/metadata`
    && result.request().method() === 'PATCH');
  await dialog.getByRole('button', { name: '保存する', exact: true }).press('Enter');
  const result = await response;
  expect(result.status()).toBe(200);
  const body = result.request().postDataJSON() as CommandsMetadataPatch;
  expect(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(body.operationId)).toBe(true);
  const receipt = await result.json() as MutationResult;
  await expect(dialog.getByRole('status')).toHaveText(receipt.changed ? 'メタデータを更新しました。' : '変更はありませんでした。');
  await dialog.getByRole('button', { name: '閉じる', exact: true }).press('Enter');
  await expect(dialog).toBeHidden();
  return { body, result: receipt };
}

// 既存runnerの2サーバー再起動前後に、このspecをそれぞれ実行する。
// collection-onlyではruntimeを読み取らず、サーバーも起動しない。
if (process.env.KP_POC_RUNTIME_PHASE === 'journey') {
  test('metadataをGUIで未公開更新・正式Minor更新・no-opにし、旧属性と原本を保持する', async ({ page }) => {
    const context = await runtime(), common = options(context.human);
    const initialMetadata = {
      document_type: 'synthetic-original-type', owning_department: 'synthetic-original-department', category: 'synthetic-original-category',
      documentType: 'synthetic-legacy-type', department: 'synthetic-legacy-department',
      legacy_key: { keep: true }, extensions: { syntheticEditor: { keep: true } },
    };
    const versionMetadata = { document_type: 'synthetic-version-type', extensions: { versionOnly: true } };
    const original = Buffer.from('Synthetic metadata GUI acceptance original.\n');
    const created = await new BinaryTransportBridge({ baseUrl: context.human }).createDocument({
      request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic metadata GUI acceptance', documentMetadata: initialMetadata, versionMetadata },
      file: new Blob([original]), originalFilename: 'synthetic-metadata.txt', mediaType: 'text/plain',
    });
    const documentId = created.documentId, path = { documentId }, versionPath = { ...path, versionId: created.documentVersionId };
    const detail = (view: 'authoring' | 'published') => getDocument({ ...common, path, query: { view } }).then(response => response.data);
    const version = (purpose: 'authoring' | 'published') => getDocumentVersion({ ...common, path: versionPath, query: { purpose } }).then(response => response.data);
    const revisions = () => listDocumentRevisions({ ...common, path, query: { pageSize: 100 } }).then(response => response.data.items);
    const before = await detail('authoring'), beforeVersion = await version('authoring');
    const beforeFiles = (await listVersionFiles({ ...common, path: versionPath, query: { purpose: 'authoring' } })).data;
    expect(before.capabilities.updateMetadata.status).toBe('available');
    expect(before.currentVersionId).toBeNull();
    expect(await revisions()).toHaveLength(0);
    privatelyEqual(before.metadata, initialMetadata);
    completed('gui-metadata-created');

    let patchRequests = 0;
    const origins = new Set<string>();
    page.on('request', request => {
      const url = new URL(request.url());
      if (url.pathname.startsWith('/v1/')) origins.add(url.origin);
      if (url.pathname === `/v1/documents/${documentId}/metadata` && request.method() === 'PATCH') patchRequests++;
    });
    await page.goto(`/documents/${documentId}?view=authoring&tab=overview`);
    await page.getByRole('link', { name: '編集作業', exact: true }).press('Enter');
    await expect(page.getByRole('table', { name: '文書一覧', exact: true })).toBeVisible();
    await fillPrivate(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);
    const initialFilters = { documentType: initialMetadata.document_type, owningDepartment: initialMetadata.owning_department, category: initialMetadata.category };
    await fillMetadataListFilters(page, initialFilters);
    await readFilteredList(page, initialFilters, [documentId]);
    expect(new URL(page.url()).searchParams.get('view')).toBe('authoring');
    await page.locator(`[data-document-id="${documentId}"]`).press('Enter');
    await page.getByRole('button', { name: '詳細を開く', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === `/documents/${documentId}`).toBe(true);
    await page.getByRole('button', { name: '← 一覧へ戻る', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === '/documents').toBe(true);
    await inputEquals(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);
    await inputEquals(page.getByLabel('文書種別', { exact: true }), initialFilters.documentType);
    await inputEquals(page.getByLabel('所管部署', { exact: true }), initialFilters.owningDepartment);
    await inputEquals(page.getByLabel('カテゴリ', { exact: true }), initialFilters.category);
    await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeFocused();
    const mismatchedFilters = { ...initialFilters, category: 'synthetic-nonmatching-category' };
    await fillMetadataListFilters(page, mismatchedFilters);
    await readFilteredList(page, mismatchedFilters, []);
    await readFilteredList(page, { documentType: '', owningDepartment: '', category: '' }, [documentId], true);
    await inputEquals(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);
    for (const name of ['文書種別', '所管部署', 'カテゴリ']) await inputEquals(page.getByLabel(name, { exact: true }), '');
    expect(new URL(page.url()).searchParams.get('view')).toBe('authoring');
    await page.locator(`[data-document-id="${documentId}"]`).press('Enter');
    await page.getByRole('button', { name: '詳細を開く', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === `/documents/${documentId}`).toBe(true);
    expect(patchRequests).toBe(0);
    privatelyEqual(await detail('authoring'), before);
    privatelyEqual(await version('authoring'), beforeVersion);
    expect(await revisions()).toHaveLength(0);
    const openEditor = page.getByRole('button', { name: 'メタデータを編集', exact: true });
    await expect(openEditor).toBeEnabled();
    await openEditor.press('Enter');
    const dialog = editor(page);
    await expect(dialog).toBeVisible();
    await inputEquals(dialog.getByLabel('文書種別', { exact: true }), initialMetadata.document_type);
    await inputEquals(dialog.getByLabel('所管部署', { exact: true }), initialMetadata.owning_department);
    await inputEquals(dialog.getByLabel('カテゴリ', { exact: true }), initialMetadata.category);
    await fillPrivate(dialog.getByLabel('文書種別', { exact: true }), 'synthetic-discarded-value');
    await dialog.getByRole('checkbox', { name: 'カテゴリを削除', exact: true }).check();
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
    await expect(dialog).toBeHidden();
    await expect(openEditor).toBeFocused();
    expect(patchRequests).toBe(0);
    await openEditor.press('Enter');
    await inputEquals(dialog.getByLabel('文書種別', { exact: true }), initialMetadata.document_type);
    await inputEquals(dialog.getByLabel('変更理由', { exact: true }), '');
    await expect(dialog.getByRole('checkbox', { name: 'カテゴリを削除', exact: true })).not.toBeChecked();
    completed('gui-metadata-cancel-verified');

    const workingSet = { document_type: '', owning_department: '   ', category: 'synthetic-working-category' };
    await fillPrivate(dialog.getByLabel('文書種別', { exact: true }), workingSet.document_type);
    await fillPrivate(dialog.getByLabel('所管部署', { exact: true }), workingSet.owning_department);
    await fillPrivate(dialog.getByLabel('カテゴリ', { exact: true }), workingSet.category);
    const workingReason = 'Synthetic unpublished metadata acceptance';
    const working = await saveMetadata(page, documentId, workingReason);
    privatelyEqual(working.body, { operationId: working.body.operationId, expectedDocumentRevision: before.revision,
      set: workingSet, unset: [], reason: workingReason });
    expect(working.result).toMatchObject({ operationId: working.body.operationId, resourceId: documentId,
      changed: true, resultingRevision: before.revision + 1 });
    const unpublished = await detail('authoring');
    privatelyEqual(unpublished.metadata, { ...initialMetadata, ...workingSet });
    expect(unpublished.revision).toBe(before.revision + 1);
    expect(unpublished.currentVersionId).toBeNull();
    expect(unpublished.displayRevision).toBeNull();
    privatelyEqual(unpublished.readState, before.readState);
    privatelyEqual(await version('authoring'), beforeVersion);
    privatelyEqual((await listVersionFiles({ ...common, path: versionPath, query: { purpose: 'authoring' } })).data, beforeFiles);
    expect((await listDocumentVersions({ ...common, path, query: { purpose: 'authoring', pageSize: 100 } })).data.items).toHaveLength(1);
    expect(await revisions()).toHaveLength(0);
    expect(patchRequests).toBe(1);
    completed('gui-metadata-working-verified');

    // 公開は既存の独立コマンドのまま。metadata GUIは正式番号を生成せず、
    // Version metadataも変更しない。
    await publishVersion({ ...common, path: versionPath, body: { operationId: uuidV7(), expectedRevision: unpublished.revision } });
    const published = await persistedSnapshot(context.human, documentId);
    privatelyEqual(await persistedSnapshot(context.agent, documentId), published);
    expect(published.revisions).toHaveLength(1);
    expect(published.revisions[0]).toMatchObject({ documentVersionId: created.documentVersionId, major: 1, minor: 0, sourceKind: 'initialPublication' });
    expect(published.versions).toHaveLength(1);
    expect(published.versions[0]!.files[0]!.hash).toBe(hash(original));
    const publishedVersion = await version('published');
    const publishedDetail = await detail('published');
    expect(typeof publishedDetail.createdAt === 'string' && publishedDetail.createdAt.length > 0).toBe(true);
    const createdAt = publishedDetail.createdAt!;
    // Dateは既知の合成createdAtを含む分幅・期待表示のfixtureだけに使う。
    const createdMilliseconds = new Date(createdAt).getTime();
    expect(Number.isFinite(createdMilliseconds)).toBe(true);
    const minuteStart = Math.floor(createdMilliseconds / 60_000) * 60_000;
    const localStart = new Date(minuteStart + 9 * 60 * 60 * 1000).toISOString().slice(0, 16);
    const exactLocal = new Date(minuteStart).toISOString() === createdAt ? localStart : null;
    const exactFrom = { createdFrom: createdAt, createdBefore: null };
    const fromReturnTo = `/documents?${new URLSearchParams({ view: 'published', titleContains: listTitle, createdFrom: createdAt })}`;
    const agentReadState = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState;
    await page.goto(`/documents/${documentId}?view=published&tab=overview&returnTo=${encodeURIComponent(fromReturnTo)}`);
    await expect(openEditor).toBeEnabled();
    completed('gui-metadata-published-verified');

    await openEditor.press('Enter');
    await inputEquals(dialog.getByLabel('文書種別', { exact: true }), '');
    await inputEquals(dialog.getByLabel('所管部署', { exact: true }), workingSet.owning_department);
    const publishedSet = { document_type: 'synthetic-published-type' };
    await fillPrivate(dialog.getByLabel('文書種別', { exact: true }), publishedSet.document_type);
    await dialog.getByRole('checkbox', { name: 'カテゴリを削除', exact: true }).check();
    const publishedReason = 'Synthetic minor metadata acceptance';
    const updated = await saveMetadata(page, documentId, publishedReason);
    privatelyEqual(updated.body, { operationId: updated.body.operationId, expectedDocumentRevision: publishedDetail.revision,
      set: publishedSet, unset: ['category'], reason: publishedReason });
    expect(updated.body.operationId === working.body.operationId).toBe(false);
    expect(updated.result).toMatchObject({ changed: true, resultingRevision: publishedDetail.revision + 1 });
    const { category: removedCategory, ...retainedMetadata } = { ...initialMetadata, ...workingSet };
    void removedCategory;
    const expectedMetadata = { ...retainedMetadata, ...publishedSet };
    const after = await persistedSnapshot(context.human, documentId);
    privatelyEqual(after.metadata, expectedMetadata);
    privatelyEqual(await persistedSnapshot(context.agent, documentId), after);
    expect(after.revisions).toHaveLength(2);
    expect(after.revisions[0]).toMatchObject({ documentVersionId: created.documentVersionId, major: 1, minor: 1, sourceKind: 'metadataRevision' });
    privatelyEqual(after.revisions[1], published.revisions[0]);
    expect(after.currentVersionId).toBe(published.currentVersionId);
    privatelyEqual(after.versions, published.versions);
    privatelyEqual(after.publications, published.publications);
    privatelyEqual(await version('published'), publishedVersion);
    privatelyEqual((await detail('published')).readState, publishedDetail.readState);
    privatelyEqual((await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState, agentReadState);
    expect(patchRequests).toBe(2);
    completed('gui-metadata-minor-verified');

    await openEditor.press('Enter');
    await inputEquals(dialog.getByLabel('カテゴリ', { exact: true }), '');
    await expect(dialog.getByRole('checkbox', { name: 'カテゴリを削除', exact: true })).not.toBeChecked();
    const noOpReason = 'Synthetic unchanged metadata acceptance';
    const unchanged = await saveMetadata(page, documentId, noOpReason);
    privatelyEqual(unchanged.body, { operationId: unchanged.body.operationId, expectedDocumentRevision: after.revision,
      set: {}, unset: [], reason: noOpReason });
    expect(unchanged.body.operationId === updated.body.operationId).toBe(false);
    expect(unchanged.result).toMatchObject({ changed: false, resultingRevision: after.revision });
    privatelyEqual(await persistedSnapshot(context.human, documentId), after);
    privatelyEqual(await persistedSnapshot(context.agent, documentId), after);
    const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
    // 既存履歴投影はno-op台帳も同じactionCodeで返す。実変更と区別する。
    const metadataHistory = history.items.filter(item => item.actionCode === 'document.metadata.changed');
    expect(metadataHistory.filter(item => item.details.changed === true)).toHaveLength(2);
    expect(metadataHistory.filter(item => item.details.changed === false)).toHaveLength(1);
    expect(patchRequests).toBe(3);
    expect(origins).toEqual(new Set([context.human]));
    completed('gui-metadata-noop-verified');
    // 既存detailのreturnToへ試験seedを置き、server原文と開始包含を実GETで検査する。
    const fromResponse = waitCreatedList(page, exactFrom);
    await page.getByRole('button', { name: '← 一覧へ戻る', exact: true }).press('Enter');
    await verifyCreatedList(page, await fromResponse, exactFrom, [{ documentId, createdAt }]);
    await inputEquals(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);
    await verifyCreatedInput(page, '作成日時の開始（含む）', createdAt, exactLocal);
    await readUnreadPublishedList(page, documentId, created.documentVersionId, exactFrom);
    await page.locator(`[data-document-id="${documentId}"]`).press('Enter');
    await page.getByRole('button', { name: '詳細を開く', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === `/documents/${documentId}`).toBe(true);
    await page.getByRole('button', { name: '← 一覧へ戻る', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === '/documents').toBe(true);
    await expect(page.getByRole('checkbox', { name: '未読のみ', exact: true })).toBeChecked();
    expect(new URL(page.url()).searchParams.get('unreadOnly')).toBe('true');
    privatelyEqual(createdListRange(new URL(page.url()).searchParams), exactFrom);
    await verifyCreatedInput(page, '作成日時の開始（含む）', createdAt, exactLocal);
    await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeFocused();
    const minuteRange = { createdFrom: new Date(minuteStart).toISOString(), createdBefore: new Date(minuteStart + 60_000).toISOString() };
    if (exactLocal === null) await page.getByRole('button', { name: '作成日時の開始を指定し直す', exact: true }).press('Enter');
    else await expect(page.getByRole('button', { name: '作成日時の開始を指定し直す', exact: true })).toBeHidden();
    await fillPrivate(page.getByLabel('作成日時の開始（含む）', { exact: true }), localStart);
    await fillPrivate(page.getByLabel('作成日時の終了（含まない）', { exact: true }), new Date(minuteStart + 60_000 + 9 * 60 * 60 * 1000).toISOString().slice(0, 16));
    const calendarResponse = waitCreatedList(page, minuteRange);
    await page.getByRole('button', { name: '絞り込む', exact: true }).press('Enter');
    await verifyCreatedList(page, await calendarResponse, minuteRange, [{ documentId, createdAt }]);
    privatelyEqual(createdListRange(new URL(page.url()).searchParams), minuteRange);
    expect(new URL(page.url()).searchParams.get('unreadOnly')).toBe('true');
    // 日時解除も有効なQuery cacheを再利用し得るため、無条件に新responseを待たない。
    await page.getByRole('button', { name: '日時の条件を解除', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).searchParams.has('createdFrom') || new URL(page.url()).searchParams.has('createdBefore')).toBe(false);
    expect(new URL(page.url()).searchParams.get('titleContains') === listTitle).toBe(true);
    expect(new URL(page.url()).searchParams.get('unreadOnly')).toBe('true');
    expect(new URL(page.url()).searchParams.has('cursor')).toBe(false);
    await expect(page.getByRole('checkbox', { name: '未読のみ', exact: true })).toBeChecked();
    await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeVisible();
    await page.getByRole('checkbox', { name: '未読のみ', exact: true }).uncheck();
    // Off may reuse the valid 15-second Query cache; no unconditional response wait.
    await page.getByRole('button', { name: '絞り込む', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).searchParams.has('unreadOnly')).toBe(false);
    expect(new URL(page.url()).searchParams.has('cursor')).toBe(false);
    await expect(page.getByRole('checkbox', { name: '未読のみ', exact: true })).not.toBeChecked();
    await expect(page.locator(`[data-document-id="${documentId}"]`)).toBeVisible();
    privatelyEqual((await getDocument({ ...common, path, query: { view: 'published' } })).data.readState, publishedDetail.readState);
    privatelyEqual((await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState, agentReadState);
    expect(patchRequests).toBe(3);
    expect(origins).toEqual(new Set([context.human]));
    completed('gui-unread-readonly-verified');
    await saveSnapshot(metadataContext(context), 'gui-metadata', documentId);
    completed('gui-metadata-snapshot-saved');
  });
} else if (process.env.KP_POC_RUNTIME_PHASE === 'persistence') {
  test('両composition rootの再起動後もmetadataを復元し、値を外部へ添付しない', async ({ page }) => {
    const context = await runtime();
    const state = JSON.parse(await readFile(metadataContext(context).statePath, 'utf8')) as PersistedState;
    expect(state.documents.map(item => item.key)).toEqual(['gui-metadata']);
    const snapshot = state.documents[0]!.snapshot;
    privatelyEqual(await persistedSnapshot(context.human, snapshot.documentId), snapshot);
    privatelyEqual(await persistedSnapshot(context.agent, snapshot.documentId), snapshot);
    const path = { documentId: snapshot.documentId };
    const humanDetail = (await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data;
    const humanReadState = humanDetail.readState;
    expect(typeof humanDetail.createdAt === 'string' && humanDetail.createdAt.length > 0).toBe(true);
    const createdAt = humanDetail.createdAt!;
    const createdMilliseconds = new Date(createdAt).getTime();
    expect(Number.isFinite(createdMilliseconds)).toBe(true);
    const minuteStart = Math.floor(createdMilliseconds / 60_000) * 60_000;
    const exactLocal = new Date(minuteStart).toISOString() === createdAt
      ? new Date(minuteStart + 9 * 60 * 60 * 1000).toISOString().slice(0, 16) : null;
    const exactBefore = { createdFrom: null, createdBefore: createdAt };
    const beforeReturnTo = `/documents?${new URLSearchParams({ view: 'published', titleContains: listTitle, createdBefore: createdAt })}`;
    await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview&returnTo=${encodeURIComponent(beforeReturnTo)}`);
    await page.getByRole('button', { name: 'メタデータを編集', exact: true }).press('Enter');
    const dialog = editor(page), metadata = snapshot.metadata as Record<string, unknown>;
    await inputEquals(dialog.getByLabel('文書種別', { exact: true }), metadata.document_type as string);
    await inputEquals(dialog.getByLabel('所管部署', { exact: true }), metadata.owning_department as string);
    await inputEquals(dialog.getByLabel('カテゴリ', { exact: true }), '');
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
    completed('gui-metadata-restart-verified');
    // 再起動後も同じDocumentのcreatedAtを終了境界へそのまま渡し、終了除外を検査する。
    const beforeResponse = waitCreatedList(page, exactBefore);
    await page.getByRole('button', { name: '← 一覧へ戻る', exact: true }).press('Enter');
    await verifyCreatedList(page, await beforeResponse, exactBefore, []);
    privatelyEqual(createdListRange(new URL(page.url()).searchParams), exactBefore);
    await verifyCreatedInput(page, '作成日時の終了（含まない）', createdAt, exactLocal);
    await page.getByRole('button', { name: '日時の条件を解除', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).searchParams.has('createdFrom') || new URL(page.url()).searchParams.has('createdBefore')).toBe(false);
    expect(new URL(page.url()).searchParams.get('titleContains') === listTitle).toBe(true);
    expect(new URL(page.url()).searchParams.has('cursor')).toBe(false);
    await expect(page.getByRole('table', { name: '文書一覧', exact: true })).toBeVisible();
    await inputEquals(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);
    const retainedFilters = { documentType: metadata.document_type as string, owningDepartment: metadata.owning_department as string, category: '' };
    privatelyEqual(retainedFilters, { documentType: 'synthetic-published-type', owningDepartment: '   ', category: '' });
    expect(Object.prototype.hasOwnProperty.call(metadata, 'category')).toBe(false);
    await fillMetadataListFilters(page, retainedFilters);
    await readFilteredList(page, retainedFilters, [snapshot.documentId]);
    expect(new URL(page.url()).searchParams.get('view')).toBe('published');
    const agentReadState = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState;
    await readUnreadPublishedList(page, snapshot.documentId, snapshot.currentVersionId!);
    privatelyEqual((await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data.readState, humanReadState);
    privatelyEqual((await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState, agentReadState);
    completed('gui-unread-restart-readonly-verified');
    const removedCategoryFilters = { ...retainedFilters, category: 'synthetic-working-category' };
    await fillMetadataListFilters(page, removedCategoryFilters);
    await readFilteredList(page, removedCategoryFilters, []);
    privatelyEqual(await persistedSnapshot(context.human, snapshot.documentId), snapshot);
    privatelyEqual(await persistedSnapshot(context.agent, snapshot.documentId), snapshot);
  });
}
