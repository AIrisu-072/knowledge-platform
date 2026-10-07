import { expect, test } from '@playwright/test';
import type { Delegation, RoleAssignment, TaskDetail, WorkSession } from '../src/api/generated-work/types.gen';
import { hidden, loadPolicyState, openPage, policyAction, read, readPolicyContext, sessions } from './policy-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });

test('6 processの再起動後も割当・委任・担当変更・取消と非開示を保持する', async ({ browser, request }) => {
  // The owning harness restarted all six processes on the same owned database.
  const context = readPolicyContext();
  policyAction('policy-persistence');
  const state = await loadPolicyState(context);
  const current = await sessions(request, context);
  expect(current.delegate.responsibilities).toEqual([]);
  expect(current.multiRole.responsibilities?.map((value) => value.roleLabel)).toEqual(['審査']);
  expect(current.review.responsibilities?.map((value) => value.roleLabel)).toEqual(['審査', '営業']);
  expect((await read<{ items: RoleAssignment[] }>(request, context.approver, '/v1/organization/role-assignments')).items).toEqual(state.final.assignments);
  expect((await read<{ items: Delegation[] }>(request, context.approver, '/v1/organization/delegations')).items).toEqual(state.final.delegations);
  expect(await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${state.officeTaskId}`)).toEqual(state.final.officeTask);
  expect(await read<TaskDetail>(request, context.review, `/v1/organization/tasks/${state.salesTaskId}`)).toEqual(state.final.reviewSalesTask);
  // Committed receipts recover by the same operation ID under current visibility.
  expect(await read(request, context.approver, `/v1/organization/operations/${state.roleAssignment.operationId}`)).toEqual(state.roleAssignment.result);
  expect(await read(request, context.approver, `/v1/organization/operations/${state.assignmentRevoked.operationId}`)).toEqual(state.assignmentRevoked.result);
  expect(await read(request, context.office, `/v1/organization/operations/${state.delegation.operationId}`)).toEqual(state.delegation.result);
  expect(await read(request, context.office, `/v1/organization/operations/${state.delegationRevoked.operationId}`)).toEqual(state.delegationRevoked.result);
  expect(await read(request, context.approver, `/v1/organization/operations/${state.salesAssigned.operationId}`)).toEqual(state.salesAssigned.result);
  // Exact replays return the stored receipt; a changed reason with the same ID conflicts.
  const replay = await request.post(`${context.approver}/v1/organization/tasks/${state.officeTaskId}/assignment`, { data: state.officeAssigned.command });
  expect(replay.status()).toBe(200);
  expect(await replay.json()).toEqual(state.officeAssigned.result);
  const conflict = await request.post(`${context.office}/v1/organization/delegations`, { data: { ...state.delegation.command, reason: '変更した理由' } });
  expect(conflict.status()).toBe(409);
  expect(((await conflict.json()) as { code: string }).code).toBe('OPERATION_CONFLICT');
  // Revocation and reassignment stay effective after restart: no private disclosure.
  await hidden(request, context.sales, `/v1/organization/tasks/${state.salesTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await hidden(request, context[state.claimWinner], `/v1/organization/tasks/${state.officeTaskId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  await hidden(request, context.delegate, `/v1/organization/operations/${state.delegation.operationId}`, 'WORK_ITEM_NOT_FOUND', state.text);
  const page = await openPage(browser, context.office);
  try {
    await page.goto('/organization/responsibilities');
    await expect(page.getByRole('heading', { name: '自分が出した委任' })).toBeVisible();
    await expect(page.getByText(/delegate-01 · 取消済み/u)).toBeVisible();
    await expect(page.getByRole('button', { name: 'この委任を取り消す' })).toHaveCount(0);
    const session: WorkSession = current.office;
    expect(session.responsibilities?.map((value) => value.roleLabel)).toEqual(['事務処理']);
  } finally {
    await page.context().close();
  }
});
