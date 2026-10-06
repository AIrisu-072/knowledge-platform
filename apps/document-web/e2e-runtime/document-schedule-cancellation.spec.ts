import { test, expect, type Page } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, getDocumentVersion, getSession, publishVersion,
  type CancelPublicationResult, type CommandsCancelPublicationSchedule, type SchedulePublicationResult,
} from '@knowledge-platform/document-api-client';
import { formatDateTime } from '../src/view-model/date-time';
import { startDiagnostics, finishDiagnostics } from './startup-diagnostics';
import { hash, options, persistedSnapshot, runtime, uuidV7 } from './support';

// 同じowned runtimeだけを利用し、成功時・失敗時とも画像や操作記録を残さない。
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); await startDiagnostics(page); });
test.afterEach(async ({ page }, info) => { await info.attach('runtime-startup.json', { body: Buffer.from(JSON.stringify(await finishDiagnostics(page))), contentType: 'application/json' }); });
const completed = (stage: string) => test.info().annotations.push({ type: 'runtime-completed', description: stage });
const scheduledCode = 'document.version.publication.scheduled';
const cancelledCode = 'document.version.publication.cancelled';
type Context = Awaited<ReturnType<typeof runtime>>;
type SavedState = { runId: string; documentId: string; versionId: string; state: Awaited<ReturnType<typeof readState>> };
const statePath = (context: Context) => `${context.statePath}.schedule-cancellation.json`;

async function readState(context: Context, documentId: string, versionId: string) {
  const human = await persistedSnapshot(context.human, documentId);
  const agent = await persistedSnapshot(context.agent, documentId);
  // ReadHistoryだけのAgentにはWORKING原本を公開しない。正式改訂と公開原本は同じ状態。
  expect(agent).toEqual({ ...human, versions: human.versions.filter(version => version.lifecycleState !== 'working') });
  const common = options(context.human), path = { documentId };
  const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  expect(history.nextCursor).toBeNull();
  expect((await getDocumentHistory({ ...options(context.agent), path, query: { pageSize: 100 } })).data).toEqual(history);
  const version = (await getDocumentVersion({ ...common, path: { documentId, versionId }, query: { purpose: 'authoring' } })).data;
  return { human, agent, history: history.items, version: {
    versionId: version.versionId, versionNo: version.versionNo, lifecycleState: version.lifecycleState,
    currentPublicationScheduleId: version.currentPublicationScheduleId, scheduledPublishAt: version.scheduledPublishAt,
    publishedAt: version.publishedAt, isCurrent: version.isCurrent,
    cancelAvailability: version.capabilities.cancelPublicationSchedule.status,
    scheduleAvailability: version.capabilities.schedulePublication.status,
  } };
}

async function openVersions(page: Page, documentId: string, versionId: string) {
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions&versionId=${versionId}`);
  await expect(page.getByRole('button', { name: /WORKING · 版 2/ })).toHaveAttribute('aria-pressed', 'true');
}

async function scheduleInGui(page: Page, documentId: string, versionId: string, instant: string) {
  await page.getByRole('button', { name: '予約公開する', exact: true }).click();
  const jstInput = new Date(Date.parse(instant) + 9 * 60 * 60 * 1000).toISOString().slice(0, 16);
  await page.getByLabel('公開日時（JST / UTC+09:00）', { exact: true }).fill(jstInput);
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await page.getByRole('button', { name: '公開を予約する', exact: true }).click();
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === `/v1/documents/${documentId}/versions/${versionId}:schedule-publication`
    && response.request().method() === 'POST');
  await page.getByRole('dialog', { name: '予約公開を確認', exact: true }).getByRole('button', { name: '確定する', exact: true }).press('Enter');
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const schedule = await response.json() as SchedulePublicationResult;
  expect(schedule).toMatchObject({ documentId, targetVersionId: versionId });
  expect(Date.parse(schedule.scheduledPublishAt)).toBe(Date.parse(instant));
  expect(schedule.publishOperationId).toBe(response.request().postDataJSON().operationId);
  await expect(page.getByRole('status')).toContainText('公開を予約しました');
  await page.getByRole('button', { name: '版の一覧へ戻る', exact: true }).click();
  return schedule;
}

async function openCancellation(page: Page, schedule: SchedulePublicationResult) {
  const trigger = page.getByRole('button', { name: '公開予約を取り消す', exact: true });
  await expect(trigger).toBeEnabled();
  await trigger.press('Enter');
  const dialog = page.getByRole('dialog', { name: '公開予約の取消', exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(formatDateTime(schedule.scheduledPublishAt, 'Asia/Tokyo'));
  await expect(dialog.getByRole('textbox')).toHaveCount(0);
  return dialog;
}

async function cancelInGui(page: Page, documentId: string, versionId: string, schedule: SchedulePublicationResult) {
  const dialog = await openCancellation(page, schedule);
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === `/v1/documents/${documentId}/versions/${versionId}:cancel-publication-schedule`
    && response.request().method() === 'POST');
  await dialog.getByRole('button', { name: '予約を取り消す', exact: true }).press('Enter');
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const command = response.request().postDataJSON() as CommandsCancelPublicationSchedule;
  expect(Object.keys(command).sort()).toEqual(['expectedRevision', 'operationId', 'publishOperationId']);
  expect(command.operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
  expect(command).toMatchObject({ publishOperationId: schedule.publishOperationId, expectedRevision: schedule.acceptedRevision });
  const result = await response.json() as CancelPublicationResult;
  expect(result).toEqual({ operationId: command.operationId, publishOperationId: schedule.publishOperationId,
    documentId, targetVersionId: versionId, resultingRevision: schedule.acceptedRevision + 1 });
  await expect(dialog).toBeHidden();
  const cancellation = page.getByRole('region', { name: '公開予約の取消操作', exact: true });
  await expect(cancellation.getByRole('status')).toHaveText('公開予約を取り消しました');
  await expect(page.getByRole('button', { name: '公開予約を取り消す', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '予約公開する', exact: true })).toBeEnabled();
  return result;
}

function expectCancelled(state: Awaited<ReturnType<typeof readState>>, baseline: Awaited<ReturnType<typeof readState>>) {
  expect(state.version).toMatchObject({ lifecycleState: 'working', currentPublicationScheduleId: null,
    scheduledPublishAt: null, publishedAt: null, isCurrent: false, cancelAvailability: 'disabled', scheduleAvailability: 'available' });
  expect(state.human.currentVersionId).toBe(baseline.human.currentVersionId);
  expect(state.human.revisions).toEqual(baseline.human.revisions);
  expect(state.human.publications).toEqual(baseline.human.publications);
  expect(state.human.versions).toEqual(baseline.human.versions);
}

if (process.env.KP_POC_RUNTIME_PHASE === 'journey') {
  test('GUIで予約を取り消し、再予約の現在IDと履歴を保持して両profileから確認する', async ({ page }) => {
    const context = await runtime(), common = options(context.human);
    expect((await getSession(common)).data.principal.principalId).toBe('poc-human');
    expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
    const bridge = new BinaryTransportBridge({ baseUrl: context.human });
    const baseBytes = Buffer.from('【合成データ】予約取消の基準原本です。\n');
    const workingBytes = Buffer.from('【合成データ】予約取消の更新原本です。\n');
    const created = await bridge.createDocument({ request: { folderId: context.manifest.folders.shared.folderId,
      title: '【合成データ】GUI公開予約の取消', documentMetadata: {}, versionMetadata: {} },
      file: new Blob([baseBytes]), originalFilename: 'primary', mediaType: 'text/plain' });
    const documentId = created.documentId, path = { documentId };
    let detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
    await publishVersion({ ...common, path: { documentId, versionId: created.documentVersionId },
      body: { operationId: uuidV7(), expectedRevision: detail.revision } });
    detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
    const versionId = uuidV7();
    await bridge.createVersion(documentId, { request: { operationId: uuidV7(), targetVersionId: versionId,
      expectedRevision: detail.revision, title: detail.title,
      items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'text/plain', originalFilename: 'primary' }] },
      files: new Map([['primary', new Blob([workingBytes], { type: 'text/plain' })]]) });
    const baseline = await readState(context, documentId, versionId);
    expect(baseline.human.revisions).toHaveLength(1);
    expect(baseline.human.versions.find(version => version.versionId === created.documentVersionId)?.files[0]?.hash).toBe(hash(baseBytes));
    expect(baseline.human.versions.find(version => version.versionId === versionId)?.files[0]?.hash).toBe(hash(workingBytes));
    expectCancelled(baseline, baseline);

    let cancellationRequests = 0;
    const apiOrigins = new Set<string>();
    page.on('request', request => {
      const url = new URL(request.url());
      if (url.pathname.startsWith('/v1/')) apiOrigins.add(url.origin);
      if (url.pathname.endsWith(':cancel-publication-schedule') && request.method() === 'POST') cancellationRequests++;
    });
    await openVersions(page, documentId, versionId);
    const due = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000); due.setUTCSeconds(0, 0);
    const first = await scheduleInGui(page, documentId, versionId, due.toISOString());
    const pending = await readState(context, documentId, versionId);
    expect(pending.version).toMatchObject({ currentPublicationScheduleId: first.publishOperationId,
      scheduledPublishAt: first.scheduledPublishAt, lifecycleState: 'working', cancelAvailability: 'available' });
    expect(first.acceptedRevision).toBe(baseline.human.revision + 1);
    completed('gui-schedule-created');

    const dialog = await openCancellation(page, first);
    await dialog.getByRole('button', { name: '戻る', exact: true }).press('Enter');
    await expect(dialog).toBeHidden();
    await expect(page.getByRole('button', { name: '公開予約を取り消す', exact: true })).toBeFocused();
    expect(cancellationRequests).toBe(0);
    expect(await readState(context, documentId, versionId)).toEqual(pending);
    completed('gui-schedule-dismissed');

    const firstCancel = await cancelInGui(page, documentId, versionId, first);
    const cancelled = await readState(context, documentId, versionId);
    expectCancelled(cancelled, baseline);
    expect(cancelled.human.revision).toBe(firstCancel.resultingRevision);
    expect(cancellationRequests).toBe(1);
    completed('gui-schedule-cancelled');

    const second = await scheduleInGui(page, documentId, versionId, new Date(due.valueOf() + 24 * 60 * 60 * 1000).toISOString());
    expect(second.publishOperationId).not.toBe(first.publishOperationId);
    expect(second.acceptedRevision).toBe(firstCancel.resultingRevision + 1);
    const replaced = await readState(context, documentId, versionId);
    expect(replaced.version).toMatchObject({ currentPublicationScheduleId: second.publishOperationId,
      scheduledPublishAt: second.scheduledPublishAt, cancelAvailability: 'available' });
    expect(replaced.history.filter(item => item.actionCode === scheduledCode).map(item => item.sourceKey).sort())
      .toEqual([`schedule:${first.publishOperationId}`, `schedule:${second.publishOperationId}`].sort());
    expect(replaced.history.filter(item => item.actionCode === cancelledCode)).toHaveLength(1);
    completed('gui-schedule-replaced');

    const secondCancel = await cancelInGui(page, documentId, versionId, second);
    const final = await readState(context, documentId, versionId);
    expectCancelled(final, baseline);
    expect(secondCancel.operationId).not.toBe(firstCancel.operationId);
    expect(final.human.revision).toBe(secondCancel.resultingRevision);
    expect(final.history.filter(item => item.actionCode === scheduledCode)).toEqual(replaced.history.filter(item => item.actionCode === scheduledCode));
    expect(final.history.filter(item => item.actionCode === cancelledCode).map(item => item.sourceKey).sort())
      .toEqual([`version_operation:${firstCancel.operationId}`, `version_operation:${secondCancel.operationId}`].sort());
    for (const item of final.history.filter(item => [scheduledCode, cancelledCode].includes(item.actionCode))) {
      expect(item).toMatchObject({ actor: { principalId: 'poc-human' }, provenanceQuality: 'operationLedger', details: { document_version_id: versionId } });
    }
    expect(cancellationRequests).toBe(2);
    expect(apiOrigins).toEqual(new Set([context.human]));
    // 既存共有snapshotのkey集合には追加しない。同じrun所有領域のprivate証跡だけを使う。
    const saved: SavedState = { runId: context.runId, documentId, versionId, state: final };
    await writeFile(statePath(context), JSON.stringify(saved, null, 2), { mode: 0o600 });
    completed('gui-schedule-final-state-saved');
  });
} else {
  test('両HTTP serverの再起動後も予約取消済みのWORKING・正式改訂・履歴・原本を保持する', async ({ page }) => {
    const context = await runtime();
    const saved = JSON.parse(await readFile(statePath(context), 'utf8')) as SavedState;
    expect(saved.runId).toBe(context.runId);
    expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
    expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
    const actual = await readState(context, saved.documentId, saved.versionId);
    expect(actual).toEqual(saved.state);
    expectCancelled(actual, saved.state);
    expect(actual.history.filter(item => item.actionCode === scheduledCode)).toHaveLength(2);
    expect(actual.history.filter(item => item.actionCode === cancelledCode)).toHaveLength(2);
    await openVersions(page, saved.documentId, saved.versionId);
    await expect(page.getByRole('heading', { name: saved.state.human.title, level: 1 })).toBeVisible();
    await expect(page.getByRole('button', { name: '予約公開する', exact: true })).toBeEnabled();
    await expect(page.getByRole('button', { name: '公開予約を取り消す', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: /WORKING · 版 2/ })).not.toContainText('予約公開');
    await page.getByRole('tab', { name: '履歴', exact: true }).click();
    await expect(page.getByText(cancelledCode, { exact: true })).toHaveCount(2);
    await expect(page.getByText(scheduledCode, { exact: true })).toHaveCount(2);
    completed('gui-schedule-restart-verified');
  });
}
