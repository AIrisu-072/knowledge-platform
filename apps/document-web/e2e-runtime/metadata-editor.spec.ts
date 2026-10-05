import { test, expect, type Locator, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, getDocumentVersion,
  listDocumentRevisions, listDocumentVersions, listVersionFiles, publishVersion,
  type CommandsMetadataPatch, type MutationResult,
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
    const detail = () => getDocument({ ...common, path, query: { view: 'authoring' } }).then(response => response.data);
    const version = () => getDocumentVersion({ ...common, path: versionPath, query: { purpose: 'authoring' } }).then(response => response.data);
    const revisions = () => listDocumentRevisions({ ...common, path, query: { pageSize: 100 } }).then(response => response.data.items);
    const before = await detail(), beforeVersion = await version();
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
    const unpublished = await detail();
    privatelyEqual(unpublished.metadata, { ...initialMetadata, ...workingSet });
    expect(unpublished.revision).toBe(before.revision + 1);
    expect(unpublished.currentVersionId).toBeNull();
    expect(unpublished.displayRevision).toBeNull();
    privatelyEqual(unpublished.readState, before.readState);
    privatelyEqual(await version(), beforeVersion);
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
    const publishedVersion = await version();
    const publishedDetail = await detail();
    const agentReadState = (await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.readState;
    await page.reload();
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
    privatelyEqual(await version(), publishedVersion);
    privatelyEqual((await detail()).readState, publishedDetail.readState);
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
    await page.goto(`/documents/${snapshot.documentId}?view=authoring&tab=overview`);
    await page.getByRole('button', { name: 'メタデータを編集', exact: true }).press('Enter');
    const dialog = editor(page), metadata = snapshot.metadata as Record<string, unknown>;
    await inputEquals(dialog.getByLabel('文書種別', { exact: true }), metadata.document_type as string);
    await inputEquals(dialog.getByLabel('所管部署', { exact: true }), metadata.owning_department as string);
    await inputEquals(dialog.getByLabel('カテゴリ', { exact: true }), '');
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
    completed('gui-metadata-restart-verified');
  });
}
