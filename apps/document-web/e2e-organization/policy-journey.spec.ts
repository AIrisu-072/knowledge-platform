import { expect, test } from '@playwright/test';
import type { Assigned, Claimed, Completed, Delegation, DelegationCreated, DelegationRevoked, RoleAssignment, RoleAssignmentCreated, RoleAssignmentRevoked, Submitted, TaskDetail, TaskPage, WorkSession } from '../src/api/generated-work/types.gen';
import { createOperationId } from '../src/application/operation-id';
import { capture, hidden, jstLocal, openPage, policyAction, read, readPolicyContext, savePolicyState, sessions, type PolicyState } from './policy-support';

// Image-free like the existing runtime acceptance.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });

const text = '【合成データ】営業の非公開文案。担当変更後も同じ試行で引き継ぐ。';
const items = (page: TaskPage) => page.items;

test('6名の合成担当で割当・担当変更・期限付き委任・同時引受・権限失効を実UIで確認する', async ({ browser, request }) => {
  const context = readPolicyContext();
  policyAction('policy-setup');
  const initial = await sessions(request, context);
  expect(initial.multiRole.responsibilities).toHaveLength(2);
  expect(initial.delegate.responsibilities).toEqual([]);
  expect(initial.delegate.actingAssignmentId).toBeNull();
  expect(initial.approver.canManageOrganization).toBe(true);
  for (const role of ['sales', 'office', 'review', 'multiRole', 'delegate'] as const) expect(initial[role].canManageOrganization).toBe(false);
  const [source] = items(await read<TaskPage>(request, context.sales, '/v1/organization/tasks?view=context'));
  expect(source).toMatchObject({ state: 'active', canEdit: true, assignment: { principalId: 'sales-01' } });
  expect(items(await read<TaskPage>(request, context.review, '/v1/organization/tasks?view=queue'))).toEqual([]);

  const sales = await openPage(browser, context.sales);
  const approver = await openPage(browser, context.approver);
  const review = await openPage(browser, context.review);
  const office = await openPage(browser, context.office);
  const delegate = await openPage(browser, context.delegate);
  try {
    policyAction('policy-draft');
    await sales.goto(`/tasks?view=context&taskId=${source!.id}`);
    await sales.getByLabel('作業中の文案', { exact: true }).fill(text);
    const saved = await capture<{ artifact: { id: string } }>(sales, 'POST', /\/working-artifacts$/u, () => sales.getByRole('button', { name: '文案を保存', exact: true }).click());
    await expect(sales.getByText('文案を保存しました', { exact: true })).toBeVisible();

    // A current formal management assignment grants a second sales responsibility.
    policyAction('policy-role-assignment');
    await approver.goto('/organization/responsibilities');
    const assignmentForm = approver.getByRole('region', { name: '割当の追加', exact: true });
    await assignmentForm.getByLabel('担当者').selectOption('review-01');
    await assignmentForm.getByLabel('役割@組織').selectOption({ label: '営業@営業店' });
    await assignmentForm.getByLabel('割当の理由').fill('【合成データ】営業応援');
    await assignmentForm.getByRole('button', { name: '割当の内容を確認', exact: true }).click();
    const roleAssignment = await capture<RoleAssignmentCreated>(approver, 'POST', /\/v1\/organization\/role-assignments$/u, () => approver.getByRole('dialog', { name: '割当の追加の確認' }).getByRole('button', { name: '確定', exact: true }).click());
    await expect(approver.getByText('割当の追加が確定しました', { exact: true })).toBeVisible();
    expect(roleAssignment.result.assignment).toMatchObject({ principal: 'review-01', createdBy: 'approver-01', revokedAt: null });
    const reviewSession = await read<WorkSession>(request, context.review, '/v1/organization/session');
    expect(reviewSession.responsibilities?.map((value) => value.roleLabel)).toEqual(['審査', '営業']);

    // The manager sees assignment state only and reassigns through an explicit confirmation.
    policyAction('policy-reassign-sales');
    await approver.goto(`/tasks?view=queue&taskId=${source!.id}`);
    const management = approver.getByRole('region', { name: '担当の管理', exact: true });
    await expect(management).toBeVisible();
    await expect(approver.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    await expect(approver.getByText(text)).toHaveCount(0);
    await management.getByRole('button', { name: '担当変更の内容を確認', exact: true }).click();
    const salesDialog = approver.getByRole('dialog', { name: '担当変更の確認' });
    await salesDialog.getByRole('radio', { name: /review-01/u }).check();
    await salesDialog.getByLabel('担当変更の理由').fill('【合成データ】営業担当の不在');
    const salesAssigned = await capture<Assigned>(approver, 'POST', new RegExp(`/tasks/${source!.id}/assignment$`, 'u'), () => salesDialog.getByRole('button', { name: '担当変更を確定', exact: true }).click());
    await expect(approver.getByText('担当変更が確定しました', { exact: true })).toBeVisible();
    expect(salesAssigned.command).toMatchObject({ assigneePrincipalId: 'review-01', assigneeResponsibilityId: roleAssignment.result.assignment.id, expectedAttemptId: source!.attemptId });

    // Same attempt: the private draft moves to the new assignee; the old one cannot read it.
    policyAction('policy-transfer-verify');
    await hidden(request, context.sales, `/v1/organization/tasks/${source!.id}`, 'WORK_ITEM_NOT_FOUND', text);
    await hidden(request, context.sales, `/v1/organization/working-artifacts/${saved.result.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    const transferred = await read<TaskDetail>(request, context.review, `/v1/organization/tasks/${source!.id}`);
    expect(transferred).toMatchObject({ attemptId: source!.attemptId, assignment: { principalId: 'review-01', actingKind: 'role_assignment' } });
    expect(transferred.workingArtifacts.map((artifact) => artifact.value?.text)).toEqual([text]);
    await sales.reload();
    await expect(sales.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    await review.goto(`/tasks?view=context&taskId=${source!.id}`);
    await expect(review.getByLabel('作業中の文案', { exact: true })).toHaveValue(text);

    policyAction('policy-submit');
    await review.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const submitted = await capture<Submitted>(review, 'POST', /\/submit$/u, () => review.getByRole('dialog', { name: '提出の確認' }).getByRole('button', { name: '提出を確定', exact: true }).click());
    await expect(review.getByText('提出が確定しました', { exact: true })).toBeVisible();
    expect(submitted.result.snapshot).toMatchObject({ submittedBy: 'review-01', actingAssignmentId: roleAssignment.result.assignment.id });
    const officeTaskId = submitted.result.nextTask.id;

    // A holder delegates a narrowed processing responsibility with an exclusive end.
    policyAction('policy-delegation');
    await office.goto('/organization/responsibilities');
    const delegationForm = office.getByRole('region', { name: '委任の作成', exact: true });
    await delegationForm.getByLabel('受任者').selectOption('delegate-01');
    await delegationForm.getByLabel(/期限/u).fill(jstLocal(120));
    await delegationForm.getByLabel('委任の理由').fill('【合成データ】休暇中の代理');
    await delegationForm.getByRole('button', { name: '委任の内容を確認', exact: true }).click();
    const delegation = await capture<DelegationCreated>(office, 'POST', /\/v1\/organization\/delegations$/u, () => office.getByRole('dialog', { name: '委任の確認' }).getByRole('button', { name: '確定', exact: true }).click());
    await expect(office.getByText('委任が確定しました', { exact: true })).toBeVisible();
    expect(delegation.result.delegation).toMatchObject({ delegator: 'office-01', recipient: 'delegate-01', actions: ['queue.read', 'work.read', 'work.claim'] });
    const delegateSession = await read<WorkSession>(request, context.delegate, '/v1/organization/session');
    expect(delegateSession.responsibilities).toMatchObject([{ id: delegation.result.delegation.id, kind: 'delegation', delegator: 'office-01' }]);
    await delegate.goto(`/tasks?view=queue&taskId=${officeTaskId}`);
    await expect(delegate.getByRole('button', { name: '担当を引き受ける', exact: true })).toBeVisible();

    // Two eligible principals claim the same revision concurrently: one assignment.
    policyAction('policy-concurrent-claim');
    const [ready] = items(await read<TaskPage>(request, context.multiRole, '/v1/organization/tasks?view=queue'));
    expect(ready).toMatchObject({ id: officeTaskId, canClaim: true, assignment: null });
    const multiActing = initial.multiRole.responsibilities!.find((value) => value.roleLabel === '事務処理')!.id;
    const claimBody = (acting: string) => ({ operationId: createOperationId(), expectedRevision: ready!.revision, actingAssignmentId: acting });
    const multiCommand = claimBody(multiActing), delegateCommand = claimBody(delegation.result.delegation.id);
    const [multiClaim, delegateClaim] = await Promise.all([
      request.post(`${context.multiRole}/v1/organization/tasks/${officeTaskId}/claim`, { data: multiCommand }),
      request.post(`${context.delegate}/v1/organization/tasks/${officeTaskId}/claim`, { data: delegateCommand }),
    ]);
    expect([multiClaim.status(), delegateClaim.status()].sort()).toEqual([200, 409]);
    const claimWinner = multiClaim.status() === 200 ? 'multiRole' : 'delegate';
    const winnerResponse = claimWinner === 'multiRole' ? multiClaim : delegateClaim;
    const loserResponse = claimWinner === 'multiRole' ? delegateClaim : multiClaim;
    expect(['REVISION_CONFLICT', 'WORK_ASSIGNMENT_CONFLICT']).toContain(((await loserResponse.json()) as { code: string }).code);
    const claim = { operationId: (claimWinner === 'multiRole' ? multiCommand : delegateCommand).operationId, command: claimWinner === 'multiRole' ? multiCommand : delegateCommand, result: await winnerResponse.json() as Claimed };

    policyAction('policy-reassign-office');
    await approver.goto(`/tasks?view=queue&taskId=${officeTaskId}`);
    await expect(approver.getByRole('region', { name: '現在の担当' })).toContainText(claimWinner === 'multiRole' ? 'multi-role-01' : 'delegate-01');
    await approver.getByRole('button', { name: '担当変更の内容を確認', exact: true }).click();
    const officeDialog = approver.getByRole('dialog', { name: '担当変更の確認' });
    await officeDialog.getByRole('radio', { name: /^事務担当（模擬） · office-01/u }).check();
    await officeDialog.getByLabel('担当変更の理由').fill('【合成データ】委任元へ戻す');
    const officeAssigned = await capture<Assigned>(approver, 'POST', new RegExp(`/tasks/${officeTaskId}/assignment$`, 'u'), () => officeDialog.getByRole('button', { name: '担当変更を確定', exact: true }).click());
    await expect(approver.getByText('担当変更が確定しました', { exact: true })).toBeVisible();
    await hidden(request, claimWinner === 'multiRole' ? context.multiRole : context.delegate, `/v1/organization/tasks/${officeTaskId}`, 'WORK_ITEM_NOT_FOUND', text);
    await office.goto(`/tasks?view=queue&taskId=${officeTaskId}`);
    await expect(office.getByRole('region', { name: '受領したスナップショット' })).toContainText(text);

    policyAction('policy-delegation-revoke');
    await office.goto('/organization/responsibilities');
    await office.getByRole('button', { name: 'この委任を取り消す', exact: true }).click();
    const delegationRevoked = await capture<DelegationRevoked>(office, 'POST', /\/delegations\/[^/]+\/revoke$/u, () => office.getByRole('dialog', { name: '委任の取消の確認' }).getByRole('button', { name: '確定', exact: true }).click());
    await expect(office.getByText('委任の取消が確定しました', { exact: true })).toBeVisible();
    expect((await read<WorkSession>(request, context.delegate, '/v1/organization/session')).responsibilities).toEqual([]);
    expect(items(await read<TaskPage>(request, context.delegate, '/v1/organization/tasks?view=queue'))).toEqual([]);

    policyAction('policy-assignment-revoke');
    await approver.goto('/organization/responsibilities');
    const multiRow = approver.getByRole('listitem').filter({ hasText: 'multi-role-01' }).filter({ hasText: '事務処理@事務' });
    await multiRow.getByRole('button', { name: 'この割当を取り消す', exact: true }).click();
    const assignmentRevoked = await capture<RoleAssignmentRevoked>(approver, 'POST', /\/role-assignments\/[^/]+\/revoke$/u, () => approver.getByRole('dialog', { name: '割当の取消の確認' }).getByRole('button', { name: '確定', exact: true }).click());
    await expect(approver.getByText('割当の取消が確定しました', { exact: true })).toBeVisible();
    expect((await read<WorkSession>(request, context.multiRole, '/v1/organization/session')).responsibilities?.map((value) => value.roleLabel)).toEqual(['審査']);

    policyAction('policy-complete');
    await office.goto(`/tasks?view=queue&taskId=${officeTaskId}`);
    await office.getByRole('button', { name: '完了内容を確認', exact: true }).click();
    const completed = await capture<Completed>(office, 'POST', /\/actions$/u, () => office.getByRole('dialog', { name: 'タスク完了の確認' }).getByRole('button', { name: '完了を確定', exact: true }).click());
    await expect(office.getByText('タスクの完了が確定しました', { exact: true })).toBeVisible();

    const state: PolicyState = {
      schemaVersion: 1, documentId: context.documentId, salesTaskId: source!.id, officeTaskId, text,
      roleAssignment, salesAssigned, submitted, delegation, claimWinner, claim, officeAssigned, delegationRevoked, assignmentRevoked, completed,
      final: {
        assignments: (await read<{ items: RoleAssignment[] }>(request, context.approver, '/v1/organization/role-assignments')).items,
        delegations: (await read<{ items: Delegation[] }>(request, context.approver, '/v1/organization/delegations')).items,
        officeTask: await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${officeTaskId}`),
        reviewSalesTask: await read<TaskDetail>(request, context.review, `/v1/organization/tasks/${source!.id}`),
      },
    };
    expect(state.final.officeTask).toMatchObject({ state: 'completed', assignment: { principalId: 'office-01' } });
    await savePolicyState(context, state);
  } finally {
    for (const page of [sales, approver, review, office, delegate]) await page.context().close();
  }
});
