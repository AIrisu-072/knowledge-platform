import { expect, test } from '@playwright/test';
import type { Assigned, Claimed, Returned, Submitted, TaskAttention, TaskDetail, TaskPage, WorkContextPage } from '../src/api/generated-work/types.gen';
import { capture, hidden, openPage, read } from './policy-support';
import { contextAction, readContextRuntime, saveContextState } from './context-support';

// Image-free like the existing runtime acceptance.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });

const text = '【合成データ】依頼Cの非公開メモ。提出で固定した内容だけを次工程へ渡す。';
const textB = '【合成データ】案件Bの非公開メモ。審査へ提出する。';
const returnReason = '【合成データ】資金使途の記載を確認してください。';

test('文脈・注意・業務Profileで複数の文脈を実画面で扱い、非開示と確認済みを保つ', async ({ browser, request }) => {
  const context = readContextRuntime();
  contextAction('context-setup');
  const contexts = (await read<WorkContextPage>(request, context.sales, '/v1/organization/work-contexts')).items;
  expect(contexts).toHaveLength(3);
  const contextB = contexts.find((value) => value.title === '合成案件B・運転資金相談')!;
  const contextC = contexts.find((value) => value.title === '合成依頼C・住所変更届')!;
  expect([contextB.kind, contextC.kind]).toEqual(['case', 'request']);
  // Identity is never disclosed to a principal outside the owner unit without an assignment.
  expect((await read<WorkContextPage>(request, context.delegate, '/v1/organization/work-contexts')).items).toEqual([]);
  await hidden(request, context.delegate, `/v1/organization/work-contexts/${contextC.id}`, 'WORK_CONTEXT_NOT_FOUND', contextC.title);
  await hidden(request, context.office, `/v1/organization/work-contexts/${contextB.id}`, 'WORK_CONTEXT_NOT_FOUND', contextB.title);
  const salesTasks = (await read<TaskPage>(request, context.sales, '/v1/organization/tasks?view=context')).items;
  const cSales = salesTasks.find((item) => item.contextId === contextC.id)!;
  const bSales = salesTasks.find((item) => item.contextId === contextB.id)!;
  expect(cSales.attention.map((value) => value.kind)).toEqual(['overdue']);
  expect(bSales.attention.map((value) => value.kind)).toEqual(['due_soon']);

  const sales = await openPage(browser, context.sales);
  const office = await openPage(browser, context.office);
  const approver = await openPage(browser, context.approver);
  const review = await openPage(browser, context.review);
  try {
    // The sales profile opens the context projection without a view in the URL.
    contextAction('context-select');
    await sales.goto('/tasks');
    const groupC = sales.getByRole('region', { name: `文脈 ${contextC.title}`, exact: true });
    await expect(groupC.getByRole('button', { name: /期限超過/u })).toBeVisible();
    await groupC.getByRole('button', { name: new RegExp(contextC.title, 'u') }).click();
    const overview = sales.getByRole('region', { name: '文脈の概要', exact: true });
    await expect(overview.getByRole('list', { name: '工程の進捗' })).toContainText('営業内容整理 · 担当待ち · 試行 1 · 未割当');
    await overview.getByRole('button', { name: '営業内容整理（引受可能）を開く', exact: true }).click();
    contextAction('context-claim');
    const claimed = await capture<Claimed>(sales, 'POST', new RegExp(`/tasks/${cSales.id}/claim$`, 'u'), () => sales.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    expect(claimed.result.task).toMatchObject({ id: cSales.id, state: 'active', contextTitle: contextC.title });
    await expect(sales.getByText(`文脈：${contextC.title}`, { exact: true })).toBeVisible();
    await sales.getByLabel('作業中の文案', { exact: true }).fill(text);
    await capture(sales, 'POST', /\/working-artifacts$/u, () => sales.getByRole('button', { name: '文案を保存', exact: true }).click());
    contextAction('context-submit');
    await sales.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const cSubmitted = await capture<Submitted>(sales, 'POST', /\/submit$/u, () => sales.getByRole('dialog', { name: '提出の確認' }).getByRole('button', { name: '提出を確定', exact: true }).click());
    const cOffice = cSubmitted.result.nextTask;
    // An eligible-only processor row is generic: no customer label.
    const eligible = (await read<TaskPage>(request, context.office, '/v1/organization/tasks?view=queue')).items.find((item) => item.id === cOffice.id)!;
    expect(eligible).toMatchObject({ canClaim: true, contextTitle: null, workTypeLabel: '事務内容確認', handoffSnapshotId: null });

    // A manager assigns the processor explicitly; the assignee sees newly-assigned attention.
    contextAction('context-assign');
    await approver.goto(`/tasks?view=queue&taskId=${cOffice.id}`);
    await approver.getByRole('button', { name: '担当変更の内容を確認', exact: true }).click();
    const dialog = approver.getByRole('dialog', { name: '担当変更の確認' });
    await dialog.getByRole('radio', { name: /^事務担当（模擬） · office-01/u }).check();
    await dialog.getByLabel('担当変更の理由').fill('【合成データ】期限超過の依頼を担当へ割当');
    const officeAssigned = await capture<Assigned>(approver, 'POST', new RegExp(`/tasks/${cOffice.id}/assignment$`, 'u'), () => dialog.getByRole('button', { name: '担当変更を確定', exact: true }).click());
    await expect(approver.getByText('担当変更が確定しました', { exact: true })).toBeVisible();

    contextAction('context-acknowledge');
    await office.goto('/tasks');
    const own = office.getByRole('region', { name: '自分の担当', exact: true });
    await expect(own.getByRole('button', { name: /事務内容確認.*新しい割当/u })).toBeVisible();
    await own.getByRole('button', { name: /事務内容確認.*新しい割当/u }).click();
    await expect(office.getByText(`文脈：${contextC.title}`, { exact: true })).toBeVisible();
    await expect(office.getByRole('list', { name: 'このタスクの注意' })).toContainText('新しい割当');
    const before = await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${cOffice.id}`);
    const acknowledged = await capture<TaskAttention>(office, 'POST', new RegExp(`/tasks/${cOffice.id}/attention-seen$`, 'u'), () => office.getByRole('button', { name: '確認済みにする', exact: true }).click());
    expect(acknowledged.result.items).toEqual([]);
    await expect(office.getByText(/作業は完了していません/u)).toBeVisible();
    await expect(office.getByRole('list', { name: 'このタスクの注意' })).toHaveCount(0);
    const after = await read<TaskDetail>(request, context.office, `/v1/organization/tasks/${cOffice.id}`);
    expect({ revision: after.revision, state: after.state }).toEqual({ revision: before.revision, state: 'active' });
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(text);

    // Context B: due-soon sales work, a review step on the review profile, then a return.
    contextAction('context-review');
    await sales.goto('/tasks');
    const groupB = sales.getByRole('region', { name: `文脈 ${contextB.title}`, exact: true });
    await groupB.getByRole('button', { name: /営業内容整理.*期限間近/u }).click();
    await capture(sales, 'POST', new RegExp(`/tasks/${bSales.id}/claim$`, 'u'), () => sales.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    await sales.getByLabel('作業中の文案', { exact: true }).fill(textB);
    await capture(sales, 'POST', /\/working-artifacts$/u, () => sales.getByRole('button', { name: '文案を保存', exact: true }).click());
    await sales.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const bSubmitted = await capture<Submitted>(sales, 'POST', /\/submit$/u, () => sales.getByRole('dialog', { name: '提出の確認' }).getByRole('button', { name: '提出を確定', exact: true }).click());
    const bReview = bSubmitted.result.nextTask;
    expect(bReview).toMatchObject({ title: '審査内容確認', state: 'ready' });
    expect((await read<TaskPage>(request, context.office, '/v1/organization/tasks?view=queue')).items.some((item) => item.id === bReview.id)).toBe(false);
    await review.goto('/tasks');
    const claimable = review.getByRole('region', { name: '引受可能', exact: true });
    await claimable.getByRole('button', { name: /審査内容確認/u }).click();
    await capture(review, 'POST', new RegExp(`/tasks/${bReview.id}/claim$`, 'u'), () => review.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    await expect(review.getByLabel('文脈モジュール').getByRole('button', { name: '根拠', exact: true })).toHaveAttribute('aria-pressed', 'true');
    await expect(review.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(textB);

    contextAction('context-return');
    await review.getByLabel('差戻理由', { exact: true }).fill(returnReason);
    await review.getByRole('button', { name: '差戻内容を確認', exact: true }).click();
    const bReturned = await capture<Returned>(review, 'POST', new RegExp(`/tasks/${bReview.id}/return$`, 'u'), () => review.getByRole('dialog', { name: '差戻の確認', exact: true }).getByRole('button', { name: '差戻を確定', exact: true }).click());
    expect(bReturned.result.nextTask).toMatchObject({ id: bSales.id, attemptNumber: 2, state: 'ready' });

    contextAction('context-verify');
    await sales.goto(`/tasks?taskId=${bSales.id}`);
    await expect(sales.getByRole('list', { name: 'このタスクの注意' })).toContainText('差戻し');
    const progress = (await read<WorkContextPage>(request, context.sales, '/v1/organization/work-contexts')).items.find((value) => value.id === contextB.id)!.progress!;
    expect(progress.map((step) => [step.stepLabel, step.state, step.attemptNumber])).toEqual([['営業内容整理', 'ready', 2], ['審査内容確認', 'completed', 1]]);
    expect(JSON.stringify(progress)).not.toContain(returnReason);

    await saveContextState(context, {
      schemaVersion: 1, documentId: context.documentId, contextB: contextB.id, contextC: contextC.id, cOfficeTaskId: cOffice.id, bSalesTaskId: bSales.id,
      officeAssigned, acknowledged: acknowledged.result, cSubmitted, bReturned, text,
    });
  } finally {
    for (const page of [sales, office, approver, review]) await page.context().close();
  }
});
