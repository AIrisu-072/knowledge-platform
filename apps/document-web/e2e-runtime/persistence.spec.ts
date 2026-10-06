import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import { BinaryTransportBridge, getDocument, getSession, type ModelsHistory } from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, type PersistedState } from './support';
import { formatDateTime } from '../src/view-model/date-time';

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
      const documentId = snapshot.documentId;
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
