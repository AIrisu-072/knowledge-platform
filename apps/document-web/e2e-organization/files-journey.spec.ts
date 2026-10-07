import { expect, test } from '@playwright/test';
import type { ArtifactContentWritten, ArtifactCreated, Completed, HandoffSnapshot, SubmissionImported, Submitted, TaskDetail } from '../src/api/generated-work/types.gen';
import { capture, hidden, openPage, read } from './policy-support';
import { loadContextState, readContextRuntime } from './context-support';
import { downloadFrom, filesAction, openDownloadPage, saveFilesState, sha256 } from './files-support';

// Image-free like the existing runtime acceptance; downloads are read as bytes only.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });

const fileName = '【合成】資金計画メモ.txt';
const bytes = Buffer.from('【合成データ】資金計画メモ。差戻し後に追加した作業ファイル。\n', 'utf8');
const addition = '\n【合成データ】資金使途を追記した。';

test('差戻し後に前回の提出を取り込み、作業ファイルを共有の作業領域へ保存して提出し、審査担当が取得する', async ({ browser, request }) => {
  const context = readContextRuntime();
  const state = await loadContextState(context);
  filesAction('files-setup');
  const firstSnapshotId = state.bReturned.result.returnInstruction.previousSubmissionId;
  const firstSnapshot = await read<HandoffSnapshot>(request, context.sales, `/v1/organization/handoff-snapshots/${firstSnapshotId}`);
  expect(firstSnapshot.artifacts.every((value) => value.file === undefined)).toBe(true);
  const sales = await openPage(browser, context.sales);
  const review = await openDownloadPage(browser, context.review);
  try {
    filesAction('files-claim');
    await sales.goto(`/tasks?taskId=${state.bSalesTaskId}`);
    await capture(sales, 'POST', new RegExp(`/tasks/${state.bSalesTaskId}/claim$`, 'u'), () => sales.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    await expect(sales.getByRole('region', { name: '確定した差戻指示', exact: true })).toBeVisible();
    await expect(sales.getByLabel('作業中の文案', { exact: true })).toHaveValue('');

    // Rework starts only from an explicit import; the earlier submission is never edited.
    filesAction('files-import');
    const rework = sales.getByRole('region', { name: '差戻し後の作業', exact: true });
    const imported = await capture<SubmissionImported>(sales, 'POST', new RegExp(`/tasks/${state.bSalesTaskId}/working-artifacts/import$`, 'u'), () => rework.getByRole('button', { name: '前回の提出内容を取り込む', exact: true }).click());
    expect(imported.result.artifacts.map((value) => value.derivedFrom)).toEqual(firstSnapshot.artifacts.map((value) => ({ snapshotId: firstSnapshotId, artifactId: value.artifactId })));
    await expect(sales.getByLabel('作業中の文案', { exact: true })).toHaveValue(firstSnapshot.artifacts[0]!.value!.text);
    const draft = sales.getByLabel('作業中の文案', { exact: true });
    await draft.fill(`${await draft.inputValue()}${addition}`);
    await capture(sales, 'PUT', /\/working-artifacts\/[0-9a-f-]{36}$/u, () => sales.getByRole('button', { name: '文案を保存', exact: true }).click());

    // A chosen file is uploaded explicitly to the Work-owned store; no path is sent.
    filesAction('files-attach');
    const files = sales.getByRole('region', { name: '作業ファイル', exact: true });
    const created = sales.waitForResponse((response) => response.request().method() === 'POST' && /\/tasks\/[0-9a-f-]{36}\/working-artifacts$/u.test(new URL(response.url()).pathname));
    const written = sales.waitForResponse((response) => response.request().method() === 'PUT' && /\/working-artifacts\/[0-9a-f-]{36}\/content$/u.test(new URL(response.url()).pathname));
    await files.getByLabel('作業ファイルを追加', { exact: true }).setInputFiles({ name: fileName, mimeType: 'text/plain', buffer: bytes });
    const createdResponse = await created;
    expect(createdResponse.status()).toBe(200);
    expect(createdResponse.request().postDataJSON()).toMatchObject({ file: { fileName, mediaType: 'text/plain' } });
    expect(JSON.stringify(createdResponse.request().postDataJSON())).not.toMatch(/fakepath|[\\/]home[\\/]/u);
    const fileArtifactId = ((await createdResponse.json()) as ArtifactCreated).artifact.id;
    const writtenResponse = await written;
    expect(writtenResponse.status()).toBe(200);
    const contentOperationId = writtenResponse.request().headers()['x-operation-id']!;
    const content = (await writtenResponse.json()) as ArtifactContentWritten;
    const generation = content.artifact.file!.generation!;
    expect(generation).toMatchObject({ id: contentOperationId, sizeBytes: bytes.length, sha256: sha256(bytes), providerId: 'organization.work-artifacts' });
    await expect(files.getByRole('list', { name: '作業ファイルの一覧', exact: true })).toContainText(`${fileName}`);
    await expect(files.getByRole('list', { name: '作業ファイルの一覧', exact: true })).toContainText('保存済み');

    // Private before submit: the next step, other staff and known IDs see nothing.
    filesAction('files-visibility');
    const own = await request.get(`${context.sales}/v1/organization/working-artifacts/${fileArtifactId}/content`);
    expect(own.status()).toBe(200);
    expect(Buffer.from(await own.body())).toEqual(bytes);
    expect(own.headers()['content-type']).toBe('application/octet-stream');
    expect(own.headers()['content-disposition']).toMatch(/^attachment; /u);
    expect(own.headers()['cache-control']).toBe('no-store');
    for (const origin of [context.review, context.office, context.approver]) {
      await hidden(request, origin, `/v1/organization/working-artifacts/${fileArtifactId}/content`, 'WORK_ARTIFACT_NOT_FOUND', fileName);
      await hidden(request, origin, `/v1/organization/working-artifacts/${fileArtifactId}`, 'WORK_ARTIFACT_NOT_FOUND', fileName);
    }

    filesAction('files-submit');
    await sales.getByRole('button', { name: '提出内容を確認', exact: true }).click();
    const dialog = sales.getByRole('dialog', { name: '提出の確認' });
    await expect(dialog.getByRole('list', { name: '提出するファイル', exact: true })).toContainText(fileName);
    const submitted = await capture<Submitted>(sales, 'POST', /\/submit$/u, () => dialog.getByRole('button', { name: '提出を確定', exact: true }).click());
    expect(submitted.result.snapshot.previousSubmissionId).toBe(firstSnapshotId);
    expect(submitted.result.snapshot.artifacts.find((value) => value.artifactId === fileArtifactId)?.file?.generation).toEqual(generation);
    const reviewTask = submitted.result.nextTask;
    expect(reviewTask).toMatchObject({ title: '審査内容確認', state: 'ready', attemptNumber: 2 });

    filesAction('files-review');
    await review.goto('/tasks');
    await review.getByRole('region', { name: '引受可能', exact: true }).getByRole('button', { name: /審査内容確認/u }).click();
    await capture(review, 'POST', new RegExp(`/tasks/${reviewTask.id}/claim$`, 'u'), () => review.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    const received = review.getByRole('region', { name: '受領したスナップショット', exact: true });
    await expect(received).toContainText(fileName);
    await expect(received).toContainText(addition.trim());
    // The earlier submission stays exactly as it was, beside the new one.
    await expect(review.getByRole('region', { name: '差戻前のスナップショット', exact: true })).not.toContainText(fileName);
    expect(await read<HandoffSnapshot>(request, context.review, `/v1/organization/handoff-snapshots/${firstSnapshotId}`)).toEqual(firstSnapshot);

    filesAction('files-download');
    const download = await downloadFrom(review, '受領したスナップショット', `${fileName} を取得`);
    expect(download.bytes).toEqual(bytes);
    // The authoritative name is the server's attachment header; a browser's local
    // save name depends on the host's filesystem encoding.
    const pinned = await request.get(`${context.review}/v1/organization/handoff-snapshots/${submitted.result.snapshot.id}/artifacts/${fileArtifactId}/content`);
    expect(pinned.headers()['content-disposition']).toContain(`filename*=UTF-8''${encodeURIComponent(fileName)}`);
    await expect(received).not.toContainText('資金計画メモ。差戻し後に追加');

    filesAction('files-complete');
    await review.getByRole('button', { name: '完了内容を確認', exact: true }).click();
    await capture<Completed>(review, 'POST', new RegExp(`/tasks/${reviewTask.id}/actions$`, 'u'), () => review.getByRole('dialog', { name: 'タスク完了の確認' }).getByRole('button', { name: '完了を確定', exact: true }).click());
    expect((await read<TaskDetail>(request, context.review, `/v1/organization/tasks/${reviewTask.id}`)).state).toBe('completed');

    await saveFilesState(context, {
      schemaVersion: 1, documentId: context.documentId, bSalesTaskId: state.bSalesTaskId, bReviewTaskId: reviewTask.id, fileArtifactId, fileName,
      bytesBase64: bytes.toString('base64'), generation, contentOperationId, firstSnapshot, imported, submitted,
    });
  } finally {
    for (const page of [sales, review]) await page.context().close();
  }
});
