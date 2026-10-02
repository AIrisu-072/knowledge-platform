import { startDiagnostics, finishDiagnostics, captureUiDiagnostics } from './startup-diagnostics';
import { test, expect } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import {
  BinaryTransportBridge, compareDocumentRevisions, compareDocumentVersions, getDocument,
  getDocumentAccessPolicy, getDocumentHistory, getRootFolder, getSession, listDocumentRevisions,
  listDocuments, listDocumentVersions, listFolderChildren, listVersionFiles, publishVersion,
  type VersionMutationResult, type CommandsMetadataPatch, type ModelsVersion, type ModelsDisplayFragment,
} from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, saveSnapshot, uuidV7 } from './support';

test.describe.configure({ mode: 'serial' });
test.beforeEach(async ({ page }) => { await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });

test('real same-origin GUI folder → list → detail → revisions/history/diff/files → policy → version → publish shares state with agent', async ({ page, request }) => {
  const context = await runtime();
  completed('context-read');
  const { human, agent, manifest } = context;
  const documentId = manifest.documents.regulation!.create!.result!.documentId;
  const deniedId = manifest.documents.humanOnly!.create!.result!.documentId;
  const humanOptions = options(human), agentOptions = options(agent);
  const humanSession = (await getSession(humanOptions)).data;
  const agentSession = (await getSession(agentOptions)).data;
  expect(humanSession.principal.principalId).toBe('poc-human'); expect(humanSession.invocationKind).toBe('human_interactive');
  expect(agentSession.principal.principalId).toBe('poc-agent'); expect(agentSession.invocationKind).toBe('agent');
  completed('sessions-verified');
  const forged = await request.get(`${agent}/v1/session?principal=poc-human&group=poc-users`, { headers: { 'x-identity-profile': 'poc-human', 'x-principal-id': 'poc-human', cookie: 'principal=poc-human' } });
  expect(forged.status()).toBe(200); expect((await forged.json()).principal.principalId).toBe('poc-agent');
  expect((await getRootFolder(agentOptions)).data.folderId).toBe(manifest.rootFolderId);
  const folders = (await listFolderChildren({ ...agentOptions, path: { folderId: manifest.rootFolderId! }, query: { pageSize: 100 } })).data;
  expect(folders.items.map(folder => folder.folderId)).toContain(manifest.folders.shared.folderId);
  expect(folders.items.map(folder => folder.folderId)).not.toContain(manifest.folders.humanOnly.folderId);
  const humanRoot = (await getRootFolder(humanOptions)).data;
  expect(humanRoot.folderId).toBe(manifest.rootFolderId);
  const humanFolders = (await listFolderChildren({ ...humanOptions, path: { folderId: humanRoot.folderId }, query: { pageSize: 200 } })).data;
  expect(humanFolders.items).toEqual(expect.arrayContaining([expect.objectContaining({ folderId: manifest.folders.shared.folderId, name: 'PoC Shared' })]));
  const agentList = (await listDocuments({ ...agentOptions, query: { view: 'published', folderId: manifest.folders.shared.folderId, pageSize: 100 } })).data;
  expect(agentList.items.map(document => document.documentId)).toContain(documentId);
  expect((await getDocument({ ...humanOptions, path: { documentId: deniedId }, query: { view: 'published' } })).data.documentId).toBe(deniedId);
  const denied = await request.get(`${agent}/v1/documents/${deniedId}?view=published`);
  expect(denied.status()).toBe(404); expect((await denied.json()).code).toBe('DOCUMENT_NOT_FOUND');
  expect((await request.get(`${agent}/`)).status()).toBe(404);
  completed('api-preflight-complete');

  const apiOrigins = new Set<string>();
  page.on('request', req => { const url = new URL(req.url()); if (url.pathname.startsWith('/v1/')) apiOrigins.add(url.origin); });
  await page.goto('/documents?view=published');
  await expect(page.getByRole('region', { name: 'フォルダー' })).toBeVisible();
  completed('gui-loaded');
  await captureUiDiagnostics(page, 'before-folder-wait');
  const sharedFolder = page.getByRole('button', { name: 'PoC Shared', exact: true });
  await expect(sharedFolder).toBeVisible();
  await captureUiDiagnostics(page, 'before-folder-click');
  await sharedFolder.click({ timeout: 15_000 });
  await captureUiDiagnostics(page, 'after-folder-click');
  await expect(page).toHaveURL(new RegExp(`folderId=${manifest.folders.shared.folderId}`));
  await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
  completed('folder-selected');
  const row = page.getByRole('button', { name: /規程サンプル/ });
  await row.focus(); await page.keyboard.press('Enter');
  await expect(page.getByRole('complementary', { name: '選択中の文書' })).toContainText('規程サンプル');
  completed('document-selected');
  await page.getByRole('button', { name: '詳細を開く' }).press('Enter');
  await expect(page.getByRole('heading', { name: '規程サンプル', level: 1 })).toBeVisible();
  completed('detail-opened');
  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('complementary', { name: '原本と版' }).getByRole('button', { name: '現行ファイルを取得' }).click();
  completed('download-requested');
  const download = await downloadPromise;
  completed('download-received');
  const downloaded = await readFile((await download.path())!);
  completed('download-saved');
  const before = await persistedSnapshot(human, documentId);
  expect(hash(downloaded)).toBe(before.versions.find(version => version.versionId === before.currentVersionId)!.files[0]!.hash);
  completed('snapshot-read');
  await page.getByRole('tab', { name: '概要', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: '版・改訂' })).toBeFocused();
  await expect(page.getByRole('heading', { name: '正式改訂', exact: true })).toBeVisible();
  expect(before.revisions).toHaveLength(2);
  await page.getByRole('tab', { name: '履歴', exact: true }).click();
  await expect(page.getByRole('heading', { name: '変更履歴' })).toBeVisible();
  await expect(page.getByText('document.version.published', { exact: true }).first()).toBeVisible();
  completed('history-opened');
  await page.getByRole('tab', { name: '新旧比較', exact: true }).click();
  await expect(page.getByRole('heading', { name: '本文の変更' })).toBeVisible();
  const compareBody = { baseRevisionId: before.revisions[1]!.revisionId, targetRevisionId: before.revisions[0]!.revisionId, projection: 'display' as const };
  const humanComparison = (await compareDocumentRevisions({ ...humanOptions, path: { documentId }, body: compareBody })).data;
  const agentComparison = (await compareDocumentRevisions({ ...agentOptions, path: { documentId }, body: compareBody })).data;
  expect(humanComparison.coverage).toBe('full'); expect(humanComparison.displayItems.length).toBeGreaterThan(0);
  expect(agentComparison.resultDigest).toBe(humanComparison.resultDigest);
  completed('comparison-verified');
  await page.goto(`/documents/${documentId}?view=authoring&tab=access`);
  await expect(page.getByRole('heading', { name: '現在有効なアクセス権' })).toBeVisible();
  await expect(page.getByRole('rowheader', { name: /poc-agents/ })).toBeVisible();
  await page.getByRole('radio', { name: 'この文書だけに個別設定' }).check();
  await page.getByLabel('変更理由').fill('Synthetic runtime acceptance: preserve read-only agent grant');
  const policyResponse = page.waitForResponse(response => response.url().endsWith(`/documents/${documentId}/access-policy`) && response.request().method() === 'PUT');
  await page.getByRole('button', { name: 'アクセス設定を保存', exact: true }).click();
  expect((await policyResponse).status()).toBe(200);
  await expect(page.getByRole('status')).toContainText('アクセス設定を保存しました');
  const policy = (await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data;
  expect(policy.bindingMode).toBe('explicit');
  expect(policy.effectiveGrants.find(grant => grant.subjectId === 'poc-agents')!.actions.sort()).toEqual(['read', 'readHistory']);
  completed('policy-saved');

  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await expect(page.getByRole('heading', { name: '新しい版を作成', level: 1 })).toBeVisible();
  completed('version-form-opened');
  const changedContent = Buffer.from('【合成データ】規程サンプル\n第1条 実Runtime GUIで作成した第三版です。\n');
  await page.getByLabel('原本ファイル').setInputFiles({ name: 'regulation-runtime.txt', mimeType: 'text/plain', buffer: changedContent });
  const createResponse = page.waitForResponse(response => response.url().endsWith(`/documents/${documentId}/versions`) && response.request().method() === 'POST');
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).click();
  const createdResponse = await createResponse; expect(createdResponse.status()).toBe(201);
  const created = await createdResponse.json() as VersionMutationResult;
  await expect(page.getByRole('status')).toContainText('新しい版を作成しました');
  completed('version-created');
  await page.getByRole('button', { name: '版の一覧へ戻る', exact: true }).click();
  await page.getByRole('button', { name: /WORKING · 版 3/ }).click();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const publishButton = page.getByRole('button', { name: '公開する', exact: true });
  await expect(publishButton).toBeDisabled();
  completed('publication-form-opened');
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await publishButton.click();
  const dialog = page.getByRole('dialog', { name: '公開を確認' });
  await dialog.getByRole('button', { name: 'キャンセル' }).focus(); await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: '確定する' })).toBeFocused();
  const publishResponse = page.waitForResponse(response => response.url().endsWith(':publish') && response.request().method() === 'POST');
  await page.keyboard.press('Enter'); expect((await publishResponse).status()).toBe(200);
  completed('publication-response-accepted');
  await expect(page.getByRole('status')).toContainText('公開しました');
  completed('publication-success-visible');
  const returnToVersions = page.getByRole('button', { name: '版の一覧へ戻る', exact: true });
  await expect(page.locator('section[aria-busy]').filter({ has: publishButton })).toHaveAttribute('aria-busy', 'false');
  await expect(await publishButton.isEnabled() ? publishButton : returnToVersions).toBeFocused();
  completed('publication-confirmed');
  const after = await persistedSnapshot(human, documentId);
  expect(after.currentVersionId).toBe(created.targetVersionId); expect(after.revisions).toHaveLength(3);
  expect(after.versions.find(version => version.versionId === after.currentVersionId)!.files[0]!.hash).toBe(hash(changedContent));
  expect(after.publications).toHaveLength(3);
  expect(after.publications.every(item => item.actor === 'poc-human' && item.provenanceQuality === 'operationLedger')).toBe(true);
  expect(await persistedSnapshot(agent, documentId)).toEqual(after);
  const agentAuthoring = await request.get(`${agent}/v1/documents/${documentId}?view=authoring`);
  expect(agentAuthoring.status()).toBe(404); expect((await agentAuthoring.json()).code).toBe('DOCUMENT_NOT_FOUND');
  // The existing publication HTTP contract hides mutation snapshots from read-only actors.
  // Published/history visibility above does not grant authoring or publication access.
  const agentWrite = await request.post(`${agent}/v1/documents/${documentId}/versions/${after.currentVersionId}:publish`, { data: { operationId: uuidV7(), expectedRevision: after.revision } });
  expect(agentWrite.status()).toBe(404); expect((await agentWrite.json()).code).toBe('DOCUMENT_VERSION_NOT_FOUND');
  expect(await persistedSnapshot(human, documentId)).toEqual(after);
  expect(await persistedSnapshot(agent, documentId)).toEqual(after);
  expect(apiOrigins).toEqual(new Set([human]));
  completed('state-verified');
  await saveSnapshot(context, 'regulation', documentId);
  completed('snapshot-saved');
  await test.info().attach('shared-state.json', { body: Buffer.from(JSON.stringify(after, null, 2)), contentType: 'application/json' });
});

test('real PDF editorial inspection reaches the unchanged publication quality gate', async ({ request }) => {
  const context = await runtime();
  const common = options(context.human);
  const bytes = await readFile(fileURLToPath(new URL('../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf', import.meta.url)));
  const created = await new BinaryTransportBridge({ baseUrl: context.human }).createDocument({
    request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic annotated PDF rejection', documentMetadata: {}, versionMetadata: {} },
    file: new Blob([bytes]), originalFilename: 'synthetic-annotated.pdf', mediaType: 'application/pdf',
  });
  const path = { documentId: created.documentId };
  const before = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  const rejected = await request.post(`${context.human}/v1/documents/${created.documentId}/versions/${created.documentVersionId}:publish`,
    { data: { operationId: uuidV7(), expectedRevision: before.revision } });
  expect(rejected.status()).toBe(422);
  expect((await rejected.json()).code).toBe('PUBLISH_QUALITY_REJECTED');
  const after = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  expect(after.revision).toBe(before.revision);
  expect(after.currentVersionId).toBeNull();
  const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'authoring', pageSize: 100 } })).data;
  expect(versions.items).toHaveLength(1);
  expect(versions.items[0]!.lifecycleState).toBe('working' satisfies ModelsVersion['lifecycleState']);
  const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  expect(history.items.filter(item => item.actionCode === 'document.version.published')).toHaveLength(0);
});

test('real PDFium inspection and production PDF Diff display preserve both original files', async ({ page }) => {
  const context = await runtime();
  completed('pdf-context-read');
  const common = options(context.human), bridge = new BinaryTransportBridge({ baseUrl: context.human });
  const fixture = (name: string) => fileURLToPath(new URL(`./fixtures/pdf/${name}`, import.meta.url));
  const base = await readFile(fixture('base.pdf')), target = await readFile(fixture('text-change.pdf'));
  completed('pdf-fixtures-read');
  const created = await bridge.createDocument({ request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic PDF runtime acceptance', documentMetadata: {}, versionMetadata: {} },
    file: new Blob([base]), originalFilename: 'synthetic-base.pdf', mediaType: 'application/pdf' });
  completed('pdf-base-created');
  const documentId = created.documentId;
  let detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  completed('pdf-base-detail-read');
  await publishVersion({ ...common, path: { documentId, versionId: created.documentVersionId }, body: { operationId: uuidV7(), expectedRevision: detail.revision } });
  completed('pdf-base-published');
  detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  completed('pdf-published-detail-read');
  const versionId = uuidV7();
  await bridge.createVersion(documentId, { request: { operationId: uuidV7(), targetVersionId: versionId, expectedRevision: detail.revision, title: detail.title,
    items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'application/pdf', originalFilename: 'synthetic-change.pdf' }] }, files: new Map([['primary', new Blob([target], { type: 'application/pdf' })]]) });
  completed('pdf-target-created');
  detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  completed('pdf-target-detail-read');
  await publishVersion({ ...common, path: { documentId, versionId }, body: { operationId: uuidV7(), expectedRevision: detail.revision } });
  completed('pdf-target-published');
  const comparison = (await compareDocumentVersions({ ...common, path: { documentId }, body: { baseVersionId: created.documentVersionId, targetVersionId: versionId, profile: 'document-diff-v0', projection: 'display' } })).data;
  completed('pdf-comparison-read');
  expect(comparison.projection).toBe('display'); expect(comparison.coverage).toBe('full');
  if (comparison.projection !== 'display') throw Error('PDF display projection required');
  expect(comparison.items.some(item => item.facet === 'pdf_text')).toBe(true);
  const pdfText = comparison.items.find(item => item.facet === 'pdf_text');
  expect(pdfText?.base?.kind).toBe('text' satisfies ModelsDisplayFragment['kind']);
  expect(pdfText?.target?.kind).toBe('text' satisfies ModelsDisplayFragment['kind']);
  if (pdfText?.base?.kind !== 'text' || pdfText.target?.kind !== 'text') throw Error('Both authoritative PDF page fragments are required');
  expect(pdfText.base.text.trim()).toBe('Page A');
  expect(pdfText.target.text.trim()).toBe('Page X');
  for (const [id, bytes] of [[created.documentVersionId, base], [versionId, target]] as const) {
    const files = (await listVersionFiles({ ...common, path: { documentId, versionId: id }, query: { purpose: 'history' } })).data;
    completed(id === created.documentVersionId ? 'pdf-base-files-read' : 'pdf-target-files-read');
    const file = files.items[0]!; expect(file.mediaType).toBe('application/pdf');
    const downloaded = await bridge.downloadVersionFileBlob({ documentId, versionId: id, contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'history' });
    expect(hash(new Uint8Array(await downloaded.arrayBuffer()))).toBe(hash(bytes));
    completed(id === created.documentVersionId ? 'pdf-base-download-verified' : 'pdf-target-download-verified');
  }
  await page.goto(`/documents/${documentId}?view=published&tab=compare`);
  await expect(page.getByRole('heading', { name: '本文の変更' })).toBeVisible();
  await expect(page.getByRole('heading', { name: /pdf_text/ }).first()).toBeVisible();
  completed('pdf-gui-verified');
  expect(await persistedSnapshot(context.agent, documentId)).toEqual(await persistedSnapshot(context.human, documentId));
  completed('pdf-shared-state-verified');
  await saveSnapshot(context, 'pdf', documentId);
  completed('pdf-snapshot-saved');
  await test.info().attach('pdf-display.json', { body: Buffer.from(JSON.stringify(comparison, null, 2)), contentType: 'application/json' });
});

test('a real stale GUI version upload reports OCC conflict without duplicate creation', async ({ page }) => {
  const context = await runtime();
  const documentId = context.manifest.documents.manual!.create!.result!.documentId;
  const common = options(context.human);
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await expect(page.getByRole('heading', { name: '新しい版を作成', level: 1 })).toBeVisible();
  const bytes = Buffer.from('Synthetic concurrent version, never customer data.\n');
  await page.getByLabel('原本ファイル').setInputFiles({ name: 'stale.txt', mimeType: 'text/plain', buffer: bytes });
  const detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  await new BinaryTransportBridge({ baseUrl: context.human }).createVersion(documentId, {
    request: { operationId: uuidV7(), targetVersionId: uuidV7(), expectedRevision: detail.revision, title: detail.title,
      items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'text/plain', originalFilename: 'concurrent.txt' }] },
    files: new Map([['primary', new Blob([bytes], { type: 'text/plain' })]]),
  });
  const responsePromise = page.waitForResponse(response => response.url().endsWith(`/documents/${documentId}/versions`) && response.request().method() === 'POST');
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(409); expect((await response.json()).code).toBe('REVISION_CONFLICT');
  await expect(page.getByRole('alert')).toContainText('文書の状態が更新されています');
  await expect(page.getByRole('status').filter({ hasText: '新しい版を作成しました' })).toHaveCount(0);
  const versions = (await listDocumentVersions({ ...common, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
  expect(versions.items).toHaveLength(2);
});

test('real backend not-found and invalid API routes, keyboard return, reduced-motion and layout', async ({ page, request }) => {
  const context = await runtime();
  const documentId = context.manifest.documents.regulation!.create!.result!.documentId;
  await page.emulateMedia({ reducedMotion: 'reduce' });
  for (const width of [1280, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/documents?view=published&folderId=${context.manifest.folders.shared.folderId}`);
    await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--motion-spatial').trim())).toBe('0ms');
  }
  await page.getByRole('button', { name: /規程サンプル/ }).press('Enter');
  await page.getByRole('button', { name: '詳細を開く' }).press('Enter');
  await expect(page).toHaveURL(new RegExp(documentId));
  await page.getByRole('button', { name: /一覧へ戻る/ }).press('Enter');
  await expect(page.getByRole('button', { name: /規程サンプル/ })).toBeFocused();
  await page.goto(`/documents/${uuidV7()}?view=published`);
  await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません。');
  for (const path of ['/v1/not-a-real-route', '/health/not-a-real-route', '/missing.js', '/.env']) {
    const response = await request.get(path);
    expect(response.status()).toBe(404); expect(response.headers()['content-type'] ?? '').not.toContain('text/html');
  }
  const history = (await getDocumentHistory({ ...options(context.human), path: { documentId }, query: { pageSize: 100 } })).data;
  expect(history.items.some(item => item.actionCode === 'document.version.published')).toBe(true);
});


test('prepare an API-created synthetic WORKING original for actual process drain observations', async () => {
  const context = await runtime();
  const bytes = Buffer.alloc(32 * 1024 * 1024, 's');
  Buffer.from('Synthetic runtime drain fixture. No customer data.\n').copy(bytes);
  const created = await new BinaryTransportBridge({ baseUrl: context.human }).createDocument({
    request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic working drain fixture', documentMetadata: {}, versionMetadata: {} },
    file: new Blob([bytes]), originalFilename: 'synthetic-drain.txt', mediaType: 'text/plain',
  });
  const path = { documentId: created.documentId, versionId: created.documentVersionId };
  const files = (await listVersionFiles({ ...options(context.human), path, query: { purpose: 'authoring' } })).data;
  const file = files.items[0]!;
  expect(file.sizeBytes).toBe(bytes.length);
  const detail = (await getDocument({ ...options(context.human), path: { documentId: created.documentId }, query: { view: 'authoring' } })).data;
  const mutation: CommandsMetadataPatch = { operationId: uuidV7(), expectedDocumentRevision: detail.revision,
    set: { extensions: { runtimeDrainObservation: 'synthetic-in-flight-completed' } }, unset: [], reason: 'Synthetic actual-process in-flight drain acceptance' };
  await writeFile(context.drainFixturePath, JSON.stringify({ ...path, contentItemId: file.contentItemId, representationId: file.representationId,
    sizeBytes: bytes.length, sha256: hash(bytes), mutation }, null, 2), { mode: 0o600 });
});
