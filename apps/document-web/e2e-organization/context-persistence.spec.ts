import { expect, test } from '@playwright/test';
import type { TaskAttention, TaskDetail, TaskPage, WorkContextPage } from '../src/api/generated-work/types.gen';
import { hidden, openPage, read } from './policy-support';
import { contextAction, loadContextState, readContextRuntime } from './context-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });

test('6 processの再起動後も文脈・確認済み・差戻しの注意と非開示を保持する', async ({ browser, request }) => {
  const context = readContextRuntime();
  const state = await loadContextState(context);
  contextAction('context-persistence');
  // The acknowledgment persisted: no newly-assigned attention, work still active.
  expect(await read<TaskAttention>(request, context.office, `/v1/organization/tasks/${state.cOfficeTaskId}/attention`)).toMatchObject({ taskId: state.cOfficeTaskId, items: [] });
  const officeTask = await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${state.cOfficeTaskId}`);
  expect(officeTask).toMatchObject({ state: 'active', contextTitle: '合成依頼C・住所変更届', assignment: { principalId: 'office-01' } });
  // Acknowledging again is idempotent and still not a Work mutation.
  const again = await request.post(`${context.office}/v1/organization/tasks/${state.cOfficeTaskId}/attention-seen`, { data: { workAssignmentId: state.officeAssigned.result.assignment.id } });
  expect(again.status()).toBe(200);
  expect((await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${state.cOfficeTaskId}`)).revision).toBe(officeTask.revision);
  // Returned attention and context progress survive the restart; receipts recover by operation ID.
  const bSales = (await read<TaskPage>(request, context.sales, '/v1/organization/tasks?view=context')).items.find((item) => item.id === state.bSalesTaskId)!;
  expect(bSales.attention.map((value) => value.kind)).toContain('returned');
  const contexts = (await read<WorkContextPage>(request, context.sales, '/v1/organization/work-contexts')).items;
  expect(contexts.map((value) => value.id)).toEqual(expect.arrayContaining([state.contextB, state.contextC]));
  expect(await read(request, context.review, `/v1/organization/operations/${state.bReturned.operationId}`)).toEqual(state.bReturned.result);
  expect(await read(request, context.sales, `/v1/organization/operations/${state.cSubmitted.operationId}`)).toEqual(state.cSubmitted.result);
  // Non-disclosure is unchanged.
  await hidden(request, context.office, `/v1/organization/work-contexts/${state.contextB}`, 'WORK_CONTEXT_NOT_FOUND', '合成案件B');
  expect((await read<WorkContextPage>(request, context.delegate, '/v1/organization/work-contexts')).items).toEqual([]);
  const office = await openPage(browser, context.office);
  try {
    await office.goto(`/tasks?taskId=${state.cOfficeTaskId}`);
    await expect(office.getByText('文脈：合成依頼C・住所変更届', { exact: true })).toBeVisible();
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(state.text);
    await expect(office.getByRole('button', { name: '確認済みにする', exact: true })).toHaveCount(0);
  } finally {
    await office.context().close();
  }
});
