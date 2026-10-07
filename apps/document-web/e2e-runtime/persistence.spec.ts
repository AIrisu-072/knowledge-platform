import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import { BinaryTransportBridge, getDocument, getDocumentAccessPolicy, getFolderAccessPolicy, getSession, listDocumentVersions, resetDocumentVersionReadState, type ModelsHistory, type VersionList, type VersionDetail, type FileList, type ModelsAccessPolicyRead, type PolicyGrantInput } from '@knowledge-platform/document-api-client';
import { currentReadState, hash, observeReadStateChange, options, persistedSnapshot, runtime, type PersistedState } from './support';
import { formatDateTime } from '../src/view-model/date-time';

test('both restarted composition roots retain document/revision/operation IDs and storage hashes', async ({ page, request }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState & {
    folderAccessPolicy: Omit<ModelsAccessPolicyRead, 'effectiveGrants'> & { effectiveGrants: PolicyGrantInput[] };
    documentAccessPolicy: Omit<ModelsAccessPolicyRead, 'effectiveGrants'> & { effectiveGrants: PolicyGrantInput[] };
  };
  expect(state.documents.map(item => item.key).sort()).toEqual(['c3-consistency', 'c3-diff-recovery', 'c3-dsi-recovery', 'gui-initial', 'pdf', 'regulation']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  // GUI表示より前に最後のRESETと同一receiptを照合する。HTTP process再起動の資格だけを取る。
  expect(state.documentReadState).toBeDefined();
  const savedRead = state.documentReadState!;
  const readPath = { documentId: savedRead.state.documentId, versionId: savedRead.state.versionId };
  expect(savedRead.state).toMatchObject({ isRead: false, needsRecheck: true, readStateRevision: 4 });
  expect(await currentReadState(context, readPath)).toEqual(savedRead.state);
  const resetReplay = await resetDocumentVersionReadState({ ...options(context.human), path: readPath, body: savedRead.request });
  expect(resetReplay.response.status).toBe(200); expect(resetReplay.data).toEqual(savedRead.receipt);
  expect(await currentReadState(context, readPath)).toEqual(savedRead.state);
  expect((await getDocument({ ...options(context.human), path: readPath, query: { view: 'published' } })).data.revision).toBe(savedRead.documentRevision);
  // 4回のGUI変更後に保存した正規GETと照合する。同一DBのHTTP再起動でありDB再起動ではない。
  const folderId = context.manifest.folders.shared.folderId;
  const policy = (await getFolderAccessPolicy({ ...options(context.human), path: { folderId } })).data;
  const grants = policy.effectiveGrants.map(({ subjectKind, identityProvider, subjectId, actions }) =>
    ({ subjectKind, identityProvider, subjectId, actions: [...actions].sort() }))
    .sort((left, right) => JSON.stringify([left.subjectKind, left.identityProvider, left.subjectId])
      .localeCompare(JSON.stringify([right.subjectKind, right.identityProvider, right.subjectId])));
  expect({ ...policy, effectiveGrants: grants }).toEqual(state.folderAccessPolicy);
  expect(policy).toMatchObject({ target: { kind: 'folder', id: folderId }, bindingMode: 'explicit',
    policyRevision: context.manifest.folders.shared.policy!.result!.resultingRevision + 4, effectiveSource: { kind: 'folder', id: folderId } });
  await page.goto('/documents?view=published');
  await page.getByRole('button', { name: 'PoC Shared', exact: true }).click();
  const folderPolicyResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.origin === context.human && url.pathname === `/v1/folders/${folderId}/access-policy` && response.request().method() === 'GET';
  });
  await page.getByRole('button', { name: '選択したフォルダーのアクセス設定', exact: true }).click();
  const folderPolicyResult = await folderPolicyResponse;
  expect(folderPolicyResult.status()).toBe(200); expect(await folderPolicyResult.json()).toEqual(policy);
  const folderPolicyDialog = page.getByRole('dialog', { name: '選択したフォルダーのアクセス設定', exact: true });
  await expect(folderPolicyDialog.getByRole('combobox', { name: '設定方式', exact: true })).toHaveValue('explicit');
  for (const grant of grants) {
    const row = folderPolicyDialog.getByRole('group', { name: `${grant.subjectKind} / ${grant.identityProvider} / ${grant.subjectId}`, exact: true });
    for (const [action, label] of [['read', '閲覧'], ['readHistory', '履歴閲覧'], ['write', '編集'], ['publish', '公開'], ['administer', 'アクセス管理']] as const) {
      await expect(row.getByRole('checkbox', { name: label, exact: true })).toBeChecked({ checked: grant.actions.includes(action) });
    }
  }
  await folderPolicyDialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
  await expect(folderPolicyDialog).toBeHidden();
  for (const { key, snapshot } of state.documents) {
    expect(await persistedSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    expect(await persistedSnapshot(context.agent, snapshot.documentId)).toEqual(snapshot);
    if (key === 'regulation') {
      const reopened = await observeReadStateChange(page, context, savedRead.state, 'VIEW', () =>
        page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`));
      expect(reopened.state).toEqual({ ...savedRead.state, isRead: true, needsRecheck: false, readStateRevision: 5 });
      expect(reopened.documentRevision).toBe(savedRead.documentRevision);
    } else await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
    await expect(page.getByRole('heading', { name: snapshot.title, level: 1 })).toBeVisible();
    await expect(page.getByRole('complementary', { name: '原本と版' })).toBeVisible();
    if (key === 'regulation') {
      const documentId = snapshot.documentId;
      const documentPolicy = (await getDocumentAccessPolicy({ ...options(context.human), path: { documentId } })).data;
      const documentGrants = documentPolicy.effectiveGrants.map(({ subjectKind, identityProvider, subjectId, actions }) =>
        ({ subjectKind, identityProvider, subjectId, actions: [...actions].sort() }))
        .sort((left, right) => JSON.stringify([left.subjectKind, left.identityProvider, left.subjectId])
          .localeCompare(JSON.stringify([right.subjectKind, right.identityProvider, right.subjectId])));
      expect({ ...documentPolicy, effectiveGrants: documentGrants }).toEqual(state.documentAccessPolicy);
      expect(documentPolicy).toMatchObject({ target: { kind: 'document', id: documentId }, bindingMode: 'explicit',
        effectivePolicyId: documentPolicy.policyId, effectiveSource: { kind: 'document', id: documentId } });
      expect(documentPolicy.policyId).not.toBeNull();
      expect(documentGrants).toEqual(grants);
      const documentPolicyResponse = page.waitForResponse(response => {
        const url = new URL(response.url());
        return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/access-policy` && response.request().method() === 'GET';
      });
      await page.getByRole('tab', { name: 'アクセス', exact: true }).click();
      const documentPolicyResult = await documentPolicyResponse;
      expect(documentPolicyResult.status()).toBe(200); expect(await documentPolicyResult.json()).toEqual(documentPolicy);
      await expect(page.getByRole('radio', { name: 'この文書だけに個別設定', exact: true })).toBeChecked();
      const effectiveDocumentPolicy = page.getByRole('region', { name: '現在有効なアクセス権', exact: true });
      await expect(effectiveDocumentPolicy.getByRole('rowheader')).toHaveCount(documentGrants.length);
      for (const [index, grant] of documentPolicy.effectiveGrants.entries()) {
        const row = effectiveDocumentPolicy.getByRole('row').nth(index + 1);
        await expect(row.getByRole('rowheader')).toHaveText(grant.presentation.displayName ?? grant.subjectId);
        await expect(row.getByRole('cell')).toHaveText((['read', 'readHistory', 'write', 'publish', 'administer'] as const)
          .map(action => grant.actions.includes(action) ? '許可' : '—'));
      }
      // HTTP再起動後は保存した正規policyを照合する。操作結果storeの永続化は要求しない。
      await expect(page.getByRole('region', { name: '文書アクセス設定の保存結果', exact: true })).toBeHidden();
      const historyReadState = (await getDocument({ ...options(context.human), path: { documentId }, query: { view: 'published' } })).data.readState;
      const historyRegion = page.getByRole('region', { name: '変更履歴', exact: true });
      let initialHistory: ModelsHistory | undefined;
      for (const read of ['open', 'restart'] as const) {
        const historyResponse = page.waitForResponse(response => {
          const url = new URL(response.url());
          return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/history`
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
        // HTTP再起動後も既存の少数履歴を読む。実GUIの101件目や跨page snapshotは未資格。
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
      expect(isDeepStrictEqual(await persistedSnapshot(context.human, documentId), snapshot)).toBe(true);
      expect(isDeepStrictEqual(await persistedSnapshot(context.agent, documentId), snapshot)).toBe(true);
      expect(isDeepStrictEqual((await getDocument({ ...options(context.human), path: { documentId }, query: { view: 'published' } })).data.readState, historyReadState)).toBe(true);
      await page.getByRole('tab', { name: '版・改訂', exact: true }).click();
      const common = options(context.human);
      const oldVersion = snapshot.versions.find(version => version.versionNo === 1)!;
      expect(oldVersion.versionId).not.toBe(snapshot.currentVersionId);
      const currentVersion = snapshot.versions.find(version => version.versionId === snapshot.currentVersionId)!;
      const normalSelection = page.getByRole('heading', { name: `選択中: 版 ${currentVersion.versionNo}`, exact: true });
      await expect(normalSelection).toBeVisible();
      // This document has no WORKING after publication; this does not qualify a positive comparison after restart.
      expect(snapshot.versions.every(version => version.lifecycleState !== 'working')).toBe(true);
      await expect(page.getByRole('button', { name: '現行公開版とこの作業版を比較', exact: true })).toBeHidden();
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
  await test.info().attach('restart-persistence.json', { body: Buffer.from(JSON.stringify({ documents: state.documents }, null, 2)), contentType: 'application/json' });
});
