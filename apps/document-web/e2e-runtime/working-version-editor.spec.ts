import { test as base, expect, type Page } from '@playwright/test';
import { createRequire } from 'node:module';
import { BinaryTransportBridge, getDocument, getVersionEditManifest, getDocumentVersion,
  publishVersion, type VersionMutationResult,
} from '@knowledge-platform/document-api-client';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { hash, options, runtime, uuidV7 } from './support';
import { lifecycleSnapshot } from './lifecycle-support';
import { createMultiOriginalFixture, originalBytes, renditionBytes, sourceNames,
  saveWorkingEditorSnapshot, workingEditorSnapshot } from './working-version-support';

type LossReceipt = { received: number; dispatched: number; dropped: number; unexpected: number;
  upstreamStatus: number; retryStatus?: number; bytesEqual?: boolean; contentTypeEqual?: boolean;
  result: VersionMutationResult; retryResult?: VersionMutationResult };
type WorkingLoss = { origin: string; arm(target: { method: 'POST' | 'PUT'; path: string }): void;
  dropped(): Promise<LossReceipt>; allowRetry(): void; assertRecovered(): Promise<LossReceipt>; allowPublish(path: string): void };
const { withWorkingResponseLoss } = createRequire(import.meta.url)('../../../tools/document-poc-runtime/response-loss.mjs') as {
  withWorkingResponseLoss<T>(origin: string, action: (control: WorkingLoss) => Promise<T>): Promise<T>;
};
const test = base.extend<{ loss: WorkingLoss }>({
  loss: async ({}, use) => { const context = await runtime(); await withWorkingResponseLoss(context.human, use); },
  page: async ({ browser, loss }, use) => {
    const owned = await runtime();
    // Only this page is proxied; the Agent APIRequestContext and SDK reads stay direct.
    const context = await browser.newContext({ baseURL: owned.human, proxy: { server: loss.origin },
      locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block' });
    try { await use(await context.newPage()); } finally { await context.close(); }
  },
});

// This new slice never records screenshots, traces or video on success or failure.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });
const editor = (page: Page) => page.getByRole('form', { name: '作業版の原本を編集', exact: true });
const stableRepresentations = (item: { representations: Array<{ representationId: string }> }) => item.representations.map(({ representationId: _representationId, ...entry }) => entry);

async function save(page: Page, loss: WorkingLoss, origin: string, documentId: string, method: 'POST' | 'PUT', target?: string) {
  const pathname = `/v1/documents/${documentId}/versions${target ? `/${target}` : ''}`;
  const before = await workingEditorSnapshot(origin, documentId);
  loss.arm({ method, path: pathname });
  completed('gui-working-loss-armed');
  await editor(page).getByRole('button', { name: method === 'POST' ? '新しい作業版を作成' : '作業版を保存', exact: true }).click();
  completed('gui-working-loss-save-clicked');
  const lost = await loss.dropped();
  completed('gui-working-loss-dropped');
  expect(lost).toMatchObject({ received: 1, dispatched: 1, dropped: 1, unexpected: 0, upstreamStatus: method === 'POST' ? 201 : 200 });
  await expect(page.getByRole('heading', { name: '保存結果を確認できません', exact: true })).toBeVisible();
  completed('gui-working-loss-unknown-visible');
  await expect(page.getByRole('status').filter({ hasText: method === 'POST' ? '新しい作業版を作成しました' : '作業版を保存しました' })).toHaveCount(0);
  const committed = await workingEditorSnapshot(origin, documentId);
  expect(committed.revision).toBe(before.revision + 1);
  expect(committed.currentVersionId).toBe(before.currentVersionId);
  expect(committed.versions).toHaveLength(before.versions.length + (method === 'POST' ? 1 : 0));
  expect(committed.operations.filter(item => item.sourceKey === `version_operation:${lost.result.operationId}`)).toHaveLength(1);
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === pathname && response.request().method() === method);
  loss.allowRetry();
  completed('gui-working-loss-retry-armed');
  await page.getByRole('button', { name: '同じ内容で再試行', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(method === 'POST' ? 201 : 200);
  const result = await response.json() as VersionMutationResult;
  const replay = await loss.assertRecovered();
  completed('gui-working-loss-recovered');
  expect(replay).toMatchObject({ received: 2, dispatched: 2, dropped: 1, unexpected: 0,
    bytesEqual: true, contentTypeEqual: true, retryStatus: method === 'POST' ? 201 : 200 });
  expect(result).toEqual(lost.result);
  expect(replay.retryResult).toEqual(lost.result);
  expect(await workingEditorSnapshot(origin, documentId)).toEqual(committed);
  await expect(page.getByRole('status')).toContainText(method === 'POST' ? '新しい作業版を作成しました' : '作業版を保存しました');
  return result;
}

async function openWorking(page: Page, documentId: string, versionId: string) {
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions&versionId=${versionId}`);
  await page.getByRole('button', { name: '作業版を編集', exact: true }).first().click();
  await expect(page.getByRole('heading', { name: '作業版を編集', level: 1 })).toBeVisible();
  await expect(editor(page).getByLabel(/^差替ファイル:/).first()).toBeVisible();
}

test('未検査の初回WORKING原本を修復し、同じ版ID・番号と公開なしを維持する', async ({ page, request, loss }) => {
  const context = await runtime(), common = options(context.human), bridge = new BinaryTransportBridge({ baseUrl: context.human });
  // Invalid UTF-8 is intentionally repairable before initial publication; old DSI must not be mandatory.
  const first = Buffer.from([0xff]);
  const changed = Buffer.from('【合成データ】初回編集\n公開前に修正した本文です。\n');
  const created = await bridge.createDocument({ request: { folderId: context.manifest.folders.shared.folderId,
    title: '【合成データ】初回作業版更新', documentMetadata: {}, versionMetadata: {} },
    file: new Blob([first]), originalFilename: 'initial.txt', mediaType: 'text/plain' });
  const { documentId, documentVersionId: versionId } = created;
  const before = await workingEditorSnapshot(context.human, documentId);
  await openWorking(page, documentId, versionId);
  await editor(page).getByLabel(/^差替ファイル:/).setInputFiles({ name: 'before-publication.txt', mimeType: 'text/plain', buffer: changed });
  const result = await save(page, loss, context.human, documentId, 'PUT', versionId);
  expect(result).toMatchObject({ targetVersionId: versionId, versionNo: 1, baseVersionId: null,
    resultingRevision: before.revision + 1 });
  const after = await workingEditorSnapshot(context.human, documentId);
  expect(after.currentVersionId).toBeNull();
  expect(after.versions).toHaveLength(1);
  expect(after.versions[0]).toMatchObject({ versionId, versionNo: 1, lifecycleState: 'working', baseVersionId: null });
  expect(after.files).toHaveLength(1);
  expect(after.files[0]).toMatchObject({ hash: hash(changed), logicalPath: 'primary', ordinal: 0 });
  expect(after.manifest.items[0]!.representations[0]!.originalFilename).toBe('before-publication.txt');
  expect(after.revisions).toHaveLength(0);
  const hidden = await request.get(`${context.agent}/v1/documents/${documentId}?view=published`);
  expect(hidden.status()).toBe(404);
  const deniedManifest = await request.get(`${context.agent}/v1/documents/${documentId}/versions/${versionId}/edit-manifest?purpose=authoring`);
  expect(deniedManifest.status()).toBe(404);
  completed('gui-working-initial-updated');
  await saveWorkingEditorSnapshot(context, 'initial', documentId);
  completed('gui-working-snapshot-saved');
});

test('複数原本を保持する新作業版と更新は対象変換物だけ除外し、公開時だけcurrentを切り替える', async ({ page, request, loss }) => {
  const context = await runtime(), common = options(context.human);
  const { documentId, versionId: publishedVersionId } = await createMultiOriginalFixture(context);
  const path = { documentId };
  const source = (await getVersionEditManifest({ ...common, path: { ...path, versionId: publishedVersionId }, query: { purpose: 'published' } })).data;
  expect(source.items).toHaveLength(2);
  expect(source.items.map(item => item.representations.length)).toEqual([2, 2]);
  expect(source.items.map(item => item.representations[0]!.originalFilename)).toEqual([...sourceNames]);
  const before = await workingEditorSnapshot(context.human, documentId);
  expect(before.files.map(file => file.hash).sort()).toEqual([...originalBytes, ...renditionBytes].map(bytes => hash(bytes)).sort());
  const deniedManifest = await request.get(`${context.agent}/v1/documents/${documentId}/versions/${publishedVersionId}/edit-manifest?purpose=published`);
  expect(deniedManifest.status()).toBe(404);
  completed('gui-working-manifest-ready');

  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await expect(editor(page).getByLabel(/^差替ファイル:/)).toHaveCount(2);
  await editor(page).getByRole('button', { name: '版の一覧へ戻る', exact: true }).click();
  expect(await workingEditorSnapshot(context.human, documentId)).toEqual(before);
  completed('gui-working-cancel-verified');

  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  const changedPrimary = Buffer.from('【合成データ】主文書\n選択した原本だけ差替えました。\n');
  await editor(page).getByLabel(/^差替ファイル:/).nth(0).setInputFiles({ name: 'primary-edited.txt', mimeType: 'text/plain', buffer: changedPrimary });
  await expect(editor(page)).toContainText('主 文書の変換.txt');
  const created = await save(page, loss, context.human, documentId, 'POST');
  expect(created.targetVersionId).not.toBe(publishedVersionId);
  expect(created).toMatchObject({ baseVersionId: publishedVersionId, versionNo: 2, resultingRevision: before.revision + 1 });
  const workingVersionId = created.targetVersionId;
  const firstEdit = (await getVersionEditManifest({ ...common, path: { ...path, versionId: workingVersionId }, query: { purpose: 'authoring' } })).data;
  expect(firstEdit.items.map(item => [item.logicalPath, item.ordinal])).toEqual(source.items.map(item => [item.logicalPath, item.ordinal]));
  expect(firstEdit.items.map(item => item.representations.length)).toEqual([1, 2]);
  expect(firstEdit.items[0]!.representations[0]!.fileId).not.toBe(source.items[0]!.representations[0]!.fileId);
  expect(stableRepresentations(firstEdit.items[1]!)).toEqual(stableRepresentations(source.items[1]!));
  const during = await workingEditorSnapshot(context.human, documentId);
  expect(during.currentVersionId).toBe(publishedVersionId);
  expect(during.files.filter(file => file.versionId === publishedVersionId)).toEqual(before.files);
  expect((await getVersionEditManifest({ ...common, path: { ...path, versionId: publishedVersionId }, query: { purpose: 'published' } })).data.items).toEqual(source.items);
  completed('gui-working-created');

  await openWorking(page, documentId, workingVersionId);
  const changedAppendix = Buffer.from('【合成データ】付録\n作業版で付録だけ追加修正しました。\n');
  await editor(page).getByLabel(/^差替ファイル:/).nth(1).setInputFiles({ name: 'appendix-edited.txt', mimeType: 'text/plain', buffer: changedAppendix });
  await expect(editor(page)).toContainText('付録の変換.txt');
  const updated = await save(page, loss, context.human, documentId, 'PUT', workingVersionId);
  expect(updated).toMatchObject({ targetVersionId: workingVersionId, versionNo: 2, baseVersionId: publishedVersionId, resultingRevision: created.resultingRevision + 1 });
  const finalWorking = (await getVersionEditManifest({ ...common, path: { ...path, versionId: workingVersionId }, query: { purpose: 'authoring' } })).data;
  expect(finalWorking.items.map(item => item.representations.length)).toEqual([1, 1]);
  expect(stableRepresentations(finalWorking.items[0]!)).toEqual(stableRepresentations(firstEdit.items[0]!));
  const edited = await workingEditorSnapshot(context.human, documentId);
  expect(edited.files.filter(file => file.versionId === publishedVersionId)).toEqual(before.files);
  expect(edited.currentVersionId).toBe(publishedVersionId);
  expect(edited.files.filter(file => file.versionId === workingVersionId).map(file => file.hash).sort()).toEqual([hash(changedPrimary), hash(changedAppendix)].sort());
  completed('gui-working-updated');

  const failedPublish = await publishVersion({ ...common, throwOnError: false, path: { ...path, versionId: workingVersionId },
    body: { operationId: uuidV7(), expectedRevision: created.resultingRevision } });
  expect(failedPublish.response?.status).toBe(409);
  expect(failedPublish.error?.code).toBe('REVISION_CONFLICT');
  expect(await workingEditorSnapshot(context.human, documentId)).toEqual(edited);
  expect((await getDocument({ ...options(context.agent), path, query: { view: 'published' } })).data.currentVersionId).toBe(publishedVersionId);
  completed('gui-working-publication-preserved');

  await page.goto(`/documents/${documentId}?view=authoring&tab=versions&versionId=${workingVersionId}`);
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const publishResponse = page.waitForResponse(response => new URL(response.url()).pathname === `/v1/documents/${documentId}/versions/${workingVersionId}:publish`);
  loss.allowPublish(`/v1/documents/${documentId}/versions/${workingVersionId}:publish`);
  await page.getByRole('dialog', { name: '公開を確認', exact: true }).getByRole('button', { name: '確定する', exact: true }).click();
  expect((await publishResponse).status()).toBe(200);
  await expect(page.getByRole('status')).toContainText('公開しました');
  const published = await workingEditorSnapshot(context.human, documentId);
  expect(published.currentVersionId).toBe(workingVersionId);
  expect(published.versions.map(version => version.lifecycleState)).toEqual(['published', 'published']);
  expect(published.files.filter(file => file.versionId === publishedVersionId)).toEqual(before.files);
  expect(published.revisions).toHaveLength(before.revisions.length + 1);
  const oldPublished = await getDocumentVersion({ ...common, throwOnError: false, path: { ...path, versionId: publishedVersionId }, query: { purpose: 'published' } });
  expect(oldPublished.response?.status).toBe(404);
  expect(await lifecycleSnapshot(context.agent, documentId)).toEqual(await lifecycleSnapshot(context.human, documentId));
  completed('gui-working-published');
  await saveWorkingEditorSnapshot(context, 'multiple', documentId);
  completed('gui-working-snapshot-saved');
});
