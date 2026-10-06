import { currentAction } from './support';
import { expect, test } from '@playwright/test';
import { isDeepStrictEqual } from 'node:util';
import { replaySelectedFolderMove, replaySelectedFolderRename, assertSelectedFolderUi, replaySelectedFolderCreate, assertFolderPaginationUi, assertRootFolderCreated, assertRootFolderUi, loadRootFolderState, openRootFolderHome, readRootFolderSnapshot, replayRootFolderCreate } from './support';
import type { HandoffSnapshot, ReturnInstruction, WorkingArtifact } from '../src/api/generated-work/types.gen';
import { assertHoldResumeState, assertCompletionState, assertEvidenceState, assertAgentState, assertHidden, assertSessions, captureFinal, get, loadState, readRuntimeContext } from './support';

// These worker-scoped settings explicitly preserve the existing image-free runtime configuration.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });

test('両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する', async ({ page, browser, request }) => {
  // The owning harness stops and restarts both processes before invoking this phase.
  // This test deliberately neither spawns a process nor prepares/reseeds a database.
  const context = readRuntimeContext();
  currentAction('persistence-verify');
  const state = await loadState(context);
  await assertSessions(request, context);
  const resubmissionId = state.rework.submit.result.snapshot.id;
  const instructionId = state.rework.returned.result.returnInstruction.id;
  const actual = await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId);
  expect(actual).toEqual(state.final);
  expect(actual.salesTask).toMatchObject({ id: state.salesTaskId, state: 'completed', attemptNumber: 2, canEdit: false, handoffSnapshotId: resubmissionId });
  expect(actual.officeTask).toMatchObject({ id: state.officeTaskId, state: 'completed', attemptNumber: 2, canComplete: false, completionActionId: null, canClaim: false, canEdit: false, workingArtifacts: [], handoffSnapshotId: resubmissionId });
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
  currentAction('persistence-verify');
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence, state.agents);
  await assertAgentState(request, context, state.agents);
  await assertCompletionState(request, context, state.completion);
  await assertHoldResumeState(request, context, state.holdResume);
  for (const [role, receipt] of Object.entries(state.agents) as ['sales' | 'office', typeof state.agents.sales][]) {
    const replay = await request.post(`${context[role]}/v1/organization/tasks/${receipt.execution.workItemId}/agent-executions`, { data: receipt.command });
    expect(replay.status()).toBe(202);
    expect(await replay.json()).toEqual(receipt.result);
  }
  await assertAgentState(request, context, state.agents);
  const decisionReplay = await request.post(`${context.office}/v1/organization/findings/${state.evidence.finding.result.finding.id}/decisions`, { data: state.evidence.officeReworkDecision.command });
  expect(decisionReplay.status()).toBe(200);
  expect(await decisionReplay.json()).toEqual(state.evidence.officeReworkDecision.result);
  // Replaying the exact operation must not append another immutable decision or task revision.
  currentAction('persistence-verify');
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence, state.agents);
  const replay = await request.post(`${context.office}/v1/organization/tasks/${state.officeTaskId}/return`, { data: state.rework.returned.command });
  expect(replay.status()).toBe(200);
  expect(await replay.json()).toEqual(state.rework.returned.result);
  currentAction('persistence-verify');
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId)).toEqual(state.final);
  await assertHidden(request, context.office, `/v1/organization/working-artifacts/${state.rework.save.result.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', state.rework.text);
  await assertHidden(request, context.office, `/v1/organization/operations/${state.rework.save.operationId}`, 'WORK_ITEM_NOT_FOUND', state.rework.text);
  await assertHidden(request, context.office, `/v1/organization/tasks/${state.salesTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/working-artifacts/${state.artifactId}`, 'WORK_ARTIFACT_NOT_FOUND', state.text);
  await assertHidden(request, context.office, `/v1/organization/operations/${state.save.operationId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await assertHidden(request, context.sales, `/v1/organization/tasks/${state.officeTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  for (const origin of [context.sales, context.office]) expect((await get<{ documentId: string }>(request, origin, `/v1/documents/${state.documentId}?view=published`)).documentId).toBe(state.documentId);

  currentAction('sales-navigation');
  await page.goto(`/tasks?view=context&taskId=${state.salesTaskId}`);
  await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(state.rework.text);
  await expect(page.getByRole('region', { name: '差戻前のスナップショット', exact: true })).toContainText(state.text);
  await expect(page.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(state.final.returnInstruction.reason);
  await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
  currentAction('evidence-module');
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
  const salesGenerated = page.getByRole('region', { name: `候補 ${state.agents.sales.finding.id}`, exact: true });
  await expect(salesGenerated).toContainText('organization-synthetic/agent-01');
  await expect(salesGenerated).toContainText(`生成元の実行 ${state.agents.sales.execution.id}`);
  await expect(salesGenerated).toContainText(state.agents.sales.decision.result.decision.adoptedClaim!);
  currentAction('agent-module');
  await page.getByRole('button', { name: 'Agent', exact: true }).click();
  const persistedSalesExecution = page.getByRole('region', { name: `Agent実行 ${state.agents.sales.execution.id}`, exact: true });
  await expect(persistedSalesExecution).toContainText('実行状態：成功');
  await expect(persistedSalesExecution).toContainText(state.agents.sales.output.summary);
  const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false });
  const office = await officeContext.newPage();
  try {
    currentAction('office-navigation');
    await office.goto(`${context.office}/tasks?view=queue&taskId=${state.officeTaskId}`);
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(state.rework.text);
    await expect(office.getByRole('region', { name: '差戻前のスナップショット', exact: true })).toContainText(state.text);
    await expect(office.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(state.final.returnInstruction.reason);
    await expect(office.getByRole('button', { name: '担当を引き受ける', exact: true })).toHaveCount(0);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    await expect(office.getByRole('button', { name: '完了内容を確認', exact: true })).toHaveCount(0);
    await expect(office.getByLabel('差戻理由', { exact: true })).toHaveCount(0);
    await office.getByRole('button', { name: '履歴', exact: true }).click();
    await expect(office.getByText('タスクを完了', { exact: true })).toBeVisible();
    currentAction('evidence-module');
    await office.getByRole('button', { name: '根拠', exact: true }).click();
    await expect(office.getByRole('button', { name: '判断内容を確認', exact: true })).toHaveCount(0);
    const officeFinding = office.getByRole('region', { name: `候補 ${state.evidence.finding.result.finding.id}`, exact: true });
    await expect(officeFinding).toContainText(state.evidence.finding.result.finding.claim);
    for (const receipt of [...state.evidence.decisions, state.evidence.officeReworkDecision]) await expect(officeFinding).toContainText(receipt.result.decision.reason!);
    await expect(office.getByText(state.evidence.officeDecision.result.decision.reason!, { exact: false })).toHaveCount(0);
    for (const finding of [state.evidence.privateFinding.result.finding, state.evidence.rework.finding.result.finding]) await expect(office.getByText(finding.claim, { exact: true })).toHaveCount(0);
    for (const evidence of [state.evidence.unselected.result.evidence, state.evidence.rework.evidence.result.evidence]) {
      await expect(office.getByRole('region', { name: `根拠 ${evidence.id}`, exact: true })).toHaveCount(0);
      await expect(office.getByText(evidence.relevantLocation, { exact: false })).toHaveCount(0);
    }
    const officeGenerated = office.getByRole('region', { name: `候補 ${state.agents.office.finding.id}`, exact: true });
    await expect(officeGenerated).toContainText('organization-synthetic/agent-01');
    await expect(officeGenerated).toContainText(`生成元の実行 ${state.agents.office.execution.id}`);
    await expect(officeGenerated).toContainText(state.agents.office.decision.result.decision.reason!);
    const sharedGenerated = office.getByRole('region', { name: `候補 ${state.agents.sales.finding.id}`, exact: true });
    await expect(sharedGenerated).toContainText(state.agents.sales.decision.result.decision.adoptedClaim!);
    currentAction('agent-module');
    await office.getByRole('button', { name: 'Agent', exact: true }).click();
    const persistedOfficeExecution = office.getByRole('region', { name: `Agent実行 ${state.agents.office.execution.id}`, exact: true });
    await expect(persistedOfficeExecution).toContainText('実行状態：成功');
    await expect(persistedOfficeExecution).toContainText(state.agents.office.output.summary);
    await expect(office.getByLabel('Agentへの依頼目的', { exact: true })).toBeDisabled();
    await expect(office.getByRole('region', { name: `Agent実行 ${state.agents.sales.execution.id}`, exact: true })).toHaveCount(0);
    currentAction('document-navigation');
    await office.getByRole('button', { name: '文書・比較', exact: true }).click();
    const input = office.getByRole('link', { name: '共有入力文書', exact: true });
    await expect(input).toHaveAttribute('href', new RegExp(`/documents/${state.documentId}`));
  } finally {
    await officeContext.close();
  }
  currentAction('persistence-verify');
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence, state.agents);
  await assertAgentState(request, context, state.agents);
  // Merely reading both UIs must not create another handoff, task revision or history entry.
  currentAction('persistence-verify');
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId)).toEqual(state.final);
});

test.describe('System Root folder creation', () => {
  test.use({ acceptDownloads: false });

  test('両process再起動後もRoot直下フォルダーと固定要求replayと現在権限を保持する', async ({ page, browser, request }) => {
    // The existing harness already restarted both HTTP processes; no process, DB or seed setup here.
    const context = readRuntimeContext();
    currentAction('root-folder-persistence');
    const state = await loadRootFolderState(context);
    await assertSessions(request, context);
    for (const role of ['sales', 'office'] as const) {
      const actual = await readRootFolderSnapshot(request, context[role]);
      expect(isDeepStrictEqual(actual, state[role])).toBe(true);
      assertRootFolderCreated(actual, state.request);
    }
    await replayRootFolderCreate(request, context, state);
    await openRootFolderHome(page, context.sales);
    await assertRootFolderUi(page, state.sales, state.request.folderId, 'sales');
    await assertFolderPaginationUi(page, state);
    await assertSelectedFolderUi(page, request, context, state);
    await replaySelectedFolderCreate(request, context, state);
    await replaySelectedFolderRename(request, context, state);
    await replaySelectedFolderMove(request, context, state);
    // No tracing, video or screenshot capture for this independent office context.
    const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false, recordVideo: undefined });
    try {
      const office = await officeContext.newPage();
      await openRootFolderHome(office, context.office);
      currentAction('root-folder-office');
      await assertRootFolderUi(office, state.office, state.request.folderId, 'office');
    } finally {
      await officeContext.close();
    }
    currentAction('root-folder-persistence');
    for (const role of ['sales', 'office'] as const) expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
  });
});
