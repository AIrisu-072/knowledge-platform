import { expect, test } from '@playwright/test';
import type { ArtifactContentWritten, HandoffSnapshot } from '../src/api/generated-work/types.gen';
import { hidden, openPage, read } from './policy-support';
import { readContextRuntime } from './context-support';
import { downloadFrom, filesAction, loadFilesState } from './files-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });

test('6 processの再起動後も提出したファイル・前回の提出・取込みと非開示を保持する', async ({ browser, request }) => {
  const context = readContextRuntime();
  const state = await loadFilesState(context);
  filesAction('files-persistence');
  const bytes = Buffer.from(state.bytesBase64, 'base64');
  const snapshotId = state.submitted.result.snapshot.id;
  // The pinned generation is still served from the Work store after restart.
  const pinned = await request.get(`${context.review}/v1/organization/handoff-snapshots/${snapshotId}/artifacts/${state.fileArtifactId}/content`);
  expect(pinned.status()).toBe(200);
  expect(Buffer.from(await pinned.body())).toEqual(bytes);
  expect(pinned.headers()['content-disposition']).toContain("filename*=UTF-8''");
  expect(await read<HandoffSnapshot>(request, context.review, `/v1/organization/handoff-snapshots/${snapshotId}`)).toEqual(state.submitted.result.snapshot);
  expect(await read<HandoffSnapshot>(request, context.review, `/v1/organization/handoff-snapshots/${state.firstSnapshot.id}`)).toEqual(state.firstSnapshot);
  // Receipts recover by operation ID; the content write keeps its server identity.
  expect(await read(request, context.sales, `/v1/organization/operations/${state.imported.operationId}`)).toEqual(state.imported.result);
  const written = await read<ArtifactContentWritten>(request, context.sales, `/v1/organization/operations/${state.contentOperationId}`);
  expect(written.artifact.file?.generation).toEqual(state.generation);
  // Non-disclosure is unchanged for everyone outside the handoff.
  for (const origin of [context.office, context.approver, context.delegate]) {
    await hidden(request, origin, `/v1/organization/handoff-snapshots/${snapshotId}/artifacts/${state.fileArtifactId}/content`, 'WORK_ARTIFACT_NOT_FOUND', state.fileName);
    await hidden(request, origin, `/v1/organization/working-artifacts/${state.fileArtifactId}/content`, 'WORK_ARTIFACT_NOT_FOUND', state.fileName);
  }
  const review = await openPage(browser, context.review);
  try {
    await review.goto(`/tasks?taskId=${state.bReviewTaskId}`);
    const received = review.getByRole('region', { name: '受領したスナップショット', exact: true });
    await expect(received).toContainText(state.fileName);
    const download = await downloadFrom(review, '受領したスナップショット', `${state.fileName} を取得`);
    expect(download.bytes).toEqual(bytes);
  } finally {
    await review.context().close();
  }
});
