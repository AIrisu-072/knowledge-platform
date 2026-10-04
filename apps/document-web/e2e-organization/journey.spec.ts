import { expect, test } from '@playwright/test';
import type { Claimed, DraftCommand, DraftSaved, HandoffSnapshot, SubmitCommand, Submitted, TaskDetail, TaskPage, WorkCommand, WorkingArtifact } from '../src/api/generated-work/types.gen';
import { assertHidden, assertSessions, captureFinal, get, readRuntimeContext, saveState } from './support';

const text = '【合成データ】営業で参照資料を確認しました。事務担当は提出内容と共有文書を照合してください。';

test('実営業UIの文書往復・非公開保存・確認提出から実事務UIの引受と固定内容受領まで', async ({ page, browser, request }) => {
  const context = readRuntimeContext();
  const sessions = await assertSessions(request, context);
  const salesList = await get<TaskPage>(request, context.sales, '/v1/organization/tasks?view=context');
  expect(salesList.nextCursor).toBeNull();
  expect(salesList.items).toHaveLength(1);
  const source = salesList.items[0]!;
  expect(source).toMatchObject({ state: 'active', canEdit: true, canClaim: false, handoffSnapshotId: null });
  expect(await get(request, context.sales, '/v1/organization/tasks?view=queue')).toEqual(salesList);
  expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });
  const document = await get<{ documentId: string; title: string }>(request, context.sales, `/v1/documents/${context.documentId}?view=published`);
  expect(document.documentId).toBe(context.documentId);
  expect((await get<{ documentId: string }>(request, context.office, `/v1/documents/${context.documentId}?view=published`)).documentId).toBe(context.documentId);

  const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false });
  const office = await officeContext.newPage();
  try {
    await office.goto(`${context.office}/tasks?view=queue`);
    await expect(office.getByText('閲覧できるタスクはありません', { exact: true })).toBeVisible();
    await page.goto('/tasks?view=context');
    await page.getByRole('complementary', { name: 'タスク一覧' }).getByRole('button', { name: new RegExp(source.title) }).click();
    await expect(page).toHaveURL((url) => url.pathname === '/tasks' && url.searchParams.get('taskId') === source.id);
    const editor = page.getByLabel('作業中の文案', { exact: true });
    await expect(editor).toHaveValue('');
    await editor.fill(text);
    await expect(page.getByRole('button', { name: '提出内容を確認', exact: true })).toBeDisabled();
    const input = page.getByRole('link', { name: '共有入力文書', exact: true });
    await expect(input).toHaveAttribute('href', new RegExp(`/documents/${context.documentId}`));
    await input.click();
    await expect(page).toHaveURL((url) => url.pathname === `/documents/${context.documentId}`);
    await expect(page.getByRole('heading', { name: document.title, level: 1 })).toBeVisible();
    await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: 'タスク', exact: true }).click();
    await expect(page).toHaveURL((url) => url.pathname === '/tasks' && url.searchParams.get('taskId') === source.id && url.searchParams.get('view') === 'context');
    await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveValue(text);

    const saveResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/working-artifacts` && response.request().method() === 'POST');
    await page.getByRole('button', { name: '文案を保存', exact: true }).click();
    const savedResponse = await saveResponse;
    expect(savedResponse.status()).toBe(200);
    const saved = await savedResponse.json() as DraftSaved;
    const saveCommand = savedResponse.request().postDataJSON() as DraftCommand;
    expect(saved.kind).toBe('draft_saved');
    expect(saveCommand).toMatchObject({ expectedRevision: source.revision, actingAssignmentId: sessions.sales.actingAssignmentId, value: { text } });
    expect(saved.artifact).toMatchObject({ taskId: source.id, attemptId: source.attemptId, value: { text }, visibility: 'work_item_private' });
    await expect(page.getByText('文案を保存しました', { exact: true })).toBeVisible();
    expect((await get<TaskDetail>(request, context.sales, `/v1/organization/tasks/${source.id}`)).workingArtifacts).toEqual([saved.artifact]);
    expect(await get<WorkingArtifact>(request, context.sales, `/v1/organization/working-artifacts/${saved.artifact.id}`)).toEqual(saved.artifact);
    await office.getByRole('button', { name: '再読込', exact: true }).click();
    await expect(office.getByText('閲覧できるタスクはありません', { exact: true })).toBeVisible();
    expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });
    await assertHidden(request, context.office, `/v1/organization/tasks/${source.id}`, 'WORK_ITEM_NOT_FOUND', text);
    await assertHidden(request, context.office, `/v1/organization/working-artifacts/${saved.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);

    await page.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '提出の確認', exact: true });
    await expect(dialog).toContainText(text);
    await expect(dialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    expect((await get<TaskDetail>(request, context.sales, `/v1/organization/tasks/${source.id}`)).revision).toBe(saved.task.revision);
    expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });

    await page.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const submitResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/submit` && response.request().method() === 'POST');
    await page.getByRole('dialog', { name: '提出の確認', exact: true }).getByRole('button', { name: '提出を確定', exact: true }).click();
    const submittedResponse = await submitResponse;
    expect(submittedResponse.status()).toBe(200);
    const submitted = await submittedResponse.json() as Submitted;
    const submitCommand = submittedResponse.request().postDataJSON() as SubmitCommand;
    expect(submitted.kind).toBe('submitted');
    expect(submitCommand).toMatchObject({ expectedRevision: saved.task.revision, actingAssignmentId: sessions.sales.actingAssignmentId, artifacts: [{ artifactId: saved.artifact.id, revision: saved.artifact.revision }] });
    expect(submitted.task.state).toBe('completed');
    expect(submitted.nextTask.state).toBe('ready');
    expect(submitted.snapshot).toMatchObject({ sourceTaskId: source.id, sourceAttemptId: source.attemptId, targetTaskId: submitted.nextTask.id, submittedBy: 'sales-01', actingAssignmentId: sessions.sales.actingAssignmentId, artifacts: [{ artifactId: saved.artifact.id, revision: saved.artifact.revision, schemaId: saved.artifact.schemaId, value: { text } }] });
    await expect(page.getByText('提出が確定しました', { exact: true })).toBeVisible();
    await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(text);
    await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);

    await office.getByRole('button', { name: '再読込', exact: true }).click();
    await office.getByRole('complementary', { name: 'タスク一覧' }).getByRole('button', { name: new RegExp(submitted.nextTask.title) }).click();
    await expect(office.getByRole('button', { name: '担当を引き受ける', exact: true })).toBeVisible();
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toHaveCount(0);
    await assertHidden(request, context.office, `/v1/organization/tasks/${submitted.nextTask.id}`, 'WORK_ITEM_NOT_FOUND', text);
    await assertHidden(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    const officeReady = await get<TaskPage>(request, context.office, '/v1/organization/tasks?view=queue');
    expect(officeReady.items).toHaveLength(1);
    expect(officeReady.items[0]).toMatchObject({ id: submitted.nextTask.id, state: 'ready', canClaim: true });

    const claimResponse = office.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${submitted.nextTask.id}/claim` && response.request().method() === 'POST');
    await office.getByRole('button', { name: '担当を引き受ける', exact: true }).click();
    const claimedResponse = await claimResponse;
    expect(claimedResponse.status()).toBe(200);
    const claimed = await claimedResponse.json() as Claimed;
    const claimCommand = claimedResponse.request().postDataJSON() as WorkCommand;
    expect(claimed.kind).toBe('claimed');
    expect(claimCommand).toMatchObject({ expectedRevision: officeReady.items[0]!.revision, actingAssignmentId: sessions.office.actingAssignmentId });
    expect(claimed.task).toMatchObject({ id: submitted.nextTask.id, state: 'active', canClaim: false, canEdit: false });
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(text);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    expect(await get<HandoffSnapshot>(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    await assertHidden(request, context.office, `/v1/organization/working-artifacts/${saved.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    await assertHidden(request, context.sales, `/v1/organization/tasks/${submitted.nextTask.id}`, 'WORK_ITEM_NOT_FOUND', text);
    for (const operationId of [saveCommand.operationId, submitCommand.operationId, claimCommand.operationId]) expect(operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    const final = await captureFinal(request, context, source.id, submitted.nextTask.id, submitted.snapshot.id);
    expect(final.snapshot).toEqual(submitted.snapshot);
    expect(final.salesTask.state).toBe('completed');
    expect(final.officeTask).toMatchObject({ state: 'active', workingArtifacts: [] });
    await saveState(context, { schemaVersion: 1, documentId: context.documentId, salesTaskId: source.id, officeTaskId: submitted.nextTask.id, artifactId: saved.artifact.id, snapshotId: submitted.snapshot.id, text, save: { operationId: saveCommand.operationId, result: saved }, submit: { operationId: submitCommand.operationId, result: submitted }, claim: { operationId: claimCommand.operationId, result: claimed }, final });
  } finally {
    await officeContext.close();
  }
});
