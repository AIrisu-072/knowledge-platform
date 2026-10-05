import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { getDocument, getSession } from '@knowledge-platform/document-api-client';
import { options, runtime } from './support';
import { lifecycleSnapshot, lifecycleStatePath, type LifecycleState } from './lifecycle-support';

// Restart proof for this slice has the same no-recording boundary as its journey.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test('両HTTP再起動後もGUI取下げ・公開終了の公開不可と履歴・原本・操作IDが保持される', async ({ page }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(lifecycleStatePath(context), 'utf8')) as LifecycleState;
  expect(state.documents.map(item => item.key).sort()).toEqual(['ended', 'withdrawn']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  for (const { key, snapshot } of state.documents) {
    expect(snapshot.published).toEqual({ status: 404, code: 'DOCUMENT_NOT_FOUND' });
    expect(await lifecycleSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    expect(await lifecycleSnapshot(context.agent, snapshot.documentId)).toEqual(snapshot);
    if (key === 'ended') {
      const detail = await getDocument({ ...options(context.human), throwOnError: false,
        path: { documentId: snapshot.documentId }, query: { view: 'authoring' } });
      expect(detail.response?.status).toBe(404); expect(detail.error?.code).toBe('DOCUMENT_NOT_FOUND');
    }
    await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
    await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
    await expect(page.getByRole('complementary', { name: '原本と版' })).toHaveCount(0);
  }
  test.info().annotations.push({ type: 'runtime-completed', description: 'gui-lifecycle-restart-verified' });
});
