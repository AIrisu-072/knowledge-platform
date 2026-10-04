import { expect, test } from '@playwright/test';
import type { HandoffSnapshot, ReturnInstruction, WorkingArtifact } from '../src/api/generated-work/types.gen';
import { assertEvidenceState, assertHidden, assertSessions, captureFinal, get, loadState, readRuntimeContext } from './support';

test('両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する', async ({ page, browser, request }) => {
  // The owning harness stops and restarts both processes before invoking this phase.
  // This test deliberately neither spawns a process nor prepares/reseeds a database.
  const context = readRuntimeContext();
  const state = await loadState(context);
  await assertSessions(request, context);
  const resubmissionId = state.rework.submit.result.snapshot.id;
  const instructionId = state.rework.returned.result.returnInstruction.id;
  const actual = await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId);
  expect(actual).toEqual(state.final);
  expect(actual.salesTask).toMatchObject({ id: state.salesTaskId, state: 'completed', attemptNumber: 2, canEdit: false, handoffSnapshotId: resubmissionId });
  expect(actual.officeTask).toMatchObject({ id: state.officeTaskId, state: 'active', attemptNumber: 2, canClaim: false, canEdit: false, workingArtifacts: [], handoffSnapshotId: resubmissionId });
  expect(actual.snapshot).toEqual(state.rework.submit.result.snapshot);
  expect(actual.priorSnapshot).toEqual(state.submit.result.snapshot);
  expect(actual.returnInstruction).toEqual(state.rework.returned.result.returnInstruction);
  expect(await get<HandoffSnapshot>(request, context.office, `/v1/organization/handoff-snapshots/${state.snapshotId}`)).toEqual(state.final.priorSnapshot);
  expect(await get<HandoffSnapshot>(request, context.office, `/v1/organization/handoff-snapshots/${resubmissionId}`)).toEqual(state.final.snapshot);
  expect(await get<ReturnInstruction>(request, context.office, `/v1/organization/return-instructions/${instructionId}`)).toEqual(state.final.returnInstruction);
  await assertHidden(request, context.sales, `/v1/organization/working-artifacts/${state.artifactId}`, 'WORK_ARTIFACT_NOT_FOUND', state.text);
  await assertHidden(request, context.sales, `/v1/organization/operations/${state.save.operationId}`, 'WORK_ARTIFACT_NOT_FOUND', state.text);
  expect(await get<WorkingArtifact>(request, context.sales, `/v1/organization/working-artifacts/${state.rework.save.result.artifact.id}`)).toEqual(state.rework.save.result.artifact);
  for (const receipt of [state.submit, state.rework.salesClaim, state.rework.save, state.rework.submit]) expect(await get(request, context.sales, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
  for (const receipt of [state.claim, state.rework.returned, state.rework.officeClaim]) expect(await get(request, context.office, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence);
  const decisionReplay = await request.post(`${context.office}/v1/organization/findings/${state.evidence.finding.result.finding.id}/decisions`, { data: state.evidence.officeReworkDecision.command });
  expect(decisionReplay.status()).toBe(200);
  expect(await decisionReplay.json()).toEqual(state.evidence.officeReworkDecision.result);
  // Replaying the exact operation must not append another immutable decision or task revision.
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence);
  const replay = await request.post(`${context.office}/v1/organization/tasks/${state.officeTaskId}/return`, { data: state.rework.returned.command });
  expect(replay.status()).toBe(200);
  expect(await replay.json()).toEqual(state.rework.returned.result);
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId)).toEqual(state.final);
  await assertHidden(request, context.office, `/v1/organization/working-artifacts/${state.rework.save.result.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', state.rework.text);
  await assertHidden(request, context.office, `/v1/organization/operations/${state.rework.save.operationId}`, 'WORK_ITEM_NOT_FOUND', state.rework.text);
  await assertHidden(request, context.office, `/v1/organization/tasks/${state.salesTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/working-artifacts/${state.artifactId}`, 'WORK_ARTIFACT_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/operations/${state.save.operationId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.sales, `/v1/organization/tasks/${state.officeTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  for (const origin of [context.sales, context.office]) expect((await get<{ documentId: string }>(request, origin, `/v1/documents/${state.documentId}?view=published`)).documentId).toBe(state.documentId);

  await page.goto(`/tasks?view=context&taskId=${state.salesTaskId}`);
  await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(state.rework.text);
  await expect(page.getByRole('region', { name: '差戻前のスナップショット', exact: true })).toContainText(state.text);
  await expect(page.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(state.final.returnInstruction.reason);
  await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '根拠', exact: true }).click();
  const salesFinding = page.getByRole('region', { name: `候補 ${state.evidence.finding.result.finding.id}`, exact: true });
  await expect(salesFinding).toContainText(state.evidence.finding.result.finding.claim);
  for (const receipt of state.evidence.decisions) {
    await expect(salesFinding).toContainText(receipt.result.decision.reason!);
    if (receipt.result.decision.adoptedClaim) await expect(salesFinding).toContainText(receipt.result.decision.adoptedClaim);
  }
  await expect(page.getByRole('region', { name: `候補 ${state.evidence.rework.finding.result.finding.id}`, exact: true })).toContainText(state.evidence.rework.finding.result.finding.claim);
  await expect(page.getByText(state.evidence.privateFinding.result.finding.claim, { exact: true })).toHaveCount(0);
  await expect(page.getByText(state.evidence.officeReworkDecision.result.decision.reason!, { exact: false })).toHaveCount(0);
  const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false });
  const office = await officeContext.newPage();
  try {
    await office.goto(`${context.office}/tasks?view=queue&taskId=${state.officeTaskId}`);
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(state.rework.text);
    await expect(office.getByRole('region', { name: '差戻前のスナップショット', exact: true })).toContainText(state.text);
    await expect(office.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(state.final.returnInstruction.reason);
    await expect(office.getByRole('button', { name: '担当を引き受ける', exact: true })).toHaveCount(0);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    await office.getByRole('button', { name: '根拠', exact: true }).click();
    const officeFinding = office.getByRole('region', { name: `候補 ${state.evidence.finding.result.finding.id}`, exact: true });
    await expect(officeFinding).toContainText(state.evidence.finding.result.finding.claim);
    for (const receipt of [...state.evidence.decisions, state.evidence.officeReworkDecision]) await expect(officeFinding).toContainText(receipt.result.decision.reason!);
    await expect(office.getByText(state.evidence.officeDecision.result.decision.reason!, { exact: false })).toHaveCount(0);
    for (const finding of [state.evidence.privateFinding.result.finding, state.evidence.rework.finding.result.finding]) await expect(office.getByText(finding.claim, { exact: true })).toHaveCount(0);
    for (const evidence of [state.evidence.unselected.result.evidence, state.evidence.rework.evidence.result.evidence]) {
      await expect(office.getByRole('region', { name: `根拠 ${evidence.id}`, exact: true })).toHaveCount(0);
      await expect(office.getByText(evidence.relevantLocation, { exact: false })).toHaveCount(0);
    }
    await office.getByRole('button', { name: '文書・比較', exact: true }).click();
    const input = office.getByRole('link', { name: '共有入力文書', exact: true });
    await expect(input).toHaveAttribute('href', new RegExp(`/documents/${state.documentId}`));
  } finally {
    await officeContext.close();
  }
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence);
  // Merely reading both UIs must not create another handoff, task revision or history entry.
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId)).toEqual(state.final);
});
