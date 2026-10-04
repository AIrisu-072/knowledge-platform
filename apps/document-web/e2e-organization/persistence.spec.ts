import { expect, test } from '@playwright/test';
import type { HandoffSnapshot, WorkingArtifact } from '../src/api/generated-work/types.gen';
import { assertHidden, assertSessions, captureFinal, get, loadState, readRuntimeContext } from './support';

test('両process再起動後もタスク・提出snapshot・操作結果・private非開示を保持する', async ({ page, browser, request }) => {
  // The owning harness stops and restarts both processes before invoking this phase.
  // This test deliberately neither spawns a process nor prepares/reseeds a database.
  const context = readRuntimeContext();
  const state = await loadState(context);
  await assertSessions(request, context);
  const actual = await captureFinal(request, context, state.salesTaskId, state.officeTaskId, state.snapshotId);
  expect(actual).toEqual(state.final);
  expect(actual.salesTask).toMatchObject({ id: state.salesTaskId, state: 'completed', canEdit: false, handoffSnapshotId: state.snapshotId });
  expect(actual.officeTask).toMatchObject({ id: state.officeTaskId, state: 'active', canClaim: false, canEdit: false, workingArtifacts: [], handoffSnapshotId: state.snapshotId });
  expect(actual.snapshot).toEqual(state.submit.result.snapshot);
  expect(await get<HandoffSnapshot>(request, context.office, `/v1/organization/handoff-snapshots/${state.snapshotId}`)).toEqual(state.final.snapshot);
  expect(await get<WorkingArtifact>(request, context.sales, `/v1/organization/working-artifacts/${state.artifactId}`)).toEqual(state.save.result.artifact);
  for (const receipt of [state.save, state.submit]) expect(await get(request, context.sales, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
  expect(await get(request, context.office, `/v1/organization/operations/${state.claim.operationId}`)).toEqual(state.claim.result);
  await assertHidden(request, context.office, `/v1/organization/tasks/${state.salesTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/working-artifacts/${state.artifactId}`, 'WORK_ARTIFACT_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/operations/${state.save.operationId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.sales, `/v1/organization/tasks/${state.officeTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  for (const origin of [context.sales, context.office]) expect((await get<{ documentId: string }>(request, origin, `/v1/documents/${state.documentId}?view=published`)).documentId).toBe(state.documentId);

  await page.goto(`/tasks?view=context&taskId=${state.salesTaskId}`);
  await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(state.text);
  await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
  const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false });
  const office = await officeContext.newPage();
  try {
    await office.goto(`${context.office}/tasks?view=queue&taskId=${state.officeTaskId}`);
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(state.text);
    await expect(office.getByRole('button', { name: '担当を引き受ける', exact: true })).toHaveCount(0);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    const input = office.getByRole('link', { name: '共有入力文書', exact: true });
    await expect(input).toHaveAttribute('href', new RegExp(`/documents/${state.documentId}`));
  } finally {
    await officeContext.close();
  }
  // Merely reading both UIs must not create another handoff, task revision or history entry.
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, state.snapshotId)).toEqual(state.final);
});
