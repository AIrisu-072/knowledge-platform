import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, getDocumentVersion, getVersionEditManifest, getRootFolder, listDocumentRevisions,
  listDocumentVersions, listFolderChildren, listVersionFiles, recoverDocumentCreation,
  type CreateDocumentResult,
} from '@knowledge-platform/document-api-client';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { hash, options, persistedSnapshot, runtime, saveSnapshot } from './support';

// 初回登録の受入は失敗時も画像・trace・videoを記録しない。
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });

test('画面で初回登録したWORKINGの原本を確認し、明示公開後にagentと再起動用の状態を共有する', async ({ page, request }) => {
  const context = await runtime();
  const { human, agent, manifest } = context;
  const common = options(human);
  const root = (await getRootFolder(common)).data;
  expect(root.folderId).toBe(manifest.rootFolderId);
  expect(root.capabilities.createDocument.status).toBe('available');
  const sharedFolderId = manifest.folders.shared.folderId;
  const children = (await listFolderChildren({ ...common, path: { folderId: sharedFolderId }, query: { pageSize: 100 } })).data;
  expect(children.capabilities.createDocument.status).toBe('available');
  completed('gui-initial-capabilities-verified');

  let createRequests = 0;
  const apiOrigins = new Set<string>();
  page.on('request', req => {
    const url = new URL(req.url());
    if (url.pathname.startsWith('/v1/')) apiOrigins.add(url.origin);
    if (url.pathname === '/v1/documents' && req.method() === 'POST') createRequests++;
  });
  await page.goto('/documents?view=published');
  const openRegistration = page.getByRole('button', { name: '文書を登録', exact: true });
  await expect(openRegistration).toBeEnabled();
  await openRegistration.focus(); await page.keyboard.press('Enter');
  const registration = page.getByRole('dialog', { name: '文書を登録', exact: true });
  await expect(registration).toBeVisible();
  await expect(registration.getByText(root.name, { exact: true })).toBeVisible();
  await expect(registration.getByRole('button', { name: '下書きとして登録', exact: true })).toBeDisabled();
  await registration.getByLabel('文書名', { exact: true }).fill('取消確認専用の合成文書');
  await registration.getByRole('button', { name: 'キャンセル', exact: true }).press('Enter');
  await expect(registration).toBeHidden();
  await expect(openRegistration).toBeFocused();
  expect(createRequests).toBe(0);
  completed('gui-initial-cancel-verified');

  await page.getByRole('button', { name: 'PoC Shared', exact: true }).click();
  await expect(page).toHaveURL(url => url.searchParams.get('folderId') === sharedFolderId);
  await expect(openRegistration).toBeEnabled();
  await openRegistration.press('Enter');
  await expect(registration.getByText('PoC Shared', { exact: true })).toBeVisible();
  await expect(registration.getByLabel('文書名', { exact: true })).toHaveValue('');
  await expect(registration.getByLabel('原本ファイル', { exact: true })).toHaveValue('');
  const title = '【合成データ】GUI初回登録';
  const originalContent = Buffer.from('【合成データ】GUI初回登録\n画面操作で登録した原本です。公開前はWORKINGとして保持します。\n');
  await registration.getByLabel('文書名', { exact: true }).fill(title);
  await registration.getByLabel('原本ファイル', { exact: true }).setInputFiles({ name: 'gui-initial.txt', mimeType: 'text/plain', buffer: originalContent });
  const register = registration.getByRole('button', { name: '下書きとして登録', exact: true });
  await expect(register).toBeEnabled();
  const createResponse = page.waitForResponse(response => new URL(response.url()).pathname === '/v1/documents' && response.request().method() === 'POST');
  // 初回POSTは冪等ではない。結果不明時は失敗として停止し、登録を再送しない。
  await register.press('Enter');
  const response = await createResponse;
  expect(response.status()).toBe(201);
  const created = await response.json() as CreateDocumentResult;
  expect(createRequests).toBe(1);
  const path = { documentId: created.documentId };
  expect((await recoverDocumentCreation({ ...common, path, query: { documentVersionId: created.documentVersionId, fileId: created.fileId } })).data).toEqual(created);
  await expect(page).toHaveURL(url => url.pathname === `/documents/${created.documentId}`
    && url.searchParams.get('view') === 'authoring' && url.searchParams.get('tab') === 'versions'
    && url.searchParams.get('versionId') === created.documentVersionId);
  await expect(page.getByRole('heading', { name: title, level: 1 })).toBeVisible();
  await expect(page.getByRole('tab', { name: '版・改訂', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('button', { name: /WORKING · 版 1/ })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText('正式改訂はありません。WORKING版は上の版一覧に表示されます。', { exact: true })).toBeVisible();
  completed('gui-initial-created');

  const draft = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  expect(draft).toMatchObject({ documentId: created.documentId, title, folderId: sharedFolderId,
    lifecycleState: 'working', currentVersionId: null, displayRevision: null,
    displayVersion: { versionId: created.documentVersionId, versionNo: 1, lifecycleState: 'WORKING', isCurrent: false } });
  expect(draft.metadata).toEqual({});
  const initialVersion = (await getDocumentVersion({ ...common, path: { ...path, versionId: created.documentVersionId }, query: { purpose: 'authoring' } })).data;
  expect(initialVersion.metadata).toEqual({});
  const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'authoring', pageSize: 100 } })).data;
  expect(versions.items).toHaveLength(1);
  expect(versions.items[0]).toMatchObject({ versionId: created.documentVersionId, versionNo: 1, lifecycleState: 'working', isCurrent: false, publishedAt: null });
  expect((await listDocumentRevisions({ ...common, path, query: { pageSize: 100 } })).data.items).toHaveLength(0);
  const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  expect(history.items.filter(item => item.actionCode === 'document.version.published')).toHaveLength(0);
  const unpublished = await request.get(`${agent}/v1/documents/${created.documentId}?view=published`);
  expect(unpublished.status()).toBe(404);
  expect((await unpublished.json()).code).toBe('DOCUMENT_NOT_FOUND');
  const files = (await listVersionFiles({ ...common, path: { ...path, versionId: created.documentVersionId }, query: { purpose: 'authoring' } })).data;
  expect(files.items).toHaveLength(1);
  expect(files.items[0]).toMatchObject({ displayName: 'gui-initial.txt', mediaType: 'text/plain', sizeBytes: originalContent.length });
  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('complementary', { name: '原本と版' }).getByRole('button', { name: '現行ファイルを取得', exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe('gui-initial.txt');
  expect(await readFile((await download.path())!)).toEqual(originalContent);
  completed('gui-initial-working-verified');

  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const publish = page.getByRole('button', { name: '公開する', exact: true });
  await expect(publish).toBeDisabled();
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await expect(publish).toBeEnabled();
  await publish.click();
  const confirmation = page.getByRole('dialog', { name: '公開を確認', exact: true });
  const publishResponse = page.waitForResponse(result => new URL(result.url()).pathname === `/v1/documents/${created.documentId}/versions/${created.documentVersionId}:publish`
    && result.request().method() === 'POST');
  await confirmation.getByRole('button', { name: '確定する', exact: true }).press('Enter');
  expect((await publishResponse).status()).toBe(200);
  await expect(page.getByRole('status')).toContainText('公開しました');
  const published = await persistedSnapshot(human, created.documentId);
  expect(published.currentVersionId).toBe(created.documentVersionId);
  expect(published.revisions).toHaveLength(1);
  expect(published.revisions[0]).toMatchObject({ documentVersionId: created.documentVersionId, major: 1, minor: 0, sourceKind: 'initialPublication' });
  expect(published.versions).toHaveLength(1);
  expect(published.versions[0]).toMatchObject({ versionId: created.documentVersionId, lifecycleState: 'published', files: [{ hash: hash(originalContent) }] });
  expect(published.publications).toHaveLength(1);
  expect(published.publications[0]).toMatchObject({ actor: 'poc-human', provenanceQuality: 'operationLedger' });
  expect(await persistedSnapshot(agent, created.documentId)).toEqual(published);
  expect(createRequests).toBe(1);
  expect(apiOrigins).toEqual(new Set([human]));
  completed('gui-initial-published-shared');
  await saveSnapshot(context, 'gui-initial', created.documentId);
  completed('gui-initial-snapshot-saved');
});

// Same owned runtime, real multipart API and persistent restart oracle as the legacy test above.
test('初回原本2件をGUIの単一要求で登録し、全bytes・正式公開・再起動用snapshotを確認する', async ({ page, request }) => {
  const context = await runtime(), common = options(context.human);
  const bridge = new BinaryTransportBridge({ baseUrl: context.human });
  const title = '【合成データ】GUI初回複数原本登録';
  const originals = [
    { name: 'initial-A.txt', mimeType: 'text/plain', buffer: Buffer.from('【合成データ】初回複数原本A\n') },
    { name: 'initial-B.txt', mimeType: 'text/plain', buffer: Buffer.from('【合成データ】初回複数原本B\n') },
  ];
  let createRequests = 0;
  page.on('request', req => { if (new URL(req.url()).pathname === '/v1/documents' && req.method() === 'POST') createRequests++; });
  await page.goto(`/documents?view=published&folderId=${context.manifest.folders.shared.folderId}`);
  await page.getByRole('button', { name: '文書を登録', exact: true }).click();
  const registration = page.getByRole('dialog', { name: '文書を登録', exact: true });
  await registration.getByLabel('文書名', { exact: true }).fill(title);
  await registration.getByLabel('原本ファイル', { exact: true }).setInputFiles(originals);
  await registration.getByLabel('原本パス 1', { exact: true }).fill('chapter/A.txt');
  await registration.getByLabel('原本パス 2', { exact: true }).fill('appendix/B.txt');
  await registration.getByRole('button', { name: 'initial-B.txtを上へ', exact: true }).click();
  const createResponse = page.waitForResponse(response => new URL(response.url()).pathname === '/v1/documents' && response.request().method() === 'POST');
  await registration.getByRole('button', { name: '下書きとして登録', exact: true }).click();
  const response = await createResponse; expect(response.status()).toBe(201);
  const created = await response.json() as CreateDocumentResult;
  expect(createRequests).toBe(1);
  expect(created.fileIds).toHaveLength(2); expect(created.fileId).toBe(created.fileIds![0]);
  expect(new Set(created.fileIds).size).toBe(2);
  await expect(page).toHaveURL(url => url.pathname === `/documents/${created.documentId}` && url.searchParams.get('view') === 'authoring');
  const path = { documentId: created.documentId }, versionPath = { ...path, versionId: created.documentVersionId };
  const detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  expect(detail.currentVersionId).toBeNull(); expect(detail.displayVersion).toMatchObject({ versionNo: 1, lifecycleState: 'WORKING' });
  const draft = (await getVersionEditManifest({ ...common, path: versionPath, query: { purpose: 'authoring' } })).data;
  expect(draft.items.map(item => [item.logicalPath, item.ordinal, item.representations.length])).toEqual([['appendix/B.txt', 0, 1], ['chapter/A.txt', 1, 1]]);
  expect(draft.items.map(item => item.representations[0]!.fileId)).toEqual(created.fileIds);
  const expected = [originals[1]!, originals[0]!];
  for (const [index, item] of draft.items.entries()) {
    const representation = item.representations[0]!;
    expect(representation).toMatchObject({ originalFilename: expected[index]!.name, mediaType: 'text/plain', sizeBytes: expected[index]!.buffer.length });
    const bytes = await bridge.downloadVersionFileBlob({ ...versionPath, contentItemId: item.contentItemId, representationId: representation.representationId, purpose: 'authoring' });
    expect(Buffer.from(await bytes.arrayBuffer())).toEqual(expected[index]!.buffer);
  }
  expect((await recoverDocumentCreation({ ...common, path, query: { documentVersionId: created.documentVersionId, fileId: created.fileId, fileIds: created.fileIds!.join(',') } })).data).toEqual(created);
  const partial = await recoverDocumentCreation({ ...common, throwOnError: false, path, query: { documentVersionId: created.documentVersionId, fileId: created.fileId } });
  expect(partial.response?.status).toBe(404);
  expect((await request.get(`${context.agent}/v1/documents/${created.documentId}?view=published`)).status()).toBe(404);
  expect((await listDocumentRevisions({ ...common, path, query: { pageSize: 100 } })).data.items).toHaveLength(0);
  completed('gui-initial-working-verified');
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const publicationResponse = page.waitForResponse(result => new URL(result.url()).pathname === `/v1/documents/${created.documentId}/versions/${created.documentVersionId}:publish` && result.request().method() === 'POST');
  await page.getByRole('dialog', { name: '公開を確認', exact: true }).getByRole('button', { name: '確定する', exact: true }).click();
  expect((await publicationResponse).status()).toBe(200);
  await expect(page.getByRole('status')).toContainText('公開しました');
  const published = await persistedSnapshot(context.human, created.documentId);
  expect(published.currentVersionId).toBe(created.documentVersionId);
  expect(published.versions).toHaveLength(1); expect(published.revisions).toHaveLength(1);
  expect(published.versions[0]!.files.map(file => file.hash).sort()).toEqual(originals.map(original => hash(original.buffer)).sort());
  expect(await persistedSnapshot(context.agent, created.documentId)).toEqual(published);
  expect(createRequests).toBe(1);
  await saveSnapshot(context, 'gui-initial-multiple', created.documentId);
  completed('gui-initial-snapshot-saved');
});
