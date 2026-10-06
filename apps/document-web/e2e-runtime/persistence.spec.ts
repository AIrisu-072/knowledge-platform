import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { BinaryTransportBridge, getDocument, getSession, listDocumentVersions, type VersionList, type VersionDetail, type FileList } from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, type PersistedState } from './support';

test('both restarted composition roots retain document/revision/operation IDs and storage hashes', async ({ page, request }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState;
  expect(state.documents.map(item => item.key).sort()).toEqual(['c3-consistency', 'c3-diff-recovery', 'c3-dsi-recovery', 'gui-initial', 'pdf', 'regulation']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  for (const { key, snapshot } of state.documents) {
    expect(await persistedSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    expect(await persistedSnapshot(context.agent, snapshot.documentId)).toEqual(snapshot);
    await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
    await expect(page.getByRole('heading', { name: snapshot.title, level: 1 })).toBeVisible();
    await expect(page.getByRole('complementary', { name: '原本と版' })).toBeVisible();
    if (key === 'regulation') {
      await page.getByRole('tab', { name: '版・改訂', exact: true }).click();
      const documentId = snapshot.documentId, common = options(context.human);
      const oldVersion = snapshot.versions.find(version => version.versionNo === 1)!;
      expect(oldVersion.versionId).not.toBe(snapshot.currentVersionId);
      const currentVersion = snapshot.versions.find(version => version.versionId === snapshot.currentVersionId)!;
      const normalSelection = page.getByRole('heading', { name: `選択中: 版 ${currentVersion.versionNo}`, exact: true });
      await expect(normalSelection).toBeVisible();
      const normalUrl = page.url();
      const readStateBeforeHistory = (await getDocument({ ...common, path: { documentId }, query: { view: 'published' } })).data.readState;
      const versionsBeforeHistory = (await listDocumentVersions({ ...common, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
      const versionReadStatesBeforeHistory = versionsBeforeHistory.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt }));
      const historyResponse = (suffix: string) => page.waitForResponse(response => {
        const url = new URL(response.url());
        return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/versions${suffix}`
          && url.searchParams.get('purpose') === 'history' && response.request().method() === 'GET';
      });
      const historyListResponse = historyResponse('');
      await page.getByRole('button', { name: 'コンテンツ版の履歴を開く', exact: true }).click();
      const historyListResult = await historyListResponse;
      expect(historyListResult.status()).toBe(200);
      expect(new URL(historyListResult.url()).searchParams.get('pageSize')).toBe('100');
      expect(new URL(historyListResult.url()).searchParams.has('cursor')).toBe(false);
      const historyVersions = await historyListResult.json() as VersionList;
      expect(historyVersions.items.map(version => version.versionId)).toEqual(snapshot.versions.map(version => version.versionId));
      expect(historyVersions.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt }))).toEqual(versionReadStatesBeforeHistory);
      // 既存の3版のHTTP再起動後readであり、実GUIの101件目は未資格のまま。
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
      expect((await getDocument({ ...common, path: { documentId }, query: { view: 'published' } })).data.readState).toEqual(readStateBeforeHistory);
      const versionsAfterHistory = (await listDocumentVersions({ ...common, path: { documentId }, query: { purpose: 'history', pageSize: 100 } })).data;
      expect(versionsAfterHistory.items.map(({ versionId, firstReadAt }) => ({ versionId, firstReadAt })))
        .toEqual(versionReadStatesBeforeHistory);
      expect(await persistedSnapshot(context.human, documentId)).toEqual(snapshot);
      expect(await persistedSnapshot(context.agent, documentId)).toEqual(snapshot);
    }
  }
  const drain = JSON.parse(await readFile(context.drainFixturePath, 'utf8')) as { documentId: string; versionId: string; contentItemId: string; representationId: string; sizeBytes: number; sha256: string };
  const drained = (await getDocument({ ...options(context.human), path: { documentId: drain.documentId }, query: { view: 'authoring' } })).data;
  expect(drained.metadata).toMatchObject({ extensions: { runtimeDrainObservation: 'synthetic-in-flight-completed' } });
  const original = await new BinaryTransportBridge({ baseUrl: context.human }).downloadVersionFileBlob({ ...drain, purpose: 'authoring' });
  expect(original.size).toBe(drain.sizeBytes); expect(hash(new Uint8Array(await original.arrayBuffer()))).toBe(drain.sha256);
  const deniedId = context.manifest.documents.humanOnly!.create!.result!.documentId;
  const denied = await request.get(`${context.agent}/v1/documents/${deniedId}?view=published`);
  expect(denied.status()).toBe(404); expect((await denied.json()).code).toBe('DOCUMENT_NOT_FOUND');
  await test.info().attach('restart-persistence.json', { body: Buffer.from(JSON.stringify(state, null, 2)), contentType: 'application/json' });
});
