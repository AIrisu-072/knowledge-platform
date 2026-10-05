import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { getDocument, getSession } from '@knowledge-platform/document-api-client';
import { options, runtime } from './support';
import { lifecycleSnapshot } from './lifecycle-support';
import { workingEditorSnapshot, workingEditorStatePath, type WorkingEditorState } from './working-version-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test('両HTTP再起動後も初回WORKINGと複数原本の公開切替・元名・保持bytes・操作IDを復元する', async ({ page }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(workingEditorStatePath(context), 'utf8')) as WorkingEditorState;
  expect(state.documents.map(item => item.key).sort()).toEqual(['initial', 'multiple']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  for (const { key, snapshot } of state.documents) {
    expect(await workingEditorSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    if (key === 'initial') {
      expect(snapshot.currentVersionId).toBeNull();
      const hidden = await getDocument({ ...options(context.agent), throwOnError: false,
        path: { documentId: snapshot.documentId }, query: { view: 'published' } });
      expect(hidden.response?.status).toBe(404);
      await page.goto(`/documents/${snapshot.documentId}?view=authoring&tab=versions`);
      await expect(page.getByRole('button', { name: /WORKING · 版 1/ })).toBeVisible();
      await expect(page.getByRole('button', { name: '作業版を編集', exact: true }).first()).toBeEnabled();
    } else {
      expect(snapshot.versions).toHaveLength(2);
      expect(await lifecycleSnapshot(context.agent, snapshot.documentId)).toEqual(await lifecycleSnapshot(context.human, snapshot.documentId));
      await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
      await expect(page.getByRole('heading', { name: '【合成データ】複数原本編集', level: 1 })).toBeVisible();
    }
  }
  test.info().annotations.push({ type: 'runtime-completed', description: 'gui-working-restart-verified' });
});
