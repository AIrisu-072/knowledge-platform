import { expect, test } from '@playwright/test';
import { assertAgentState, assertEvidenceState, assertSessions, captureFinal, currentAction, loadState, readRuntimeContext } from '../support';

// Explicitly selected by the owner: same saved fixture, GET-only, no UI or writes.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
// Reuse the closed existing persistence classifier; the private file/base distinguishes this probe.
test('両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する', async ({ request }) => {
  const context = readRuntimeContext();
  const state = await loadState(context);
  currentAction('persistence-verify');
  await assertSessions(request, context);
  const resubmissionId = state.rework.submit.result.snapshot.id;
  const instructionId = state.rework.returned.result.returnInstruction.id;
  const actual = await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId);
  expect(actual).toEqual(state.final);
  await assertEvidenceState(request, context, state.salesTaskId, state.officeTaskId, state.evidence, state.agents);
  await assertAgentState(request, context, state.agents);
  expect(await captureFinal(request, context, state.salesTaskId, state.officeTaskId, resubmissionId, state.snapshotId, instructionId)).toEqual(state.final);
});
