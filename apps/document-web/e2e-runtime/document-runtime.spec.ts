import { assertApplicationJapaneseFonts } from './japanese-font';
import { visualCheckpoint } from './visual-capture';
import { startDiagnostics, finishDiagnostics, captureUiDiagnostics } from './startup-diagnostics';
import { test, expect, type Request } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import {
  BinaryTransportBridge, compareDocumentRevisions, compareDocumentVersions, getDocument,
  getDocumentAccessPolicy, getDocumentHistory, getDocumentVersion, getFolderAccessPolicy, getRootFolder, getSession, listDocumentRevisions,
  listDocuments, listDocumentVersions, listFolderChildren, listVersionFiles, publishVersion,
  type VersionMutationResult, type CommandsMetadataPatch, type ModelsVersion, type ModelsDisplayFragment, type ModelsHistory, type RevisionComparisonResponse,
  type VersionList, type VersionDetail, type FileList, type ModelsDiffDisplayProjection,
  type PolicyGrantInput, type CommandsPolicyExplicit, type CommandsPolicyInherit, type MutationResult,
} from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, saveSnapshot, uuidV7 } from './support';
import { formatDateTime } from '../src/view-model/date-time';

test.describe.configure({ mode: 'serial' });
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
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
  await expect(page.getByRole('button', { name: '選択したフォルダーのアクセス設定', exact: true })).toBeHidden();
  completed('gui-loaded');
  await assertApplicationJapaneseFonts(page);
  test.info().annotations.push({ type: 'runtime-font', description: 'kosugi-regular-japanese-heading-body' });
  await captureUiDiagnostics(page, 'before-folder-wait');
  const sharedFolder = page.getByRole('button', { name: 'PoC Shared', exact: true });
  await expect(sharedFolder).toBeVisible();
  await captureUiDiagnostics(page, 'before-folder-click');
  await sharedFolder.click({ timeout: 15_000 });
  await captureUiDiagnostics(page, 'after-folder-click');
  await expect(page).toHaveURL(new RegExp(`folderId=${manifest.folders.shared.folderId}`));
  await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
  completed('folder-selected');
  // 既存の合成Sharedだけを変更する。humanの全5権限とagentのreadは全遷移で保持する。
  const folderPath = { folderId: manifest.folders.shared.folderId };
  const grantsOnly = (grants: readonly PolicyGrantInput[]): PolicyGrantInput[] => grants
    .map(({ subjectKind, identityProvider, subjectId, actions }) => ({ subjectKind, identityProvider, subjectId, actions: [...actions].sort() }))
    .sort((left, right) => JSON.stringify([left.subjectKind, left.identityProvider, left.subjectId])
      .localeCompare(JSON.stringify([right.subjectKind, right.identityProvider, right.subjectId])));
  const originalFolderPolicy = (await getFolderAccessPolicy({ ...humanOptions, path: folderPath })).data;
  const rootPolicy = (await getFolderAccessPolicy({ ...humanOptions, path: { folderId: humanRoot.folderId } })).data;
  const humanOnlyPolicy = (await getFolderAccessPolicy({ ...humanOptions, path: { folderId: manifest.folders.humanOnly.folderId } })).data;
  const originalDocumentPolicy = (await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data;
  const originalDocument = (await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data;
  const originalGrants = grantsOnly(originalFolderPolicy.effectiveGrants);
  expect(originalFolderPolicy).toMatchObject({ target: { kind: 'folder', id: folderPath.folderId }, bindingMode: 'explicit',
    policyRevision: manifest.folders.shared.policy!.result!.resultingRevision, effectiveSource: { kind: 'folder', id: folderPath.folderId } });
  expect(originalGrants).toEqual(grantsOnly(manifest.folders.shared.policy!.request.grants));
  expect(originalGrants).toHaveLength(2);
  expect(grantsOnly(rootPolicy.effectiveGrants)).toEqual(originalGrants);
  expect(originalDocumentPolicy).toMatchObject({ bindingMode: 'inherit', effectivePolicyId: originalFolderPolicy.policyId,
    effectiveSource: { kind: 'folder', id: folderPath.folderId } });
  let currentFolderPolicy = originalFolderPolicy;
  const folderOperations = new Set<string>();
  const folderPolicyRequests: Request[] = [];
  const captureFolderPolicy = (req: Request) => {
    const url = new URL(req.url());
    if (url.origin === human && /^\/v1\/folders\/[^/]+\/access-policy$/.test(url.pathname) && req.method() === 'PUT') folderPolicyRequests.push(req);
  };
  page.on('request', captureFolderPolicy);
  for (const change of [
    { mode: 'explicit', history: false, reason: 'Synthetic Shared agent history permission off' },
    { mode: 'explicit', history: true, reason: 'Restore synthetic Shared agent history permission' },
    { mode: 'inherit', history: true, reason: 'Synthetic Shared inherits verified root policy' },
    { mode: 'explicit', history: true, reason: 'Restore synthetic Shared explicit policy from verified inherited grants' },
  ] as const) {
    // 保存成功で選択根拠を捨てるため、毎回通常ツリーから同じ行を明示選択し直す。
    await sharedFolder.click();
    const policyRead = page.waitForResponse(response => {
      const url = new URL(response.url());
      return url.origin === human && url.pathname === `/v1/folders/${folderPath.folderId}/access-policy` && response.request().method() === 'GET';
    });
    await page.getByRole('button', { name: '選択したフォルダーのアクセス設定', exact: true }).click();
    const policyReadResult = await policyRead;
    expect(policyReadResult.status()).toBe(200);
    expect(await policyReadResult.json()).toEqual(currentFolderPolicy);
    const policyDialog = page.getByRole('dialog', { name: '選択したフォルダーのアクセス設定', exact: true });
    await expect(policyDialog).toBeVisible();
    await policyDialog.getByRole('combobox', { name: '設定方式', exact: true }).selectOption(change.mode);
    const humanGrant = policyDialog.getByRole('group', { name: 'group / poc / poc-users', exact: true });
    const agentGrant = policyDialog.getByRole('group', { name: 'group / poc / poc-agents', exact: true });
    if (change.mode === 'explicit') await agentGrant.getByRole('checkbox', { name: '履歴閲覧', exact: true }).setChecked(change.history);
    for (const label of ['閲覧', '履歴閲覧', '編集', '公開', 'アクセス管理']) {
      await expect(humanGrant.getByRole('checkbox', { name: label, exact: true })).toBeChecked();
    }
    await expect(agentGrant.getByRole('checkbox', { name: '閲覧', exact: true })).toBeChecked();
    for (const label of ['編集', '公開', 'アクセス管理']) await expect(agentGrant.getByRole('checkbox', { name: label, exact: true })).not.toBeChecked();
    await policyDialog.getByRole('textbox', { name: 'アクセス設定の変更理由', exact: true }).fill(change.reason);
    await expect(policyDialog.getByRole('heading', { name: '変更内容の確認', exact: true })).toBeVisible();
    await policyDialog.getByRole('checkbox', { name: '変更内容と影響範囲を確認しました', exact: true }).check();
    const policyWrite = page.waitForResponse(response => {
      const url = new URL(response.url());
      return url.origin === human && url.pathname === `/v1/folders/${folderPath.folderId}/access-policy` && response.request().method() === 'PUT';
    });
    await policyDialog.getByRole('button', { name: 'アクセス設定を保存', exact: true }).click();
    const policyWriteResult = await policyWrite;
    expect(policyWriteResult.status()).toBe(200);
    const payload = policyWriteResult.request().postDataJSON() as CommandsPolicyExplicit | CommandsPolicyInherit;
    expect(payload.operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    expect(folderOperations.has(payload.operationId)).toBe(false); folderOperations.add(payload.operationId);
    const expectedGrants = originalGrants.map(grant => ({ ...grant, actions: grant.subjectId === 'poc-agents' && !change.history
      ? grant.actions.filter(action => action !== 'readHistory') : [...grant.actions] }));
    const expectedPayload = { operationId: payload.operationId, expectedPolicyRevision: currentFolderPolicy.policyRevision, mode: change.mode, reason: change.reason };
    if (payload.mode === 'explicit') expect({ ...payload, grants: grantsOnly(payload.grants) }).toEqual({ ...expectedPayload, grants: expectedGrants });
    else expect(payload).toEqual(expectedPayload);
    const receipt = await policyWriteResult.json() as MutationResult;
    expect(receipt).toMatchObject({ operationId: payload.operationId, resourceId: folderPath.folderId,
      changed: true, resultingRevision: currentFolderPolicy.policyRevision + 1 });
    await expect(policyDialog.getByRole('status').filter({ hasText: 'アクセス設定を保存しました。' })).toBeVisible();
    currentFolderPolicy = (await getFolderAccessPolicy({ ...humanOptions, path: folderPath })).data;
    expect(currentFolderPolicy).toMatchObject({ target: originalFolderPolicy.target, bindingMode: change.mode,
      policyId: originalFolderPolicy.policyId, policyRevision: receipt.resultingRevision,
      effectivePolicyId: change.mode === 'inherit' ? rootPolicy.effectivePolicyId : originalFolderPolicy.policyId,
      effectiveSource: change.mode === 'inherit' ? rootPolicy.effectiveSource : originalFolderPolicy.target });
    expect(grantsOnly(currentFolderPolicy.effectiveGrants)).toEqual(expectedGrants);
    const inheritedDocumentPolicy = (await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data;
    expect(inheritedDocumentPolicy).toEqual({ ...originalDocumentPolicy, effectivePolicyId: currentFolderPolicy.effectivePolicyId,
      effectiveSource: currentFolderPolicy.effectiveSource, effectiveGrants: currentFolderPolicy.effectiveGrants });
    expect((await getDocument({ ...agentOptions, path: { documentId }, query: { view: 'published' } })).data.documentId).toBe(documentId);
    const agentHistory = await request.get(`${agent}/v1/documents/${documentId}/versions?purpose=history&pageSize=100`);
    expect(agentHistory.status()).toBe(change.history ? 200 : 403);
    if (change.history) expect((await agentHistory.json()).items).toHaveLength(2);
    else {
      expect((await agentHistory.json()).code).toBe('FORBIDDEN');
      // 既知成功receiptの完全再送だけを確認する。応答喪失/自己失権後UNKNOWNの実資格ではない。
      const replay = await request.put(policyWriteResult.url(), { data: payload });
      expect(replay.status()).toBe(200); expect(await replay.json()).toEqual(receipt);
      expect((await getFolderAccessPolicy({ ...humanOptions, path: folderPath })).data).toEqual(currentFolderPolicy);
    }
    await policyDialog.getByRole('button', { name: '確認して閉じる', exact: true }).click();
    await expect(policyDialog).toBeHidden();
  }
  page.off('request', captureFolderPolicy);
  expect(folderPolicyRequests).toHaveLength(4);
  expect(folderPolicyRequests.map(req => new URL(req.url()).pathname)).toEqual(Array(4).fill(`/v1/folders/${folderPath.folderId}/access-policy`));
  expect(currentFolderPolicy.policyRevision).toBe(originalFolderPolicy.policyRevision + 4);
  expect(grantsOnly(currentFolderPolicy.effectiveGrants)).toEqual(originalGrants);
  expect((await getFolderAccessPolicy({ ...humanOptions, path: { folderId: humanRoot.folderId } })).data).toEqual(rootPolicy);
  expect((await getFolderAccessPolicy({ ...humanOptions, path: { folderId: manifest.folders.humanOnly.folderId } })).data).toEqual(humanOnlyPolicy);
  expect((await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data).toEqual(originalDocumentPolicy);
  expect((await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data).toEqual(originalDocument);
  await sharedFolder.click();
  await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
  const row = page.getByRole('button', { name: /規程サンプル/ });
  await row.focus(); await page.keyboard.press('Enter');
  await expect(page.getByRole('complementary', { name: '選択中の文書' })).toContainText('規程サンプル');
  await visualCheckpoint(page, '01-list-context-1440.png');
  completed('document-selected');
  await page.getByRole('button', { name: '詳細を開く' }).press('Enter');
  await expect(page.getByRole('heading', { name: '規程サンプル', level: 1 })).toBeVisible();
  await visualCheckpoint(page, '03-detail-overview-1440.png');
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
  await visualCheckpoint(page, '04-revision-version-1440.png');
  const oldVersion = before.versions.find(version => version.versionNo === 1)!;
  expect(oldVersion.versionId).not.toBe(before.currentVersionId);
  const currentVersion = before.versions.find(version => version.versionId === before.currentVersionId)!;
  const normalSelection = page.getByRole('heading', { name: `選択中: 版 ${currentVersion.versionNo}`, exact: true });
  await expect(normalSelection).toBeVisible();
  const normalUrl = page.url();
  const readStateBeforeHistory = (await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data.readState;
  const versionsBeforeHistory = (await listDocumentVersions({ ...humanOptions, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
  const versionReadStatesBeforeHistory = versionsBeforeHistory.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt }));
  const historyResponse = (suffix: string) => page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === human && url.pathname === `/v1/documents/${documentId}/versions${suffix}`
      && url.searchParams.get('purpose') === 'history' && response.request().method() === 'GET';
  });
  const historyListResponse = historyResponse('');
  await page.getByRole('button', { name: 'コンテンツ版の履歴を開く', exact: true }).click();
  const historyListResult = await historyListResponse;
  expect(historyListResult.status()).toBe(200);
  expect(new URL(historyListResult.url()).searchParams.get('pageSize')).toBe('100');
  expect(new URL(historyListResult.url()).searchParams.has('cursor')).toBe(false);
  const historyVersions = await historyListResult.json() as VersionList;
  expect(historyVersions.items.map(version => version.versionId)).toEqual(before.versions.map(version => version.versionId));
  expect(historyVersions.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt }))).toEqual(versionReadStatesBeforeHistory);
  // 既存の2版だけを使う。実GUIのコンテンツ版101件目の資格には読み替えない。
  expect(historyVersions.nextCursor).toBeNull();
  const contentHistory = page.getByRole('region', { name: 'コンテンツ版の履歴（閲覧専用）', exact: true });
  await expect(contentHistory).toBeVisible();
  const historySelection = contentHistory.getByRole('combobox', { name: '履歴のコンテンツ版を選択', exact: true });
  await expect(historySelection).toHaveValue('');
  const oldDetailResponse = historyResponse(`/${oldVersion.versionId}`);
  const oldFilesResponse = historyResponse(`/${oldVersion.versionId}/files`);
  await historySelection.selectOption(oldVersion.versionId);
  const oldDetailResult = await oldDetailResponse, oldFilesResult = await oldFilesResponse;
  expect(oldDetailResult.status()).toBe(200); expect(oldFilesResult.status()).toBe(200);
  const oldDetail = await oldDetailResult.json() as VersionDetail;
  expect(oldDetail).toMatchObject({ versionId: oldVersion.versionId, versionNo: 1, lifecycleState: 'published', isCurrent: false,
    firstReadAt: historyVersions.items.find(version => version.versionId === oldVersion.versionId)!.firstReadAt });
  expect(oldDetail.capabilities.download.status).toBe('available');
  const historyDetail = contentHistory.getByRole('region', { name: '選択したコンテンツ版の詳細', exact: true });
  await expect(historyDetail).toContainText(oldDetail.title);
  await expect(historyDetail).toContainText(`Version ${oldDetail.versionNo}`);
  const oldFiles = await oldFilesResult.json() as FileList;
  const originals = oldFiles.items.filter(file => file.role === 'AUTHORITATIVE');
  expect(originals.length).toBeGreaterThan(0);
  const originalButtons = historyDetail.getByRole('button', { name: /^履歴の原本を取得: / });
  await expect(originalButtons).toHaveCount(originals.length);
  for (const [index, file] of originals.entries()) {
    const originalResponse = historyResponse(`/${oldVersion.versionId}/files/${file.contentItemId}/${file.representationId}`);
    const historicalDownload = page.waitForEvent('download');
    await expect(originalButtons.nth(index)).toHaveAccessibleName(`履歴の原本を取得: ${file.displayName}`);
    await originalButtons.nth(index).click();
    expect((await originalResponse).status()).toBe(200);
    const originalDownload = await historicalDownload;
    expect(originalDownload.suggestedFilename()).toBe(file.displayName);
    const originalBytes = await readFile((await originalDownload.path())!);
    const originalSnapshot = oldVersion.files.find(item => item.contentItemId === file.contentItemId && item.representationId === file.representationId)!;
    expect(originalBytes.byteLength).toBe(file.sizeBytes); expect(hash(originalBytes)).toBe(originalSnapshot.hash);
  }
  await expect(historySelection).toHaveValue(oldVersion.versionId);
  await expect(normalSelection).toBeVisible();
  await expect(page).toHaveURL(normalUrl);
  await contentHistory.getByRole('button', { name: 'コンテンツ版の履歴を閉じる', exact: true }).click();
  await expect(contentHistory).toBeHidden();
  await expect(normalSelection).toBeVisible();
  await expect(page).toHaveURL(normalUrl);
  expect((await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data.readState).toEqual(readStateBeforeHistory);
  const versionsAfterHistory = (await listDocumentVersions({ ...humanOptions, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
  expect(versionsAfterHistory.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt })))
    .toEqual(versionReadStatesBeforeHistory);
  expect(await persistedSnapshot(human, documentId)).toEqual(before);
  expect(await persistedSnapshot(agent, documentId)).toEqual(before);
  const historyReadState = (await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data.readState;
  const historyRegion = page.getByRole('region', { name: '変更履歴', exact: true });
  let initialHistory: ModelsHistory | undefined;
  for (const read of ['open', 'restart'] as const) {
    const historyResponse = page.waitForResponse(response => {
      const url = new URL(response.url());
      return url.origin === human && url.pathname === `/v1/documents/${documentId}/history`
        && response.request().method() === 'GET';
    });
    if (read === 'open') await page.getByRole('tab', { name: '履歴', exact: true }).click();
    else await historyRegion.getByRole('button', { name: '変更履歴を最初から読み直す', exact: true }).click();
    const historyResult = await historyResponse;
    expect(historyResult.status()).toBe(200);
    expect([...new URL(historyResult.url()).searchParams]).toEqual([['pageSize', '100']]);
    const history = await historyResult.json() as ModelsHistory;
    expect(history.items.length).toBeGreaterThan(0);
    expect(history.items.some(entry => entry.actionCode === 'document.version.published')).toBe(true);
    // 既存seedの少数履歴だけを使う。実GUIの101件目や跨page snapshotの資格には読み替えない。
    expect(history.nextCursor).toBeNull();
    if (initialHistory) expect(isDeepStrictEqual(history, initialHistory)).toBe(true);
    else initialHistory = history;
    await expect(historyRegion.getByRole('heading', { name: '変更履歴', exact: true })).toBeVisible();
    const historyRows = historyRegion.getByRole('listitem');
    await expect(historyRows).toHaveCount(history.items.length);
    await expect(historyRows.locator('strong')).toHaveText(history.items.map(entry => entry.actionCode));
    await expect(historyRows.locator('span')).toHaveText(history.items.map(entry =>
      entry.provenanceQuality === 'operationLedger' ? '操作記録' : entry.provenanceQuality === 'versionFallback' ? '版からの履歴' : '由来不明の履歴'));
    for (const [index, entry] of history.items.entries()) {
      const renderedRow = historyRows.nth(index);
      await expect(renderedRow.locator('time')).toHaveText(entry.occurredAt ? formatDateTime(entry.occurredAt, 'Asia/Tokyo') : '日時不明');
      const actorLabel = entry.actor?.presentation.displayName ?? entry.actor?.principalId ?? '実行者不明';
      const actorStatus = entry.actor?.presentation.resolution === 'notFound' ? ' · ディレクトリに存在しません'
        : entry.actor?.presentation.resolution === 'unavailable' ? ' · 表示情報を取得できません' : '';
      await expect(renderedRow.locator('p')).toHaveText(actorLabel + actorStatus);
    }
    await expect(historyRegion.getByRole('button', { name: '変更履歴をさらに表示', exact: true })).toBeHidden();
    await expect(historyRegion.getByRole('button', { name: '変更履歴の続きを再試行', exact: true })).toBeHidden();
    await expect(historyRegion.getByRole('button', { name: '変更履歴を最初から読み直す', exact: true })).toBeEnabled();
  }
  expect(isDeepStrictEqual(await persistedSnapshot(human, documentId), before)).toBe(true);
  expect(isDeepStrictEqual((await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data.readState, historyReadState)).toBe(true);
  await page.getByRole('tab', { name: '履歴', exact: true }).click();
  await expect(page.getByRole('heading', { name: '変更履歴' })).toBeVisible();
  await expect(page.getByText('document.version.published', { exact: true }).first()).toBeVisible();
  completed('history-opened');
  const comparisonResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === human && url.pathname === `/v1/documents/${documentId}/revision-comparisons`
      && response.request().method() === 'POST';
  });
  await page.getByRole('tab', { name: '新旧比較', exact: true }).click();
  await expect(page.getByRole('heading', { name: '本文の変更' })).toBeVisible();
  const compareBody = { baseRevisionId: before.revisions[1]!.revisionId, targetRevisionId: before.revisions[0]!.revisionId, projection: 'display' as const };
  const comparisonResult = await comparisonResponse;
  expect(comparisonResult.status()).toBe(200);
  expect(comparisonResult.request().postDataJSON()).toEqual({ ...compareBody, pageSize: 50 });
  const comparisonBody = await comparisonResult.json() as RevisionComparisonResponse;
  expect(comparisonBody.contentComparisonStatus).toBe('differentAuthoritativeVersions');
  expect(comparisonBody.coverage).toBe('full');
  expect(comparisonBody.displayItems.length).toBeGreaterThan(0);
  // 既存の1行更新fixtureだけを使う。実GUIの50件超の追加page資格には読み替えない。
  expect(comparisonBody.nextCursor ?? null).toBeNull();
  const comparisonRegion = page.getByRole('region', { name: '新旧比較', exact: true });
  const bodyChanges = comparisonRegion.locator('section')
    .filter({ has: page.getByRole('heading', { name: '本文の変更', exact: true }) });
  await expect(bodyChanges).toHaveCount(1);
  await expect(bodyChanges.getByRole('listitem')).toHaveCount(comparisonBody.displayItems.length);
  await expect(comparisonRegion.getByRole('button', { name: '比較結果をさらに表示', exact: true })).toBeHidden();
  await expect(comparisonRegion.getByRole('button', { name: '比較結果を最初から読み直す', exact: true })).toBeVisible();
  const humanComparison = (await compareDocumentRevisions({ ...humanOptions, path: { documentId }, body: compareBody })).data;
  const agentComparison = (await compareDocumentRevisions({ ...agentOptions, path: { documentId }, body: compareBody })).data;
  expect(humanComparison.coverage).toBe('full'); expect(humanComparison.displayItems.length).toBeGreaterThan(0);
  expect(comparisonBody.resultDigest).toBe(humanComparison.resultDigest);
  expect(agentComparison.resultDigest).toBe(humanComparison.resultDigest);
  await visualCheckpoint(page, '05-comparison-1440.png');
  completed('comparison-verified');
  await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '編集作業', exact: true }).click();
  await expect(page).toHaveURL((url) => url.pathname === '/documents' && url.searchParams.get('view') === 'authoring');
  await page.getByRole('button', { name: 'PoC Shared', exact: true }).click();
  await page.getByRole('button', { name: /規程サンプル/ }).click();
  await page.getByRole('button', { name: '詳細を開く', exact: true }).click();
  await expect(page).toHaveURL((url) => url.pathname === `/documents/${documentId}` && url.searchParams.get('view') === 'authoring');
  const documentPolicyRequests: Request[] = [];
  const captureDocumentPolicy = (req: Request) => {
    const url = new URL(req.url());
    if (url.origin === human && /^\/v1\/documents\/[^/]+\/access-policy$/.test(url.pathname) && req.method() === 'PUT') documentPolicyRequests.push(req);
  };
  page.on('request', captureDocumentPolicy);
  const documentPolicyRead = () => page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === human && url.pathname === `/v1/documents/${documentId}/access-policy` && response.request().method() === 'GET';
  });
  const initialDocumentPolicyResponse = documentPolicyRead();
  await page.getByRole('tab', { name: 'アクセス', exact: true }).click();
  const initialDocumentPolicyResult = await initialDocumentPolicyResponse;
  expect(initialDocumentPolicyResult.status()).toBe(200);
  expect(await initialDocumentPolicyResult.json()).toEqual(originalDocumentPolicy);
  await expect(page.getByRole('heading', { name: '現在有効なアクセス権' })).toBeVisible();
  await expect(page.getByRole('rowheader', { name: /poc-agents/ })).toBeVisible();
  await page.getByRole('radio', { name: 'この文書だけに個別設定' }).check();
  await page.getByLabel('変更理由').fill('Synthetic runtime acceptance: preserve read-only agent grant');
  await expect(page.getByRole('radio', { name: 'この文書だけに個別設定' })).toBeChecked();
  await expect(page.getByLabel('変更理由')).toHaveValue('Synthetic runtime acceptance: preserve read-only agent grant');
  await visualCheckpoint(page, '10-access-policy-effective-draft-1440.png');
  const policyResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === human && url.pathname === `/v1/documents/${documentId}/access-policy` && response.request().method() === 'PUT';
  });
  await page.getByRole('button', { name: 'アクセス設定を保存', exact: true }).click();
  const documentPolicyResult = await policyResponse;
  expect(documentPolicyResult.status()).toBe(200);
  const documentPolicyBody = documentPolicyResult.request().postData()!;
  const documentPolicyPayload = documentPolicyResult.request().postDataJSON() as CommandsPolicyExplicit;
  expect(documentPolicyPayload.operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
  expect(folderOperations.has(documentPolicyPayload.operationId)).toBe(false);
  expect({ ...documentPolicyPayload, grants: grantsOnly(documentPolicyPayload.grants) }).toEqual({
    operationId: documentPolicyPayload.operationId, expectedPolicyRevision: originalDocumentPolicy.policyRevision,
    mode: 'explicit', reason: 'Synthetic runtime acceptance: preserve read-only agent grant',
    grants: originalGrants,
  });
  const documentPolicyReceipt = await documentPolicyResult.json() as MutationResult;
  expect(documentPolicyReceipt).toMatchObject({ operationId: documentPolicyPayload.operationId, resourceId: documentId,
    changed: true, resultingRevision: originalDocumentPolicy.policyRevision + 1 });
  expect(Number.isFinite(Date.parse(documentPolicyReceipt.occurredAt))).toBe(true);
  const documentPolicyOutcome = page.getByRole('region', { name: '文書アクセス設定の保存結果', exact: true });
  await expect(documentPolicyOutcome.getByRole('status').filter({ hasText: 'アクセス設定を保存しました。' })).toBeVisible();
  await expect(documentPolicyOutcome).toContainText(documentPolicyPayload.operationId);
  await expect(documentPolicyOutcome.getByRole('textbox', { name: '変更理由', exact: true })).toHaveValue(documentPolicyPayload.reason);
  await expect(documentPolicyOutcome.getByText(`送信時のpolicy revision：${documentPolicyPayload.expectedPolicyRevision}`, { exact: true })).toBeVisible();
  await expect(documentPolicyOutcome.getByText(`保存結果のpolicy revision：${documentPolicyReceipt.resultingRevision}`, { exact: true })).toBeVisible();
  const policy = (await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data;
  expect(policy.bindingMode).toBe('explicit');
  expect(policy).toMatchObject({ target: { kind: 'document', id: documentId }, policyRevision: documentPolicyReceipt.resultingRevision,
    effectivePolicyId: policy.policyId, effectiveSource: { kind: 'document', id: documentId } });
  expect(policy.policyId).not.toBeNull();
  expect(grantsOnly(policy.effectiveGrants)).toEqual(originalGrants);
  expect(grantsOnly(policy.effectiveGrants).find(grant => grant.subjectId === 'poc-users')!.actions)
    .toEqual(['administer', 'publish', 'read', 'readHistory', 'write']);
  expect(grantsOnly(policy.effectiveGrants).find(grant => grant.subjectId === 'poc-agents')!.actions).toEqual(['read', 'readHistory']);
  // 既知成功の同じbodyを完全再送する。実の応答喪失・自己失権後UNKNOWNの資格ではない。
  const documentPolicyReplay = await request.put(documentPolicyResult.url(), {
    data: documentPolicyBody, headers: { 'content-type': 'application/json' },
  });
  expect(documentPolicyReplay.status()).toBe(200);
  expect(await documentPolicyReplay.json()).toEqual(documentPolicyReceipt);
  expect((await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data).toEqual(policy);
  expect((await getFolderAccessPolicy({ ...humanOptions, path: folderPath })).data).toEqual(currentFolderPolicy);
  // 同じアプリ内の通常往復で成功receiptを保持する。確認して閉じた後は現在GETだけを根拠にする。
  await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '編集作業', exact: true }).click();
  await expect(page).toHaveURL((url) => url.pathname === '/documents' && url.searchParams.get('view') === 'authoring');
  await expect(page.getByRole('button', { name: '文書アクセス設定の保存結果', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'PoC Shared', exact: true }).click();
  await page.getByRole('button', { name: /規程サンプル/ }).click();
  await page.getByRole('button', { name: '詳細を開く', exact: true }).click();
  await expect(page).toHaveURL((url) => url.pathname === `/documents/${documentId}` && url.searchParams.get('view') === 'authoring');
  await page.getByRole('tab', { name: 'アクセス', exact: true }).click();
  await expect(documentPolicyOutcome.getByRole('status').filter({ hasText: 'アクセス設定を保存しました。' })).toBeVisible();
  await expect(documentPolicyOutcome).toContainText(documentPolicyPayload.operationId);
  await expect(documentPolicyOutcome.getByRole('textbox', { name: '変更理由', exact: true })).toHaveValue(documentPolicyPayload.reason);
  await expect(documentPolicyOutcome.getByText(`送信時のpolicy revision：${documentPolicyPayload.expectedPolicyRevision}`, { exact: true })).toBeVisible();
  await expect(documentPolicyOutcome.getByText(`保存結果のpolicy revision：${documentPolicyReceipt.resultingRevision}`, { exact: true })).toBeVisible();
  await documentPolicyOutcome.getByRole('button', { name: '確認して閉じる', exact: true }).click();
  await expect(documentPolicyOutcome).toBeHidden();
  await expect(page.getByRole('radio', { name: 'この文書だけに個別設定', exact: true })).toBeChecked();
  expect((await getDocumentAccessPolicy({ ...humanOptions, path: { documentId } })).data).toEqual(policy);
  page.off('request', captureDocumentPolicy);
  expect(documentPolicyRequests).toHaveLength(1);
  expect(new URL(documentPolicyRequests[0]!.url()).pathname).toBe(`/v1/documents/${documentId}/access-policy`);
  expect(documentPolicyRequests[0]!.postData()).toBe(documentPolicyBody);
  completed('policy-saved');

  await page.getByRole('tab', { name: '版・改訂', exact: true }).click();
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await expect(page.getByRole('heading', { name: '新しい版を作成', level: 1 })).toBeVisible();
  completed('version-form-opened');
  // Keep a supported single-line edit against both seeded Versions; replacing
  // multiple lines together is intentionally ambiguous in document-diff-v0.
  const changedContent = Buffer.from('【合成データ】規程サンプル\n第1条 この文書はPoC検証専用です。\n第2条 実Runtime GUIで作成した第三版の更新履歴を確認します。\n');
  // The manifest editor preserves logicalPath independently of the replacement filename.
  // Keep the synthetic primary anchor so this remains a content-only edit.
  for (const version of before.versions) {
    const files = (await listVersionFiles({ ...humanOptions, path: { documentId, versionId: version.versionId }, query: { purpose: 'history' } })).data;
    expect(files.items).toHaveLength(1);
    expect(files.items[0]).toMatchObject({ logicalPath: 'primary', ordinal: 0, mediaType: 'text/plain' });
  }
  await page.getByLabel(/^差替ファイル:/).setInputFiles({ name: 'primary', mimeType: 'text/plain', buffer: changedContent });
  await expect(page.getByLabel(/^差替ファイル:/)).toHaveValue(/(?:^|[\\/])primary$/);
  await visualCheckpoint(page, '06-version-file-selected-1440.png');
  const createResponse = page.waitForResponse(response => response.url().endsWith(`/documents/${documentId}/versions`) && response.request().method() === 'POST');
  await page.getByRole('button', { name: '新しい作業版を作成', exact: true }).click();
  const createdResponse = await createResponse; expect(createdResponse.status()).toBe(201);
  const created = await createdResponse.json() as VersionMutationResult;
  await expect(page.getByRole('status')).toContainText('新しい作業版を作成しました');
  const createdFiles = (await listVersionFiles({ ...humanOptions, path: { documentId, versionId: created.targetVersionId }, query: { purpose: 'authoring' } })).data;
  expect(createdFiles.items).toHaveLength(1);
  expect(createdFiles.items[0]).toMatchObject({ logicalPath: 'primary', ordinal: 0, mediaType: 'text/plain', displayName: 'primary' });
  completed('version-created');
  await page.getByRole('button', { name: '版の一覧へ戻る', exact: true }).click();
  await page.getByRole('button', { name: /WORKING · 版 3/ }).click();
  const workingSelection = page.getByRole('heading', { name: '選択中: WORKING · 版 3', exact: true });
  await expect(workingSelection).toBeVisible();
  const workingComparisonButton = page.getByRole('button', { name: '現行公開版とこの作業版を比較', exact: true });
  await expect(workingComparisonButton).toBeEnabled();
  const workingComparisonUrl = page.url();
  const workingComparisonBaseId = before.currentVersionId;
  if (!workingComparisonBaseId) throw Error('The seeded published Version 2 must remain current before comparison');
  const readWorkingComparisonState = async () => ({
    // history-purpose includes this WORKING for the human writer, unlike the read-only agent.
    snapshot: await persistedSnapshot(human, documentId),
    document: (await getDocument({ ...humanOptions, path: { documentId }, query: { view: 'published' } })).data,
    base: (await getDocumentVersion({ ...humanOptions, path: { documentId, versionId: workingComparisonBaseId }, query: { purpose: 'published' } })).data,
    target: (await getDocumentVersion({ ...humanOptions, path: { documentId, versionId: created.targetVersionId }, query: { purpose: 'authoring' } })).data,
  });
  const beforeWorkingComparison = await readWorkingComparisonState();
  expect(beforeWorkingComparison.document.currentVersionId).toBe(before.currentVersionId);
  expect(beforeWorkingComparison.document.capabilities.compareVersions.status).toBe('available');
  expect(beforeWorkingComparison.base).toMatchObject({ versionId: before.currentVersionId, versionNo: 2, lifecycleState: 'published', isCurrent: true });
  expect(beforeWorkingComparison.target).toMatchObject({ versionId: created.targetVersionId, versionNo: 3, lifecycleState: 'working', isCurrent: false });
  expect(beforeWorkingComparison.snapshot.revision).toBe(created.resultingRevision);
  expect(beforeWorkingComparison.snapshot.versions).toHaveLength(3);
  expect(beforeWorkingComparison.snapshot.versions.find(version => version.versionId === created.targetVersionId)!.files[0]!.hash).toBe(hash(changedContent));
  const workingComparisonResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === human && url.pathname === `/v1/documents/${documentId}/comparisons`
      && response.request().method() === 'POST';
  });
  const workingComparisonRequests: string[] = [];
  const recordWorkingComparisonRequest = (req: Request) => {
    const url = new URL(req.url());
    if (url.origin === human && url.pathname.startsWith('/v1/')) workingComparisonRequests.push(`${req.method()} ${url.pathname}`);
  };
  page.on('request', recordWorkingComparisonRequest);
  await workingComparisonButton.click();
  const workingComparisonResult = await workingComparisonResponse;
  expect(workingComparisonResult.status()).toBe(200);
  expect(workingComparisonResult.request().postDataJSON()).toEqual({ baseVersionId: before.currentVersionId,
    targetVersionId: created.targetVersionId, profile: 'document-diff-v0', projection: 'display', pageSize: 50 });
  // Version display carries content items, not the formal Revision comparison's metadata snapshots or pair IDs.
  const workingComparison = await workingComparisonResult.json() as ModelsDiffDisplayProjection;
  expect(workingComparison).toMatchObject({ projection: 'display', verdict: 'different', coverage: 'full', pageSize: 50,
    nextCursor: null, unverifiedRegions: [] });
  expect(workingComparison.resultDigest).toMatch(/^[0-9a-f]{64}$/);
  expect(workingComparison.items.length).toBeGreaterThan(0);
  const baseLine = downloaded.toString('utf8').split('\n')[2]!;
  const targetLine = changedContent.toString('utf8').split('\n')[2]!;
  expect(workingComparison.items.some(item => item.base?.kind === 'text' && item.target?.kind === 'text'
    && item.base.text.includes(baseLine) && item.target.text.includes(targetLine))).toBe(true);
  const workingComparisonRegion = page.getByRole('region', { name: '公開前の内容比較', exact: true });
  await expect(workingComparisonRegion).toBeVisible();
  const workingComparisonFact = (label: string) => workingComparisonRegion.locator('dt').filter({ hasText: new RegExp(`^${label}$`) })
    .locator('xpath=following-sibling::dd[1]');
  await expect(workingComparisonFact('基準')).toHaveText(`Version 2 · ${beforeWorkingComparison.base.title}`);
  await expect(workingComparisonFact('対象')).toHaveText(`WORKING · Version 3 · ${beforeWorkingComparison.target.title}`);
  await expect(workingComparisonFact('基準版ID')).toHaveText(workingComparisonBaseId);
  await expect(workingComparisonFact('対象版ID')).toHaveText(created.targetVersionId);
  await expect(workingComparisonFact('本文')).toHaveText('差分あり');
  await expect(workingComparisonFact('比較範囲')).toHaveText('全範囲');
  await expect(workingComparisonRegion.getByRole('heading', { name: 'メタデータの変更', exact: true })).toBeHidden();
  await expect(workingComparisonRegion.getByRole('listitem')).toHaveCount(workingComparison.items.length);
  const textFragments = workingComparison.items.flatMap(item => [item.base, item.target])
    .flatMap(fragment => fragment?.kind === 'text' ? [fragment.text] : []);
  await expect(workingComparisonRegion.locator('pre')).toHaveText(textFragments);
  // The existing one-line edit ends on this page; real 50+ pagination and multiple originals remain unqualified.
  await expect(workingComparisonRegion.getByRole('button', { name: '内容比較をさらに表示', exact: true })).toBeHidden();
  await expect(workingComparisonRegion.getByRole('button', { name: '内容比較の続きを再試行', exact: true })).toBeHidden();
  await expect(workingComparisonRegion.getByRole('button', { name: '内容比較を最初から読み直す', exact: true })).toBeEnabled();
  await workingComparisonRegion.getByRole('button', { name: '内容比較を閉じる', exact: true }).click();
  await expect(workingComparisonRegion).toBeHidden();
  await expect(workingSelection).toBeVisible();
  await expect(page).toHaveURL(workingComparisonUrl);
  page.off('request', recordWorkingComparisonRequest);
  expect(workingComparisonRequests.filter(value => !value.startsWith('GET '))).toEqual([`POST /v1/documents/${documentId}/comparisons`]);
  // Comparison/download audit may be appended. Authoritative data and both Version/read-state projections must not change.
  expect(await readWorkingComparisonState()).toEqual(beforeWorkingComparison);
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const publishButton = page.getByRole('button', { name: '公開する', exact: true });
  await expect(publishButton).toBeDisabled();
  completed('publication-form-opened');
  await expect(page.getByText('新しい作業版を作成しました。', { exact: true })).toHaveCount(0);
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await expect(publishButton).toBeEnabled();
  await visualCheckpoint(page, '07-publication-ready-1440.png');
  await publishButton.click();
  const dialog = page.getByRole('dialog', { name: '公開を確認' });
  await dialog.getByRole('button', { name: 'キャンセル' }).focus(); await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: '確定する' })).toBeFocused();
  await visualCheckpoint(page, '08-publication-confirm-focus-1440.png');
  const publishResponse = page.waitForResponse(response => response.url().endsWith(':publish') && response.request().method() === 'POST');
  await page.keyboard.press('Enter'); expect((await publishResponse).status()).toBe(200);
  completed('publication-response-accepted');
  await expect(page.getByRole('status')).toContainText('公開しました');
  completed('publication-success-visible');
  const returnToVersions = page.getByRole('button', { name: '版の一覧へ戻る', exact: true });
  await expect(page.locator('section[aria-busy]').filter({ has: publishButton })).toHaveAttribute('aria-busy', 'false');
  await expect(await publishButton.isEnabled() ? publishButton : returnToVersions).toBeFocused();
  await visualCheckpoint(page, '09-publication-success-1440.png');
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
  const savedState = JSON.parse(await readFile(context.statePath, 'utf8'));
  // 同じprivate stateを後続saveSnapshotも保存する。公開診断には追加しない。
  savedState.folderAccessPolicy = { ...currentFolderPolicy, effectiveGrants: grantsOnly(currentFolderPolicy.effectiveGrants) };
  savedState.documentAccessPolicy = { ...policy, effectiveGrants: grantsOnly(policy.effectiveGrants) };
  await writeFile(context.statePath, JSON.stringify(savedState, null, 2), { mode: 0o600 });
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
  await page.getByLabel(/^差替ファイル:/).setInputFiles({ name: 'stale.txt', mimeType: 'text/plain', buffer: bytes });
  const detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  await new BinaryTransportBridge({ baseUrl: context.human }).createVersion(documentId, {
    request: { operationId: uuidV7(), targetVersionId: uuidV7(), expectedRevision: detail.revision, title: detail.title,
      items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'text/plain', originalFilename: 'concurrent.txt' }] },
    files: new Map([['primary', new Blob([bytes], { type: 'text/plain' })]]),
  });
  const responsePromise = page.waitForResponse(response => response.url().endsWith(`/documents/${documentId}/versions`) && response.request().method() === 'POST');
  await page.getByRole('button', { name: '新しい作業版を作成', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(409); expect((await response.json()).code).toBe('REVISION_CONFLICT');
  await expect(page.getByRole('alert')).toContainText('文書の状態が更新されています');
  await expect(page.getByRole('status').filter({ hasText: '新しい作業版を作成しました' })).toHaveCount(0);
  const versions = (await listDocumentVersions({ ...common, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
  expect(versions.items).toHaveLength(2);
  await visualCheckpoint(page, '11-occ-conflict-1440.png');
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
    await page.getByRole('button', { name: /規程サンプル/ }).press('Enter');
    await page.getByRole('button', { name: '詳細を開く' }).press('Enter');
    await expect(page).toHaveURL(new RegExp(documentId));
    await page.getByRole('button', { name: /一覧へ戻る/ }).press('Enter');
    await expect(page.getByRole('button', { name: /規程サンプル/ })).toBeFocused();
    if (width === 1280) await visualCheckpoint(page, '02-list-focus-return-1280.png');
  }
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
