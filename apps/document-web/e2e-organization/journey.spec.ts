import { currentAction } from './support';
import { expect, test, type Page, type APIRequestContext } from '@playwright/test';
import { createHash, randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import type { CreateFolderData, FolderDetail, MutationResult, PublishedDocumentDetail, FileList } from '@knowledge-platform/document-api-client';
import { createSelectedFolderFromUi, replaySelectedFolderCreate, assertFolderPaginationUi, prepareFolderPagination, assertRootFolderCreated, assertRootFolderUi, openRootFolderHome, readRootFolderSnapshot, replayRootFolderCreate, saveRootFolderState, type RootFolderState } from './support';
import type { WorkflowActionCommand, Completed, Claimed, DraftCommand, DraftSaved, HandoffSnapshot, ReturnCommand, Returned, ReturnInstruction, SubmitCommand, Submitted, TaskDetail, TaskPage, WorkCommand, WorkingArtifact } from '../src/api/generated-work/types.gen';
import { holdAndResume, assertHoldResumeState, assertCompletionState, assertHidden, assertSessions, assertEvidenceState, assertAgentState, requestSyntheticFinding, captureFinal, get, publishedEvidenceSource, readRuntimeContext, recordDecision, registerEvidence, registerFinding, revisionRef, saveState } from './support';

// These worker-scoped settings explicitly preserve the existing image-free runtime configuration.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });

const returnReason = '【合成データ】対象数量を追記して再提出してください。';
const revisedText = '【合成データ】対象数量は10件です。営業で参照資料と照合して追記しました。';

const text = '【合成データ】営業で参照資料を確認しました。事務担当は提出内容と共有文書を照合してください。';

// Verify the actual UI Download bytes in Playwright's private temporary directory.
// Text response.body() can re-encode CDP strings. Never use it as an original-byte oracle.
async function assertPublishedOriginal(page: Page, request: APIRequestContext, origin: string, documentId: string, taskId: string) {
  currentAction('source-read');
  const before = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  const document = await get<PublishedDocumentDetail>(request, origin, `/v1/documents/${documentId}?view=published`);
  const files = await get<FileList>(request, origin, `/v1/documents/${documentId}/versions/${document.currentVersionId}/files?purpose=published`);
  expect(files.items).toHaveLength(1);
  const file = files.items[0]!;
  const fixture = Buffer.from('【合成データ】2名の提出確認に使う共有資料です。\n', 'utf8');
  const panel = page.getByRole('region', { name: '公開文書の内容', exact: true });
  await expect(panel).toContainText(`公開改訂 ${document.displayRevision!.label}`);
  await expect(panel).toContainText(document.displayRevision!.revisionId);
  await expect(panel).toContainText(`内容の版（Version） ${document.currentVersionId}`);
  await expect(panel).toContainText(file.displayName);
  await expect(panel).toContainText(`${file.mediaType} · ${file.sizeBytes} bytes`);
  const path = `/v1/documents/${documentId}/versions/${document.currentVersionId}/files/${file.contentItemId}/${file.representationId}`;
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === path && new URL(response.url()).searchParams.get('purpose') === 'published' && response.request().method() === 'GET');
  const downloadPromise = page.waitForEvent('download');
  await panel.getByRole('button', { name: `原本を取得 ${file.displayName}`, exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const download = await downloadPromise;
  try {
    expect(await download.failure()).toBeNull();
    const bytes = await readFile((await download.path())!);
    expect(bytes.byteLength).toBe(file.sizeBytes);
    expect(bytes.byteLength).toBe(fixture.byteLength);
    expect(createHash('sha256').update(bytes).digest('hex')).toBe(createHash('sha256').update(fixture).digest('hex'));
    expect(download.suggestedFilename()).toBe(file.displayName);
  } finally {
    await download.delete();
  }
  expect(await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(before);
}

test('実2名UIで根拠・候補・3種の人間判断を選択提出し、差戻後の新試行を非公開で再提出する', async ({ page, browser, request }) => {
  const context = readRuntimeContext();
  currentAction('journey-setup');
  const sessions = await assertSessions(request, context);
  const salesList = await get<TaskPage>(request, context.sales, '/v1/organization/tasks?view=context');
  expect(salesList.nextCursor).toBeNull();
  expect(salesList.items).toHaveLength(1);
  const source = salesList.items[0]!;
  expect(source).toMatchObject({ state: 'active', attemptNumber: 1, canEdit: true, canClaim: false, handoffSnapshotId: null });
  expect(await get(request, context.sales, '/v1/organization/tasks?view=queue')).toEqual(salesList);
  expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });
  const document = await get<{ documentId: string; title: string }>(request, context.sales, `/v1/documents/${context.documentId}?view=published`);
  expect(document.documentId).toBe(context.documentId);
  expect((await get<{ documentId: string }>(request, context.office, `/v1/documents/${context.documentId}?view=published`)).documentId).toBe(context.documentId);

  const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: true });
  const office = await officeContext.newPage();
  try {
    currentAction('office-navigation');
    await office.goto(`${context.office}/tasks?view=queue`);
    await expect(office.getByText('閲覧できるタスクはありません', { exact: true })).toBeVisible();
    currentAction('sales-navigation');
    await page.goto('/tasks?view=context');
    await page.getByRole('complementary', { name: 'タスク一覧' }).getByRole('button', { name: new RegExp(source.title) }).click();
    await expect(page).toHaveURL((url) => url.pathname === '/tasks' && url.searchParams.get('taskId') === source.id);
    const editor = page.getByLabel('作業中の文案', { exact: true });
    await expect(editor).toHaveValue('');
    await editor.fill(text);
    await expect(page.getByRole('button', { name: '提出内容を確認', exact: true })).toBeDisabled();
    await assertPublishedOriginal(page, request, context.sales, context.documentId, source.id);
    await expect(editor).toHaveValue(text);
    const input = page.getByRole('link', { name: '共有入力文書', exact: true });
    await expect(input).toHaveAttribute('href', new RegExp(`/documents/${context.documentId}`));
    currentAction('document-navigation');
    await input.click();
    await expect(page).toHaveURL((url) => url.pathname === `/documents/${context.documentId}`);
    await expect(page.getByRole('heading', { name: document.title, level: 1 })).toBeVisible();
    await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '編集作業', exact: true }).click();
    await expect(page).toHaveURL((url) => url.pathname === '/documents' && url.searchParams.get('view') === 'authoring');
    await expect(page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '編集作業', exact: true })).toHaveAttribute('aria-current', 'page');
    await page.getByRole('button', { name: new RegExp(document.title) }).click();
    await page.getByRole('button', { name: '詳細を開く', exact: true }).click();
    await expect(page).toHaveURL((url) => url.pathname === `/documents/${context.documentId}` && url.searchParams.get('view') === 'authoring');
    await page.getByRole('tab', { name: '版・改訂', exact: true }).click();
    await expect(page.getByRole('button', { name: '新しい版を作成', exact: true }).first()).toBeEnabled();
    currentAction('task-navigation');
    await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: 'タスク', exact: true }).click();
    await expect(page).toHaveURL((url) => url.pathname === '/tasks' && url.searchParams.get('taskId') === source.id && url.searchParams.get('view') === 'context');
    await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveValue(text);

    const saveResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/working-artifacts` && response.request().method() === 'POST');
    currentAction('draft-save');
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

    // Select real published source identities rather than inventing locator or revision IDs.
    currentAction('source-read');
    const evidenceSource = await publishedEvidenceSource(request, context.sales, context.documentId);
    currentAction('evidence-module');
    await page.getByRole('button', { name: '根拠', exact: true }).click();
    const selectedEvidence = await registerEvidence(page, source.id, evidenceSource, '【合成データ】共有候補の該当箇所。人間による記載で未検証。');
    const privateEvidence = await registerEvidence(page, source.id, evidenceSource, '【合成データ】未選択の根拠。事務への共有対象外。');
    const sharedFinding = await registerFinding(page, source.id, selectedEvidence.result.evidence, '【合成データ】共有する候補の主張。原本の事実と区別する。');
    const privateFinding = await registerFinding(page, source.id, privateEvidence.result.evidence, '【合成データ】未選択の候補。提出しても事務には見せない。');
    const modified = await recordDecision(page, source.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'modified', '【合成データ】営業が候補の表現を修正した。', '【合成データ】営業が修正した採用文。', true);
    const accepted = await recordDecision(page, source.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'accepted', '【合成データ】営業が候補を採用した。');
    const rejected = await recordDecision(page, source.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'rejected', '【合成データ】営業が候補を却下した。元候補は保持する。');
    const privateDecision = await recordDecision(page, source.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'accepted', '【合成データ】共有候補に対する未選択の判断。判断の共有対象外。');
    const sharedDecisions = [modified, accepted, rejected];
    expect(new Set(sharedDecisions.map((receipt) => receipt.result.decision.id)).size).toBe(3);
    for (const receipt of [...sharedDecisions, privateDecision]) expect(receipt.result.decision).toMatchObject({ humanPrincipal: 'sales-01', actingAssignmentId: sessions.sales.actingAssignmentId, attemptId: source.attemptId });
    expect(await get(request, context.sales, `/v1/organization/evidence/${selectedEvidence.result.evidence.id}`)).toEqual(selectedEvidence.result.evidence);
    expect(await get(request, context.sales, `/v1/organization/findings/${sharedFinding.result.finding.id}`)).toEqual(sharedFinding.result.finding);
    expect(await get(request, context.sales, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`)).toEqual({ items: [...sharedDecisions.map((receipt) => receipt.result.decision), privateDecision.result.decision], nextCursor: null });
    expect(await get(request, context.sales, `/v1/organization/tasks/${source.id}/evidence`)).toEqual({ items: [selectedEvidence.result.evidence, privateEvidence.result.evidence], nextCursor: null });
    expect(await get(request, context.sales, `/v1/organization/tasks/${source.id}/findings`)).toEqual({ items: [sharedFinding.result.finding, privateFinding.result.finding], nextCursor: null });
    for (const collection of ['evidence', 'findings']) await assertHidden(request, context.office, `/v1/organization/tasks/${source.id}/${collection}`, 'WORK_ITEM_NOT_FOUND');
    for (const receipt of [selectedEvidence, privateEvidence]) await assertHidden(request, context.office, `/v1/organization/evidence/${receipt.result.evidence.id}`, 'EVIDENCE_NOT_FOUND', receipt.result.evidence.relevantLocation);
    for (const receipt of [sharedFinding, privateFinding]) {
      await assertHidden(request, context.office, `/v1/organization/findings/${receipt.result.finding.id}`, 'FINDING_NOT_FOUND', receipt.result.finding.claim);
      await assertHidden(request, context.office, `/v1/organization/findings/${receipt.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
    }
    for (const receipt of [selectedEvidence, privateEvidence, sharedFinding, privateFinding, ...sharedDecisions, privateDecision]) {
      expect(await get(request, context.sales, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
      await assertHidden(request, context.office, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND');
    }
    // A judgment remains a separate record and must never automatically submit or return.
    expect((await get<TaskDetail>(request, context.sales, `/v1/organization/tasks/${source.id}`)).state).toBe('active');
    expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });

    currentAction('submit-preview');
    await page.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '提出の確認', exact: true });
    await expect(dialog).toContainText(text);
    await expect(dialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    expect((await get<TaskDetail>(request, context.sales, `/v1/organization/tasks/${source.id}`)).revision).toBe(privateDecision.result.task.revision);
    expect(await get(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [], nextCursor: null });

    currentAction('submit-preview');
    await page.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const submitDialog = page.getByRole('dialog', { name: '提出の確認', exact: true });
    currentAction('submit-selection');
    for (const kind of ['根拠', '候補', '判断']) {
      for (const checkbox of await submitDialog.getByRole('checkbox', { name: new RegExp(`^共有する${kind} `) }).all()) await expect(checkbox).not.toBeChecked();
    }
    await submitDialog.getByLabel(`共有する判断 ${modified.result.decision.id}`, { exact: true }).check();
    await expect(submitDialog.getByRole('button', { name: '提出を確定', exact: true })).toBeDisabled();
    await submitDialog.getByLabel(`共有する候補 ${sharedFinding.result.finding.id}`, { exact: true }).check();
    await expect(submitDialog.getByRole('button', { name: '提出を確定', exact: true })).toBeDisabled();
    await submitDialog.getByLabel(`共有する根拠 ${selectedEvidence.result.evidence.id}`, { exact: true }).check();
    for (const receipt of [accepted, rejected]) await submitDialog.getByLabel(`共有する判断 ${receipt.result.decision.id}`, { exact: true }).check();
    await expect(submitDialog.getByLabel(`共有する根拠 ${privateEvidence.result.evidence.id}`, { exact: true })).not.toBeChecked();
    await expect(submitDialog.getByLabel(`共有する候補 ${privateFinding.result.finding.id}`, { exact: true })).not.toBeChecked();
    await expect(submitDialog.getByLabel(`共有する判断 ${privateDecision.result.decision.id}`, { exact: true })).not.toBeChecked();
    await expect(submitDialog.getByRole('button', { name: '提出を確定', exact: true })).toBeEnabled();
    const submitResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/submit` && response.request().method() === 'POST');
    currentAction('submit-confirm');
    await page.getByRole('dialog', { name: '提出の確認', exact: true }).getByRole('button', { name: '提出を確定', exact: true }).click();
    const submittedResponse = await submitResponse;
    expect(submittedResponse.status()).toBe(200);
    const submitted = await submittedResponse.json() as Submitted;
    const submitCommand = submittedResponse.request().postDataJSON() as SubmitCommand;
    expect(submitted.kind).toBe('submitted');
    expect(submitCommand).toMatchObject({ expectedRevision: privateDecision.result.task.revision, expectedAttemptId: source.attemptId, actingAssignmentId: sessions.sales.actingAssignmentId, evidenceRevisionRefs: [revisionRef(selectedEvidence.result.evidence)], findingRevisionRefs: [revisionRef(sharedFinding.result.finding)], decisionRevisionRefs: sharedDecisions.map((receipt) => revisionRef(receipt.result.decision)), artifacts: [{ artifactId: saved.artifact.id, revision: saved.artifact.revision }] });
    expect(submitted.snapshot).toMatchObject({ evidenceRevisionRefs: submitCommand.evidenceRevisionRefs, findingRevisionRefs: submitCommand.findingRevisionRefs, decisionRevisionRefs: submitCommand.decisionRevisionRefs });
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
    await assertHidden(request, context.office, `/v1/organization/evidence/${selectedEvidence.result.evidence.id}`, 'EVIDENCE_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}`, 'FINDING_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
    const officeReady = await get<TaskPage>(request, context.office, '/v1/organization/tasks?view=queue');
    expect(officeReady.items).toHaveLength(1);
    expect(officeReady.items[0]).toMatchObject({ id: submitted.nextTask.id, state: 'ready', canClaim: true });

    const claimResponse = office.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${submitted.nextTask.id}/claim` && response.request().method() === 'POST');
    currentAction('office-claim');
    await office.getByRole('button', { name: '担当を引き受ける', exact: true }).click();
    const claimedResponse = await claimResponse;
    expect(claimedResponse.status()).toBe(200);
    const claimed = await claimedResponse.json() as Claimed;
    const claimCommand = claimedResponse.request().postDataJSON() as WorkCommand;
    expect(claimed.kind).toBe('claimed');
    expect(claimCommand).toMatchObject({ expectedRevision: officeReady.items[0]!.revision, actingAssignmentId: sessions.office.actingAssignmentId });
    expect(claimed.task).toMatchObject({ id: submitted.nextTask.id, state: 'active', canClaim: false, canEdit: false });
    await assertPublishedOriginal(office, request, context.office, context.documentId, claimed.task.id);
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(text);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    expect(await get<HandoffSnapshot>(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    await assertHidden(request, context.office, `/v1/organization/working-artifacts/${saved.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    await assertHidden(request, context.sales, `/v1/organization/tasks/${submitted.nextTask.id}`, 'WORK_ITEM_NOT_FOUND', text);
    for (const operationId of [saveCommand.operationId, submitCommand.operationId, claimCommand.operationId]) expect(operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    currentAction('evidence-module');
    await office.getByRole('button', { name: '根拠', exact: true }).click();
    await expect(office.getByRole('region', { name: `候補 ${sharedFinding.result.finding.id}`, exact: true })).toContainText(sharedFinding.result.finding.claim);
    await expect(office.getByText(privateFinding.result.finding.claim, { exact: true })).toHaveCount(0);
    await expect(office.getByText(privateDecision.result.decision.reason!, { exact: false })).toHaveCount(0);
    expect(await get(request, context.office, `/v1/organization/tasks/${claimed.task.id}/evidence`)).toEqual({ items: [selectedEvidence.result.evidence], nextCursor: null });
    expect(await get(request, context.office, `/v1/organization/tasks/${claimed.task.id}/findings`)).toEqual({ items: [sharedFinding.result.finding], nextCursor: null });
    expect(await get(request, context.office, `/v1/organization/evidence/${selectedEvidence.result.evidence.id}`)).toEqual(selectedEvidence.result.evidence);
    expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}`)).toEqual(sharedFinding.result.finding);
    expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`)).toEqual({ items: sharedDecisions.map((receipt) => receipt.result.decision), nextCursor: null });
    await assertHidden(request, context.office, `/v1/organization/evidence/${privateEvidence.result.evidence.id}`, 'EVIDENCE_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.result.finding.id}`, 'FINDING_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
    const officeDecision = await recordDecision(office, claimed.task.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'modified', '【合成データ】事務が独立して判断した。営業の判断は変更しない。', '【合成データ】事務が修正した採用文。');
    expect(officeDecision.result.decision).toMatchObject({ humanPrincipal: 'office-01', actingAssignmentId: sessions.office.actingAssignmentId, attemptId: claimed.task.attemptId });
    expect(officeDecision.command.expectedAttemptId).not.toBe(sharedFinding.result.finding.attemptId);
    expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`)).toEqual({ items: [...sharedDecisions.map((receipt) => receipt.result.decision), officeDecision.result.decision], nextCursor: null });
    expect(await get(request, context.sales, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`)).toEqual({ items: [...sharedDecisions.map((receipt) => receipt.result.decision), privateDecision.result.decision], nextCursor: null });
    await assertHidden(request, context.sales, `/v1/organization/operations/${officeDecision.operationId}`, 'WORK_ITEM_NOT_FOUND');
    expect(await get(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    // Extend this same real two-principal journey with one return/re-submit cycle.
    // No route interception, fixture response, screenshots, or second harness.
    const returnInput = office.getByLabel('差戻理由', { exact: true });
    await expect(returnInput).toHaveValue('');
    await returnInput.fill(returnReason);
    let returnPosts = 0;
    office.on('request', (outgoing) => { if (new URL(outgoing.url()).pathname === `/v1/organization/tasks/${claimed.task.id}/return` && outgoing.method() === 'POST') returnPosts += 1; });
    currentAction('return-preview');
    await office.getByRole('button', { name: '差戻内容を確認', exact: true }).click();
    const returnDialog = office.getByRole('dialog', { name: '差戻の確認', exact: true });
    await expect(returnDialog).toContainText(returnReason);
    await expect(returnDialog).toContainText(text);
    await expect(returnDialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await returnDialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(returnInput).toHaveValue(returnReason);
    currentAction('return-preview');
    await office.getByRole('button', { name: '差戻内容を確認', exact: true }).click();
    await office.keyboard.press('Escape');
    await expect(returnDialog).not.toBeVisible();
    await expect(returnInput).toHaveValue(returnReason);
    expect(returnPosts).toBe(0);
    expect((await get<TaskDetail>(request, context.office, `/v1/organization/tasks/${claimed.task.id}`)).revision).toBe(officeDecision.result.task.revision);

    currentAction('return-preview');
    await office.getByRole('button', { name: '差戻内容を確認', exact: true }).click();
    const returnResponse = office.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${claimed.task.id}/return` && response.request().method() === 'POST');
    currentAction('return-confirm');
    await returnDialog.getByRole('button', { name: '差戻を確定', exact: true }).click();
    const returnedResponse = await returnResponse;
    expect(returnedResponse.status()).toBe(200);
    const returned = await returnedResponse.json() as Returned;
    const returnCommand = returnedResponse.request().postDataJSON() as ReturnCommand;
    expect(returnPosts).toBe(1);
    expect(returnCommand).toMatchObject({ expectedRevision: officeDecision.result.task.revision, expectedAttemptId: claimed.task.attemptId, actingAssignmentId: sessions.office.actingAssignmentId, ...claimed.task.returnTransition!, reason: returnReason });
    expect(returned.task).toMatchObject({ id: claimed.task.id, attemptId: claimed.task.attemptId, state: 'completed', canEdit: false, canReturn: false });
    expect(returned.nextTask).toMatchObject({ id: source.id, attemptNumber: 2, state: 'ready', canClaim: false, canEdit: false, canSubmit: false });
    expect(returned.nextTask.attemptId).not.toBe(source.attemptId);
    expect(returned.returnInstruction).toMatchObject({ sourceTaskId: claimed.task.id, sourceAttemptId: claimed.task.attemptId, targetTaskId: source.id, targetAttemptId: returned.nextTask.attemptId, previousSubmissionId: submitted.snapshot.id, reason: returnReason });
    await expect(office.getByText('差戻が確定しました', { exact: true })).toBeVisible();
    await expect(office).toHaveURL((url) => url.searchParams.get('taskId') === claimed.task.id);
    await expect(office.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(returnReason);
    await expect(office.getByLabel('差戻理由', { exact: true })).toHaveCount(0);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    expect(await get(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    const replay = await request.post(`${context.office}/v1/organization/tasks/${claimed.task.id}/return`, { data: returnCommand });
    expect(replay.status()).toBe(200);
    expect(await replay.json()).toEqual(returned);
    expect(await get(request, context.office, `/v1/organization/operations/${officeDecision.operationId}`)).toEqual(officeDecision.result);
    expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}/decisions`)).toEqual({ items: [...sharedDecisions.map((receipt) => receipt.result.decision), officeDecision.result.decision], nextCursor: null });
    const salesReady = await get<TaskPage>(request, context.sales, '/v1/organization/tasks?view=queue');
    expect(salesReady.items).toHaveLength(1);
    expect(salesReady.items[0]).toMatchObject({ id: source.id, attemptId: returned.nextTask.attemptId, attemptNumber: 2, state: 'ready', canClaim: true });

    await page.getByRole('button', { name: '再読込', exact: true }).click();
    await expect(page).toHaveURL((url) => url.searchParams.get('taskId') === source.id);
    await expect(page.getByRole('button', { name: '担当を引き受ける', exact: true })).toBeVisible();
    await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    const salesClaimResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/claim` && response.request().method() === 'POST');
    currentAction('sales-reclaim');
    await page.getByRole('button', { name: '担当を引き受ける', exact: true }).click();
    const salesClaimedResponse = await salesClaimResponse;
    expect(salesClaimedResponse.status()).toBe(200);
    const salesClaimed = await salesClaimedResponse.json() as Claimed;
    const salesClaimCommand = salesClaimedResponse.request().postDataJSON() as WorkCommand;
    expect(salesClaimed.task).toMatchObject({ id: source.id, attemptId: returned.nextTask.attemptId, attemptNumber: 2, state: 'active', canEdit: true });
    await expect(page.getByLabel('作業中の文案', { exact: true })).toHaveValue('');
    await expect(page.getByRole('region', { name: '確定した差戻指示', exact: true })).toContainText(returnReason);
    await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(text);
    expect((await get<TaskDetail>(request, context.sales, `/v1/organization/tasks/${source.id}`)).workingArtifacts).toEqual([]);
    await assertHidden(request, context.sales, `/v1/organization/working-artifacts/${saved.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    await assertHidden(request, context.sales, `/v1/organization/operations/${saveCommand.operationId}`, 'WORK_ARTIFACT_NOT_FOUND', text);
    currentAction('evidence-module');
    await page.getByRole('button', { name: '根拠', exact: true }).click();
    expect(await get(request, context.sales, `/v1/organization/tasks/${source.id}/evidence`)).toEqual({ items: [selectedEvidence.result.evidence], nextCursor: null });
    expect(await get(request, context.sales, `/v1/organization/tasks/${source.id}/findings`)).toEqual({ items: [sharedFinding.result.finding], nextCursor: null });
    await assertHidden(request, context.sales, `/v1/organization/evidence/${privateEvidence.result.evidence.id}`, 'EVIDENCE_NOT_FOUND');
    await assertHidden(request, context.sales, `/v1/organization/findings/${privateFinding.result.finding.id}`, 'FINDING_NOT_FOUND');
    const reworkEvidence = await registerEvidence(page, source.id, evidenceSource, '【合成データ】試行2の非公開根拠。以前の共有根拠を変更しない。');
    const reworkFinding = await registerFinding(page, source.id, reworkEvidence.result.evidence, '【合成データ】試行2の非公開候補。事務に共有しない。');
    const reworkDecision = await recordDecision(page, source.id, reworkFinding.result.finding, reworkEvidence.result.evidence, 'accepted', '【合成データ】試行2の非公開判断。');
    expect(reworkEvidence.result.evidence.attemptId).toBe(returned.nextTask.attemptId);
    expect(reworkEvidence.result.evidence.id).not.toBe(selectedEvidence.result.evidence.id);
    expect(reworkFinding.result.finding.attemptId).toBe(returned.nextTask.attemptId);
    expect(reworkDecision.result.decision.attemptId).toBe(returned.nextTask.attemptId);
    await assertHidden(request, context.office, `/v1/organization/evidence/${reworkEvidence.result.evidence.id}`, 'EVIDENCE_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${reworkFinding.result.finding.id}`, 'FINDING_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${reworkFinding.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
    for (const receipt of [reworkEvidence, reworkFinding, reworkDecision]) await assertHidden(request, context.office, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND');
    expect(await get(request, context.office, `/v1/organization/evidence/${selectedEvidence.result.evidence.id}`)).toEqual(selectedEvidence.result.evidence);
    expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.result.finding.id}`)).toEqual(sharedFinding.result.finding);
    const salesAgent = await requestSyntheticFinding(page, request, context.sales, source.id, selectedEvidence.result.evidence, 'sales-01');
    const salesAgentDecision = await recordDecision(page, source.id, salesAgent.finding, selectedEvidence.result.evidence, 'modified', '【合成データ】合成候補を営業が確認し修正した。本文は別途確認が必要。', '【合成データ】営業が人間判断で修正した合成候補の採用文。');
    expect(salesAgentDecision.result.decision).toMatchObject({ humanPrincipal: 'sales-01', actingAssignmentId: sessions.sales.actingAssignmentId, attemptId: returned.nextTask.attemptId });
    await assertHidden(request, context.office, `/v1/organization/agent-executions/${salesAgent.execution.id}`, 'WORK_ITEM_NOT_FOUND', salesAgent.command.purpose);
    await assertHidden(request, context.office, `/v1/organization/agent-executions/${salesAgent.execution.id}/result`, 'WORK_ITEM_NOT_FOUND');
    await assertHidden(request, context.office, `/v1/organization/findings/${salesAgent.finding.id}`, 'FINDING_NOT_FOUND', salesAgent.finding.claim);
    await assertHidden(request, context.office, `/v1/organization/findings/${salesAgent.finding.id}/decisions`, 'FINDING_NOT_FOUND');
    await page.getByLabel('作業中の文案', { exact: true }).fill(revisedText);
    const resaveResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/working-artifacts` && response.request().method() === 'POST');
    currentAction('draft-save');
    await page.getByRole('button', { name: '文案を保存', exact: true }).click();
    const resavedResponse = await resaveResponse;
    expect(resavedResponse.status()).toBe(200);
    const resaved = await resavedResponse.json() as DraftSaved;
    const resaveCommand = resavedResponse.request().postDataJSON() as DraftCommand;
    expect(resaved.artifact).toMatchObject({ taskId: source.id, attemptId: returned.nextTask.attemptId, value: { text: revisedText }, visibility: 'work_item_private' });
    expect(resaved.artifact.id).not.toBe(saved.artifact.id);
    await expect(page.getByText('文案を保存しました', { exact: true })).toBeVisible();
    await office.getByRole('button', { name: '再読込', exact: true }).click();
    expect(await get<TaskPage>(request, context.office, '/v1/organization/tasks?view=queue')).toEqual({ items: [returned.task], nextCursor: null });
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    await expect(office.getByText(revisedText, { exact: true })).toHaveCount(0);
    await assertHidden(request, context.office, `/v1/organization/tasks/${source.id}`, 'WORK_ITEM_NOT_FOUND', revisedText);
    await assertHidden(request, context.office, `/v1/organization/working-artifacts/${resaved.artifact.id}`, 'WORK_ARTIFACT_NOT_FOUND', revisedText);
    await assertHidden(request, context.office, `/v1/organization/operations/${resaveCommand.operationId}`, 'WORK_ITEM_NOT_FOUND', revisedText);
    const privateList = await request.get(`${context.office}/v1/organization/tasks/${source.id}/working-artifacts`);
    expect(privateList.status()).toBe(404);
    expect(await privateList.text()).not.toContain(revisedText);
    expect(await get(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    expect(await get<ReturnInstruction>(request, context.office, `/v1/organization/return-instructions/${returned.returnInstruction.id}`)).toEqual(returned.returnInstruction);
    const salesHoldResume = await holdAndResume(page, request, context.sales, context.office, source.id, sessions.sales, { label: '作業中の文案', text: '【合成データ】タブ内だけの未保存編集。保留は保存しない。' });
    await expect(page.getByRole('button', { name: '提出内容を確認', exact: true })).toBeDisabled();
    await page.getByLabel('作業中の文案', { exact: true }).fill(revisedText);

    currentAction('submit-preview');
    await page.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    await expect(page.getByRole('dialog', { name: '提出の確認', exact: true })).toContainText(revisedText);
    for (const checkbox of await page.getByRole('dialog', { name: '提出の確認', exact: true }).getByRole('checkbox', { name: /^共有する/ }).all()) await expect(checkbox).not.toBeChecked();
    const resubmitDialog = page.getByRole('dialog', { name: '提出の確認', exact: true });
    // Zero evidence selection is allowed, but only explicit checks share these old revisions again.
    await expect(resubmitDialog.getByRole('button', { name: '提出を確定', exact: true })).toBeEnabled();
    currentAction('submit-selection');
    await resubmitDialog.getByLabel(`共有する根拠 ${selectedEvidence.result.evidence.id}`, { exact: true }).check();
    await resubmitDialog.getByLabel(`共有する候補 ${sharedFinding.result.finding.id}`, { exact: true }).check();
    for (const receipt of sharedDecisions) await resubmitDialog.getByLabel(`共有する判断 ${receipt.result.decision.id}`, { exact: true }).check();
    await resubmitDialog.getByLabel(`共有する候補 ${salesAgent.finding.id}`, { exact: true }).check();
    await resubmitDialog.getByLabel(`共有する判断 ${salesAgentDecision.result.decision.id}`, { exact: true }).check();
    await expect(resubmitDialog.getByLabel(`共有する根拠 ${reworkEvidence.result.evidence.id}`, { exact: true })).not.toBeChecked();
    await expect(resubmitDialog.getByLabel(`共有する候補 ${reworkFinding.result.finding.id}`, { exact: true })).not.toBeChecked();
    await expect(resubmitDialog.getByLabel(`共有する判断 ${reworkDecision.result.decision.id}`, { exact: true })).not.toBeChecked();
    const resubmitResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${source.id}/submit` && response.request().method() === 'POST');
    currentAction('resubmit');
    await page.getByRole('button', { name: '提出を確定', exact: true }).click();
    const resubmittedResponse = await resubmitResponse;
    expect(resubmittedResponse.status()).toBe(200);
    const resubmitted = await resubmittedResponse.json() as Submitted;
    const resubmitCommand = resubmittedResponse.request().postDataJSON() as SubmitCommand;
    expect(resubmitCommand).toMatchObject({ expectedRevision: salesHoldResume.resume.result.task.revision, expectedAttemptId: returned.nextTask.attemptId, evidenceRevisionRefs: submitCommand.evidenceRevisionRefs, findingRevisionRefs: [...submitted.snapshot.findingRevisionRefs, revisionRef(salesAgent.finding)], decisionRevisionRefs: [...submitted.snapshot.decisionRevisionRefs, revisionRef(salesAgentDecision.result.decision)] });
    expect(resubmitted.snapshot).toMatchObject({ evidenceRevisionRefs: submitCommand.evidenceRevisionRefs, findingRevisionRefs: [...submitted.snapshot.findingRevisionRefs, revisionRef(salesAgent.finding)], decisionRevisionRefs: [...submitted.snapshot.decisionRevisionRefs, revisionRef(salesAgentDecision.result.decision)] });
    expect(resubmitted.task).toMatchObject({ id: source.id, attemptNumber: 2, state: 'completed' });
    expect(resubmitted.nextTask).toMatchObject({ id: claimed.task.id, attemptNumber: 2, state: 'ready', canClaim: false });
    expect(resubmitted.nextTask.attemptId).not.toBe(claimed.task.attemptId);
    expect(resubmitted.snapshot.id).not.toBe(submitted.snapshot.id);
    expect(resubmitted.snapshot).toMatchObject({ sourceTaskId: source.id, sourceAttemptId: returned.nextTask.attemptId, previousSubmissionId: submitted.snapshot.id, returnInstructionId: returned.returnInstruction.id, artifacts: [{ artifactId: resaved.artifact.id, revision: resaved.artifact.revision, schemaId: resaved.artifact.schemaId, value: { text: revisedText } }] });
    await expect(page.getByText('提出が確定しました', { exact: true })).toBeVisible();
    await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(revisedText);
    await expect(page.getByRole('region', { name: '差戻前のスナップショット', exact: true })).toContainText(text);
    await office.getByRole('button', { name: '再読込', exact: true }).click();
    await expect(office.getByRole('button', { name: '担当を引き受ける', exact: true })).toBeVisible();
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toHaveCount(0);
    await assertHidden(request, context.office, `/v1/organization/handoff-snapshots/${resubmitted.snapshot.id}`, 'WORK_ARTIFACT_NOT_FOUND', revisedText);
    const officeReclaimResponse = office.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${claimed.task.id}/claim` && response.request().method() === 'POST');
    currentAction('office-reclaim');
    await office.getByRole('button', { name: '担当を引き受ける', exact: true }).click();
    const officeReclaimedResponse = await officeReclaimResponse;
    expect(officeReclaimedResponse.status()).toBe(200);
    const officeReclaimed = await officeReclaimedResponse.json() as Claimed;
    const officeReclaimCommand = officeReclaimedResponse.request().postDataJSON() as WorkCommand;
    expect(officeReclaimed.task).toMatchObject({ id: claimed.task.id, attemptNumber: 2, attemptId: resubmitted.nextTask.attemptId, state: 'active', canEdit: false });
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(revisedText);
    await expect(office.getByLabel('作業中の文案', { exact: true })).toHaveCount(0);
    expect(await get(request, context.office, `/v1/organization/handoff-snapshots/${submitted.snapshot.id}`)).toEqual(submitted.snapshot);
    expect(await get(request, context.office, `/v1/organization/return-instructions/${returned.returnInstruction.id}`)).toEqual(returned.returnInstruction);
    currentAction('evidence-module');
    await office.getByRole('button', { name: '根拠', exact: true }).click();
    const officeReworkDecision = await recordDecision(office, officeReclaimed.task.id, sharedFinding.result.finding, selectedEvidence.result.evidence, 'accepted', '【合成データ】事務が試行2で受領候補を独立して採用した。');
    expect(officeReworkDecision.result.decision).toMatchObject({ humanPrincipal: 'office-01', actingAssignmentId: sessions.office.actingAssignmentId, attemptId: officeReclaimed.task.attemptId });
    expect(officeReworkDecision.result.decision.id).not.toBe(officeDecision.result.decision.id);
    const officeAgent = await requestSyntheticFinding(office, request, context.office, officeReclaimed.task.id, selectedEvidence.result.evidence, 'office-01');
    const officeAgentDecision = await recordDecision(office, officeReclaimed.task.id, officeAgent.finding, selectedEvidence.result.evidence, 'rejected', '【合成データ】事務が合成候補を却下した。元候補を保持し、営業へ自動共有しない。');
    expect(officeAgentDecision.result.decision).toMatchObject({ humanPrincipal: 'office-01', actingAssignmentId: sessions.office.actingAssignmentId, attemptId: officeReclaimed.task.attemptId });
    const agents = { sales: { ...salesAgent, decision: salesAgentDecision }, office: { ...officeAgent, decision: officeAgentDecision } };
    await assertAgentState(request, context, agents);
    const evidence = { selected: selectedEvidence, unselected: privateEvidence, finding: sharedFinding, privateFinding, decisions: sharedDecisions, privateDecision, officeDecision, officeReworkDecision, rework: { evidence: reworkEvidence, finding: reworkFinding, decision: reworkDecision } };
    currentAction('final-verify');
    await assertEvidenceState(request, context, source.id, claimed.task.id, evidence, agents);
    currentAction('final-verify');
    const officeHoldResume = await holdAndResume(office, request, context.office, context.sales, officeReclaimed.task.id, sessions.office, { label: '差戻理由', text: '【合成データ】未送信の差戻理由。保留は差戻を実行しない。' });
    const holdResume = { sales: salesHoldResume, office: officeHoldResume };
    await assertHoldResumeState(request, context, holdResume);
    const beforeCompletion = await captureFinal(request, context, source.id, submitted.nextTask.id, resubmitted.snapshot.id, submitted.snapshot.id, returned.returnInstruction.id);
    expect(beforeCompletion.officeTask).toMatchObject({ state: 'active', canComplete: true });
    expect(beforeCompletion.officeTask.completionActionId).not.toBeNull();
    currentAction('complete-preview');
    await office.getByRole('button', { name: '完了内容を確認', exact: true }).click();
    const completionDialog = office.getByRole('dialog', { name: 'タスク完了の確認', exact: true });
    await expect(completionDialog).toContainText(officeReclaimed.task.attemptId);
    await expect(completionDialog).toContainText(sessions.office.actingAssignmentId);
    await expect(completionDialog).toContainText('完了後は読み取り専用');
    await expect(completionDialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await completionDialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    expect(await get(request, context.office, `/v1/organization/tasks/${officeReclaimed.task.id}`)).toEqual(beforeCompletion.officeTask);
    await office.getByRole('button', { name: '完了内容を確認', exact: true }).click();
    const completionResponse = office.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${officeReclaimed.task.id}/actions` && response.request().method() === 'POST');
    currentAction('complete-confirm');
    await office.getByRole('button', { name: '完了を確定', exact: true }).click();
    const completedResponse = await completionResponse;
    expect(completedResponse.status()).toBe(200);
    const completed = await completedResponse.json() as Completed;
    const completeCommand = completedResponse.request().postDataJSON() as WorkflowActionCommand;
    expect(completeCommand).toEqual({ operationId: expect.any(String), expectedRevision: beforeCompletion.officeTask.revision, actingAssignmentId: sessions.office.actingAssignmentId, expectedAttemptId: beforeCompletion.officeTask.attemptId, action: 'complete', definitionActionId: beforeCompletion.officeTask.completionActionId });
    expect(completed).toMatchObject({ kind: 'completed', task: { id: officeReclaimed.task.id, state: 'completed', attemptId: officeReclaimed.task.attemptId, revision: beforeCompletion.officeTask.revision + 1 } });
    await expect(office.getByText('タスクの完了が確定しました', { exact: true })).toBeVisible();
    await expect(office.getByRole('button', { name: '完了内容を確認', exact: true })).toHaveCount(0);
    await expect(office.getByLabel('差戻理由', { exact: true })).toHaveCount(0);
    await expect(office.getByRole('region', { name: '受領したスナップショット', exact: true })).toContainText(revisedText);
    await office.getByRole('button', { name: '履歴', exact: true }).click();
    await expect(office.getByText('タスクを完了', { exact: true })).toBeVisible();
    const completion = { operationId: completeCommand.operationId, command: completeCommand, result: completed };
    await assertCompletionState(request, context, completion);
    await assertEvidenceState(request, context, source.id, claimed.task.id, evidence, agents);
    await assertAgentState(request, context, agents);
    currentAction('final-verify');
    const final = await captureFinal(request, context, source.id, submitted.nextTask.id, resubmitted.snapshot.id, submitted.snapshot.id, returned.returnInstruction.id);
    const completionEvent = final.officeTask.history.at(-1);
    expect(completionEvent).toEqual({ kind: 'completed', occurredAt: expect.any(String) });
    expect(final.officeTask).toEqual({ ...beforeCompletion.officeTask, ...completed.task, history: [...beforeCompletion.officeTask.history, completionEvent] });
    // History is the shared workflow progress projection; source task content and revision stay unchanged.
    expect(final.salesTask).toEqual({ ...beforeCompletion.salesTask, history: [...beforeCompletion.salesTask.history, completionEvent] });
    expect(final.salesContext).toEqual({ ...beforeCompletion.salesContext, items: beforeCompletion.salesContext.items.map((item) => item.id === completed.task.id ? { ...item, state: 'completed', revision: completed.task.revision } : item) });
    expect(final.salesQueue).toEqual(beforeCompletion.salesQueue);
    for (const view of ['officeContext', 'officeQueue'] as const) expect(final[view]).toEqual({ ...beforeCompletion[view], items: beforeCompletion[view].items.map((item) => item.id === completed.task.id ? completed.task : item) });
    expect(final.snapshot).toEqual(resubmitted.snapshot);
    expect(final.priorSnapshot).toEqual(submitted.snapshot);
    expect(final.returnInstruction).toEqual(returned.returnInstruction);
    expect(final.salesTask).toMatchObject({ state: 'completed', attemptNumber: 2 });
    expect(final.officeTask).toMatchObject({ state: 'completed', attemptNumber: 2, workingArtifacts: [] });
    currentAction('final-verify');
    await saveState(context, { schemaVersion: 6, documentId: context.documentId, salesTaskId: source.id, officeTaskId: submitted.nextTask.id, artifactId: saved.artifact.id, snapshotId: submitted.snapshot.id, text, save: { operationId: saveCommand.operationId, result: saved }, submit: { operationId: submitCommand.operationId, result: submitted }, claim: { operationId: claimCommand.operationId, result: claimed }, rework: { text: revisedText, returned: { operationId: returnCommand.operationId, command: returnCommand, result: returned }, salesClaim: { operationId: salesClaimCommand.operationId, result: salesClaimed }, save: { operationId: resaveCommand.operationId, result: resaved }, submit: { operationId: resubmitCommand.operationId, result: resubmitted }, officeClaim: { operationId: officeReclaimCommand.operationId, result: officeReclaimed } }, evidence, agents, holdResume, completion, final });
  } finally {
    await officeContext.close();
  }
});

test.describe('System Root folder creation', () => {
  test.use({ acceptDownloads: false });

  test('実2名UIでSystem Root直下にフォルダーを作成し固定要求replayと現在Readを確認する', async ({ page, browser, request }) => {
    const context = readRuntimeContext();
    currentAction('root-folder-read');
    await assertSessions(request, context);
    const before = {
      sales: await readRootFolderSnapshot(request, context.sales),
      office: await readRootFolderSnapshot(request, context.office),
    };
    expect(before.sales.root.capabilities.createFolder).toEqual({ status: 'available' });
    expect(before.office.root.capabilities.createFolder).toEqual({ status: 'disabled', reason: 'permission' });
    expect(before.sales.root.folderId === before.office.root.folderId).toBe(true);
    for (const snapshot of Object.values(before)) expect(snapshot.root.parentFolderId).toBeNull();
    let folderPosts = 0;
    page.on('request', sent => {
      const url = new URL(sent.url());
      if (url.origin === context.sales && url.pathname === '/v1/folders' && sent.method() === 'POST') folderPosts++;
    });
    await openRootFolderHome(page, context.sales);
    const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
    // If the existing fixture has a child, exercise selection without creating extra fixtures.
    const existing = before.sales.children.items[0];
    if (existing) {
      const selection = rail.getByRole('button', { name: existing.name, exact: true });
      await selection.click();
      await expect(selection).toHaveAttribute('aria-current', 'location');
    }
    const entry = rail.getByRole('button', { name: 'System Rootにフォルダーを作成', exact: true });
    currentAction('root-folder-preview');
    await expect(entry).toBeEnabled();
    await entry.click();
    const dialog = page.getByRole('dialog', { name: 'System Rootにフォルダーを作成', exact: true });
    await expect(dialog).toContainText('登録先：System Root直下（選択中のフォルダーには作成しません）');
    await dialog.getByLabel('フォルダー名', { exact: true }).fill('【合成データ】キャンセルする名前');
    await dialog.getByLabel('作成理由', { exact: true }).fill('【合成データ】キャンセルする理由');
    currentAction('root-folder-cancel');
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(entry).toBeFocused();
    expect(folderPosts).toBe(0);
    await entry.click();
    expect(await dialog.getByLabel('フォルダー名', { exact: true }).inputValue() === '').toBe(true);
    expect(await dialog.getByLabel('作成理由', { exact: true }).inputValue() === '').toBe(true);
    expect(folderPosts).toBe(0);

    currentAction('root-folder-input');
    const name = `合成Rootフォルダー-${randomUUID()}`;
    const reason = '【合成データ】System Root直下のフォルダー作成を確認する';
    await dialog.getByLabel('フォルダー名', { exact: true }).fill(name);
    await dialog.getByLabel('作成理由', { exact: true }).fill(reason);
    // Observe the fresh server read and actual GUI POST before the click; never synthesize a receipt.
    const freshRootPromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales && new URL(response.url()).pathname === '/v1/folders/root' && response.request().method() === 'GET');
    const responsePromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales && new URL(response.url()).pathname === '/v1/folders' && response.request().method() === 'POST');
    currentAction('root-folder-create');
    await dialog.getByRole('button', { name: '作成する', exact: true }).click();
    const freshRootResponse = await freshRootPromise;
    expect(freshRootResponse.status()).toBe(200);
    const freshRoot = await freshRootResponse.json() as FolderDetail;
    expect(isDeepStrictEqual(freshRoot, before.sales.root)).toBe(true);
    const response = await responsePromise;
    expect(response.status()).toBe(201);
    const command = response.request().postDataJSON() as CreateFolderData['body'];
    const receipt = await response.json() as MutationResult;
    const uuidV7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    expect(uuidV7.test(command.operationId)).toBe(true);
    expect(uuidV7.test(command.folderId)).toBe(true);
    expect(command.operationId !== command.folderId).toBe(true);
    expect(isDeepStrictEqual(command, { operationId: command.operationId, folderId: command.folderId, parentFolderId: freshRoot.folderId, expectedParentRevision: freshRoot.revision, name, reason })).toBe(true);
    expect(receipt.operationId === command.operationId).toBe(true);
    expect(receipt.resourceId === command.folderId).toBe(true);
    expect(receipt.changed).toBe(true);
    expect(receipt.resultingRevision).toBe(0);
    expect(Number.isFinite(Date.parse(receipt.occurredAt))).toBe(true);
    await expect(dialog.getByRole('status')).toHaveText('フォルダーを作成しました。');
    await dialog.getByRole('button', { name: '確認して閉じる', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    expect(folderPosts).toBe(1);

    currentAction('root-folder-verify');
    const state: Omit<RootFolderState, 'selectedCreate'> = {
      schemaVersion: 3, documentId: context.documentId, request: command, receipt,
      paginationChildren: await prepareFolderPagination(request, context.sales, command.folderId, receipt.resultingRevision),
      sales: await readRootFolderSnapshot(request, context.sales),
      office: await readRootFolderSnapshot(request, context.office),
    };
    for (const role of ['sales', 'office'] as const) {
      assertRootFolderCreated(state[role], command);
      expect(isDeepStrictEqual(state[role].root, before[role].root)).toBe(true);
      expect(state[role].children.items.length).toBe(before[role].children.items.length + 1);
      expect(isDeepStrictEqual(state[role].children.items.filter(folder => folder.folderId !== command.folderId), before[role].children.items)).toBe(true);
    }
    await assertRootFolderUi(page, state.sales, command.folderId, 'sales');
    await assertFolderPaginationUi(page, state);
    const selectedCreate = await createSelectedFolderFromUi(page, request, context, state);
    await replaySelectedFolderCreate(request, context, { ...state, selectedCreate });
    await replayRootFolderCreate(request, context, state);
    // The independent office context never starts tracing or records video/screenshots.
    const officeContext = await browser.newContext({ locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block', acceptDownloads: false, recordVideo: undefined });
    try {
      const office = await officeContext.newPage();
      await openRootFolderHome(office, context.office);
      currentAction('root-folder-office');
      await assertRootFolderUi(office, state.office, command.folderId, 'office');
    } finally {
      await officeContext.close();
    }
    currentAction('root-folder-verify');
    for (const role of ['sales', 'office'] as const) expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
    expect(folderPosts).toBe(2);
    await saveRootFolderState(context, { ...state, selectedCreate });
  });
});
