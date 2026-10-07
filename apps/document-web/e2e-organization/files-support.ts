import { expect, test, type Download, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import type { FileGeneration, HandoffSnapshot, SubmissionImported, Submitted } from '../src/api/generated-work/types.gen';
import type { ContextRuntime } from './context-support';

type FilesAction = 'files-setup' | 'files-claim' | 'files-import' | 'files-attach' | 'files-visibility' | 'files-submit' | 'files-review' | 'files-download' | 'files-complete' | 'files-persistence';
/** Closed failure projection marker shared with the existing harness diagnostics. */
export function filesAction(action: FilesAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  annotations.push({ type: 'organization-stage', description: action });
}
export const sha256 = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
/** Saved bytes of a browser download; never rendered in the page. */
export async function downloaded(download: Download): Promise<Buffer> {
  const stream = await download.createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream) chunks.push(Buffer.from(chunk as Uint8Array));
  return Buffer.concat(chunks);
}
export async function downloadFrom(page: Page, region: string, label: string): Promise<{ name: string; bytes: Buffer }> {
  const pending = page.waitForEvent('download');
  await page.getByRole('region', { name: region, exact: true }).getByRole('button', { name: label, exact: true }).click();
  const download = await pending;
  return { name: download.suggestedFilename(), bytes: await downloaded(download) };
}
type Receipt<T> = { operationId: string; result: T };
export type FilesState = {
  schemaVersion: 1;
  documentId: string;
  bSalesTaskId: string;
  bReviewTaskId: string;
  fileArtifactId: string;
  fileName: string;
  bytesBase64: string;
  generation: FileGeneration;
  contentOperationId: string;
  firstSnapshot: HandoffSnapshot;
  imported: Receipt<SubmissionImported>;
  submitted: Receipt<Submitted>;
};
const path = (context: ContextRuntime) => `${context.contextStatePath}.files`;
export async function saveFilesState(context: ContextRuntime, state: FilesState) {
  await writeFile(path(context), `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadFilesState(context: ContextRuntime): Promise<FilesState> {
  const state = JSON.parse(await readFile(path(context), 'utf8')) as FilesState;
  expect(state.schemaVersion).toBe(1);
  expect(state.documentId).toBe(context.documentId);
  return state;
}
