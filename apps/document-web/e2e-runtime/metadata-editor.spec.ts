import { test, expect, type Locator, type Page, type Request, type Response } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, getDocumentVersion,
  listDocumentRevisions, listDocumentVersions, listVersionFiles, moveDocument, publishVersion,
  type CommandsMetadataPatch, type CommandsMoveDocument, type FolderChildren, type GuiReadState, type MutationResult, type PublishedDocument,
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
type MetadataState = PersistedState & { move: {
  request: CommandsMoveDocument; receipt: MutationResult; folderId: string; humanReadState: GuiReadState; agentReadState: GuiReadState;
} };
const privatelyEqual = (actual: unknown, expected: unknown) => expect(isDeepStrictEqual(actual, expected)).toBe(true);
const inputEquals = async (field: Locator, value: string) => {
  await expect.poll(async () => (await field.inputValue()) === value).toBe(true);
};
const fillPrivate = async (field: Locator, value: string) => {
  try { await field.fill(value); }
  catch { throw new Error('合成metadataの入力操作に失敗しました'); }
};
const editor = (page: Page) => page.getByRole('dialog', { name: 'メタデータを編集', exact: true });
type MoveReadRoute = 'document' | 'root-children' | 'destination-children';
type MoveRead = { route: MoveReadRoute; started: number; finished?: number; status?: number; response?: Response };
function moveReadRoute(url: URL, method: string, human: string, documentId: string, rootId: string, destinationId: string): MoveReadRoute | undefined {
  if (url.origin !== human || method !== 'GET') return undefined;
  if (url.pathname === `/v1/documents/${documentId}` && url.searchParams.get('view') === 'published') return 'document';
  if (url.searchParams.get('pageSize') !== '200' || url.searchParams.has('cursor')) return undefined;
  if (url.pathname === `/v1/folders/${rootId}/children`) return 'root-children';
  if (url.pathname === `/v1/folders/${destinationId}/children`) return 'destination-children';
  return undefined;
}
function moveReadPrecedesPost(read: { started: number; finished?: number; status?: number }, boundary: number, post: number): boolean {
  return read.started > boundary && read.finished !== undefined && read.finished > read.started && read.finished < post && read.status === 200;
}
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

    let patchRequests = 0, moveRequests = 0;
    const origins = new Set<string>();
    page.on('request', request => {
      const url = new URL(request.url());
      if (url.pathname.startsWith('/v1/')) origins.add(url.origin);
      if (url.pathname === `/v1/documents/${documentId}/metadata` && request.method() === 'PATCH') patchRequests++;
      if (url.pathname === `/v1/documents/${documentId}:move` && request.method() === 'POST') moveRequests++;
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

    // 既存合成文書を通常の一覧→詳細→可視ツリーから、同権限のSandboxへ1回だけ移動する。
    const sourceBefore = (await getDocument({ ...common, path, query: { view: 'published' } })).data;
    const agentBefore = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data;
    expect(sourceBefore.documentId === documentId && sourceBefore.title === after.title).toBe(true);
    expect(sourceBefore.folderId === context.manifest.folders.shared.folderId).toBe(true);
    expect(typeof sourceBefore.folderName === 'string' && sourceBefore.folderName.length > 0).toBe(true);
    expect(sourceBefore.revision === after.revision).toBe(true);
    expect(sourceBefore.capabilities.moveDocument.status).toBe('available');
    privatelyEqual(sourceBefore.readState, publishedDetail.readState);
    privatelyEqual(agentBefore.readState, agentReadState);
    await page.locator(`[data-document-id="${documentId}"]`).press('Enter');
    await page.getByRole('button', { name: '詳細を開く', exact: true }).press('Enter');
    await expect.poll(() => new URL(page.url()).pathname === `/documents/${documentId}`).toBe(true);

    expect(typeof context.manifest.rootFolderId === 'string').toBe(true);
    const rootFolderId = context.manifest.rootFolderId!, sandboxId = context.manifest.folders.sandbox.folderId;
    const movePath = `/v1/documents/${documentId}:move`;
    let moveSequence = 0, movePostOrder = 0, moveReadOverflow = false;
    const moveReads = new Map<Request, MoveRead>();
    let readsAtPost: MoveRead[] = [];
    const recordMoveRequest = (request: Request) => {
      const url = new URL(request.url());
      if (url.origin === context.human && url.pathname === movePath && request.method() === 'POST') {
        movePostOrder = ++moveSequence;
        // コピーをPOST開始時に固定する。以後のreset GET/完了イベントで遡及的に合格させない。
        readsAtPost = [...moveReads.values()].map(read => ({ ...read }));
        return;
      }
      const route = moveReadRoute(url, request.method(), context.human, documentId, rootFolderId, sandboxId);
      if (!route) return;
      if (moveReads.size >= 24) { moveReadOverflow = true; return; }
      moveReads.set(request, { route, started: ++moveSequence });
    };
    const recordMoveResponse = (response: Response) => {
      const read = moveReads.get(response.request());
      if (read) { read.status = response.status(); read.response = response; }
    };
    const recordMoveReadFinished = (request: Request) => {
      const read = moveReads.get(request);
      if (read) read.finished = ++moveSequence;
    };
    page.on('request', recordMoveRequest);
    page.on('response', recordMoveResponse);
    page.on('requestfinished', recordMoveReadFinished);
    const isMoveRead = (response: Response, route: MoveReadRoute) =>
      moveReadRoute(new URL(response.url()), response.request().method(), context.human, documentId, rootFolderId, sandboxId) === route;
    const moveEntry = page.getByRole('button', { name: '文書を移動', exact: true });
    await expect(moveEntry).toBeEnabled();
    const openingBoundary = moveSequence;
    const openingSource = page.waitForResponse(response => isMoveRead(response, 'document')
      && (moveReads.get(response.request())?.started ?? 0) > openingBoundary);
    await moveEntry.press('Enter');
    const moveDialog = page.getByRole('dialog', { name: '文書を移動', exact: true });
    await expect(moveDialog).toBeVisible();
    const openingRead = await openingSource;
    expect(openingRead.status()).toBe(200);
    expect(await openingRead.finished()).toBeNull();
    privatelyEqual(await openingRead.json(), sourceBefore);
    const candidates = moveDialog.getByRole('group', { name: '移動先フォルダー', exact: true });
    const sandboxEntry = candidates.getByRole('button', { name: 'Agent Sandbox', exact: true });
    await expect(sandboxEntry).toBeEnabled();
    const selectionBoundary = moveSequence;
    const selectionRows = page.waitForResponse(response => isMoveRead(response, 'root-children')
      && (moveReads.get(response.request())?.started ?? 0) > selectionBoundary);
    const selectionChildren = page.waitForResponse(response => isMoveRead(response, 'destination-children')
      && (moveReads.get(response.request())?.started ?? 0) > selectionBoundary);
    await sandboxEntry.press('Enter');
    const rootRead = await selectionRows, sandboxRead = await selectionChildren;
    expect(rootRead.status()).toBe(200); expect(sandboxRead.status()).toBe(200);
    expect(await rootRead.finished()).toBeNull(); expect(await sandboxRead.finished()).toBeNull();
    const rootRows = await rootRead.json() as FolderChildren;
    const destinationChildren = await sandboxRead.json() as FolderChildren;
    expect(rootRows.nextCursor).toBeNull();
    const sandbox = rootRows.items.find(row => row.folderId === sandboxId)!;
    expect(Boolean(sandbox) && sandbox.name === 'Agent Sandbox' && sandbox.parentFolderId === rootFolderId).toBe(true);
    const moveReason = 'Synthetic same-policy document move acceptance';
    await fillPrivate(moveDialog.getByLabel('移動理由', { exact: true }), moveReason);
    for (const text of [`対象文書ID：${sourceBefore.documentId}`, `元フォルダーID：${sourceBefore.folderId}`,
      `現在の元所属名：${sourceBefore.folderName}`, `現在の対象名：${sourceBefore.title}（revision ${sourceBefore.revision}）`,
      `移動先フォルダーID：${sandbox.folderId}`, `移動先名：${sandbox.name}`]) {
      await expect.poll(async () => (await moveDialog.textContent())?.includes(text)).toBe(true);
    }
    await expect(moveDialog).toContainText('明示アクセス設定は保持。継承中は移動先の設定が適用され、自分を含む閲覧・編集権限が変わり得る');
    const moveConfirmation = moveDialog.getByRole('checkbox', { name: 'アクセス設定への影響を確認しました', exact: true });
    await expect(moveConfirmation).not.toBeChecked();
    await expect(moveDialog.getByRole('button', { name: '移動する', exact: true })).toBeDisabled();
    expect(moveRequests).toBe(0);
    await moveConfirmation.check();
    await expect(moveConfirmation).toBeChecked();
    expect(moveRequests).toBe(0);
    const sendingBoundary = moveSequence;
    const moveResponse = page.waitForResponse(response => {
      const url = new URL(response.url());
      return url.origin === context.human && url.pathname === movePath && response.request().method() === 'POST';
    });
    await moveDialog.getByRole('button', { name: '移動する', exact: true }).press('Enter');
    const moved = await moveResponse;
    expect(moved.status()).toBe(200);
    expect(moveReadOverflow).toBe(false);
    const submissionReads = readsAtPost.filter(read => moveReadPrecedesPost(read, sendingBoundary, movePostOrder));
    privatelyEqual(submissionReads.map(read => read.route), ['document', 'root-children', 'destination-children']);
    privatelyEqual(await submissionReads[0]!.response!.json(), sourceBefore);
    const currentRootRows = await submissionReads[1]!.response!.json() as FolderChildren;
    expect(currentRootRows.nextCursor).toBeNull();
    privatelyEqual(currentRootRows.items.find(row => row.folderId === sandbox.folderId), sandbox);
    privatelyEqual(await submissionReads[2]!.response!.json(), destinationChildren);
    const moveRequest = moved.request().postDataJSON() as CommandsMoveDocument;
    expect(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(moveRequest.operationId)).toBe(true);
    privatelyEqual(moveRequest, { operationId: moveRequest.operationId, fromFolderId: sourceBefore.folderId,
      toFolderId: sandbox.folderId, expectedDocumentRevision: sourceBefore.revision, reason: moveReason });
    const moveReceipt = await moved.json() as MutationResult;
    privatelyEqual(Object.keys(moveReceipt).sort(), ['changed', 'occurredAt', 'operationId', 'resourceId', 'resultingRevision']);
    expect(moveReceipt.operationId === moveRequest.operationId && moveReceipt.resourceId === documentId).toBe(true);
    expect(moveReceipt.changed).toBe(true);
    expect(Number.isSafeInteger(moveReceipt.resultingRevision) && moveReceipt.resultingRevision === sourceBefore.revision + 1).toBe(true);
    expect(Number.isFinite(Date.parse(moveReceipt.occurredAt))).toBe(true);
    await expect(moveDialog.getByText('文書を移動しました。', { exact: true })).toBeVisible();
    await expect(moveDialog.getByText('表示を更新中です。', { exact: true })).toHaveCount(0);
    await expect(moveDialog.getByText('表示を更新できませんでした。移動結果は確定しています。読取を再試行してください。', { exact: true })).toHaveCount(0);
    await moveDialog.getByRole('button', { name: '確認して閉じる', exact: true }).press('Enter');
    await expect(moveDialog).toBeHidden();
    page.off('request', recordMoveRequest);
    page.off('response', recordMoveResponse);
    page.off('requestfinished', recordMoveReadFinished);
    const movedHuman = (await getDocument({ ...common, path, query: { view: 'published' } })).data;
    const movedAgent = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data;
    // folder/revision以外の正式表示・日時・既読もactorごとの移動前値を保持する。
    for (const [current, previous] of [[movedHuman, sourceBefore], [movedAgent, agentBefore]]) {
      expect(current!.folderId === sandbox.folderId && current!.folderName === sandbox.name).toBe(true);
      expect(current!.revision === moveReceipt.resultingRevision).toBe(true);
      privatelyEqual({ displayRevision: current!.displayRevision, displayVersion: current!.displayVersion,
        currentVersionId: current!.currentVersionId, createdAt: current!.createdAt,
        publishedAt: 'publishedAt' in current! ? current.publishedAt : undefined, readState: current!.readState },
      { displayRevision: previous!.displayRevision, displayVersion: previous!.displayVersion,
        currentVersionId: previous!.currentVersionId, createdAt: previous!.createdAt,
        publishedAt: 'publishedAt' in previous! ? previous.publishedAt : undefined, readState: previous!.readState });
    }
    const movedSnapshot = { ...after, revision: moveReceipt.resultingRevision };
    privatelyEqual(await persistedSnapshot(context.human, documentId), movedSnapshot);
    privatelyEqual(await persistedSnapshot(context.agent, documentId), movedSnapshot);
    expect(moveRequests).toBe(1); expect(patchRequests).toBe(3);
    expect(origins).toEqual(new Set([context.human]));
    completed('gui-document-move-verified');
    await saveSnapshot(metadataContext(context), 'gui-metadata', documentId);
    const metadataState = JSON.parse(await readFile(metadataContext(context).statePath, 'utf8')) as PersistedState;
    privatelyEqual(metadataState.documents[0]!.snapshot, movedSnapshot);
    const move: MetadataState['move'] = { request: moveRequest, receipt: moveReceipt, folderId: sandbox.folderId,
      humanReadState: sourceBefore.readState, agentReadState: agentBefore.readState };
    await writeFile(metadataContext(context).statePath, JSON.stringify({ ...metadataState, move }, null, 2), { mode: 0o600 });
    completed('gui-metadata-snapshot-saved');
  });
} else if (process.env.KP_POC_RUNTIME_PHASE === 'persistence') {
  test('両composition rootの再起動後もmetadataを復元し、値を外部へ添付しない', async ({ page }) => {
    const context = await runtime();
    const state = JSON.parse(await readFile(metadataContext(context).statePath, 'utf8')) as MetadataState;
    expect(state.documents.map(item => item.key)).toEqual(['gui-metadata']);
    const snapshot = state.documents[0]!.snapshot;
    privatelyEqual(await persistedSnapshot(context.human, snapshot.documentId), snapshot);
    privatelyEqual(await persistedSnapshot(context.agent, snapshot.documentId), snapshot);
    const path = { documentId: snapshot.documentId };
    const humanDetail = (await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data;
    const agentDetail = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data;
    for (const detail of [humanDetail, agentDetail]) {
      expect(detail.folderId === state.move.folderId && detail.folderId === context.manifest.folders.sandbox.folderId).toBe(true);
      expect(detail.revision === snapshot.revision).toBe(true);
    }
    privatelyEqual(humanDetail.readState, state.move.humanReadState);
    privatelyEqual(agentDetail.readState, state.move.agentReadState);
    // 現所属へ作り直さず、同一Human・同一旧Shared要求を既存2 HTTP再起動後に再送する。
    const replay = await moveDocument({ ...options(context.human), path, body: state.move.request });
    expect(replay.response.status).toBe(200);
    privatelyEqual(replay.data, state.move.receipt);
    const replayedHuman = (await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data;
    const replayedAgent = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data;
    privatelyEqual(replayedHuman, humanDetail);
    privatelyEqual(replayedAgent, agentDetail);
    privatelyEqual(await persistedSnapshot(context.human, snapshot.documentId), snapshot);
    privatelyEqual(await persistedSnapshot(context.agent, snapshot.documentId), snapshot);
    completed('gui-document-move-replay-verified');
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
