import { readFile, writeFile } from 'node:fs/promises';
import { createHash, randomBytes } from 'node:crypto';
import { expect, type Page } from '@playwright/test';
import {
  BinaryTransportBridge, getCurrentDocumentVersionReadState, getDocument, getDocumentHistory, listDocumentRevisions,
  listDocumentVersions, listVersionFiles, type CommandsReadStateMutationRequest, type ModelsCurrentReadState, type ModelsReadStateMutationResult,
} from '@knowledge-platform/document-api-client';
import type { Manifest } from '../../../tools/document-poc-seed/src/seed';

export type RuntimeContext = { runId: string; human: string; agent: string; manifestPath: string; drainFixturePath: string; statePath: string; workerHashes: { dsi: string; diff: string }; visualCapture?: { directory: string; ownership: 'synthetic-owned-runtime'; database: 'harness-owned-disposable-loopback' } };
export async function runtime() {
  const context = JSON.parse(await readFile(process.env.KP_POC_RUNTIME_CONTEXT!, 'utf8')) as RuntimeContext;
  const manifest = JSON.parse(await readFile(context.manifestPath, 'utf8')) as Manifest;
  return { ...context, manifest };
}
export const options = (baseUrl: string) => ({ baseUrl, throwOnError: true as const });
export const hash = (value: Uint8Array) => createHash('sha256').update(value).digest('hex');
export function uuidV7(): string {
  const bytes = randomBytes(16); bytes.writeUIntBE(Date.now(), 0, 6);
  bytes[6] = (bytes[6]! & 15) | 0x70; bytes[8] = (bytes[8]! & 63) | 0x80;
  const hex = bytes.toString('hex');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export async function persistedSnapshot(baseUrl: string, documentId: string) {
  const common = options(baseUrl), path = { documentId };
  const detail = (await getDocument({ ...common, path, query: { view: 'published' } })).data;
  const revisions = (await listDocumentRevisions({ ...common, path, query: { pageSize: 100 } })).data;
  const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
  if (revisions.nextCursor || versions.nextCursor) throw Error('Synthetic acceptance snapshot unexpectedly requires pagination');
  const bridge = new BinaryTransportBridge({ baseUrl });
  const fingerprints = [];
  for (const version of versions.items) {
    const files = (await listVersionFiles({ ...common, path: { documentId, versionId: version.versionId }, query: { purpose: 'history' } })).data;
    const items = [];
    for (const file of files.items) {
      const blob = await bridge.downloadVersionFileBlob({ documentId, versionId: version.versionId, contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'history' });
      items.push({ contentItemId: file.contentItemId, representationId: file.representationId, mediaType: file.mediaType, hash: hash(new Uint8Array(await blob.arrayBuffer())) });
    }
    fingerprints.push({ versionId: version.versionId, versionNo: version.versionNo, lifecycleState: version.lifecycleState, files: items });
  }
  // Reading legitimately changes read-state/audit; only authoritative state belongs in this oracle.
  const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  const publications = history.items.filter(item => item.actionCode === 'document.version.published')
    .map(item => ({ sourceKey: item.sourceKey, actionCode: item.actionCode, actor: item.actor?.principalId, provenanceQuality: item.provenanceQuality }));
  return { documentId, title: detail.title, metadata: detail.metadata, revision: detail.revision,
    currentVersionId: detail.currentVersionId, revisions: revisions.items, versions: fingerprints, publications };
}
export type SavedDocumentReadState = {
  state: ModelsCurrentReadState; request: CommandsReadStateMutationRequest; receipt: ModelsReadStateMutationResult; documentRevision: number;
};
export type PersistedState = {
  documents: Array<{ key: string; snapshot: Awaited<ReturnType<typeof persistedSnapshot>> }>;
  // 本人だけの状態・固定receiptは共通snapshot/公開添付へ混ぜない。
  documentReadState?: SavedDocumentReadState;
};
export const currentReadState = async (context: RuntimeContext, path: { documentId: string; versionId: string }) =>
  (await getCurrentDocumentVersionReadState({ ...options(context.human), path })).data;
export async function observeReadStateChange(page: Page, context: RuntimeContext, before: ModelsCurrentReadState,
  kind: 'VIEW' | 'RESET', trigger: () => Promise<unknown>): Promise<SavedDocumentReadState> {
  const path = { documentId: before.documentId, versionId: before.versionId };
  const documentRevision = (await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data.revision;
  const response = page.waitForResponse(result => {
    const url = new URL(result.url());
    return url.origin === context.human && url.pathname === `/v1/documents/${path.documentId}/versions/${path.versionId}/read-state/${kind.toLowerCase()}`
      && result.request().method() === 'POST';
  });
  await trigger();
  const result = await response;
  expect(result.status()).toBe(200);
  const request = result.request().postDataJSON() as CommandsReadStateMutationRequest;
  expect(request.operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
  expect(request).toEqual({ operationId: request.operationId, expectedReadStateRevision: before.readStateRevision });
  const receipt = await result.json() as ModelsReadStateMutationResult;
  expect(receipt).toEqual({ ...path, operationId: request.operationId, kind, expectedReadStateRevision: before.readStateRevision,
    changed: true, occurredAt: expect.any(String), resultingReadState: {
      firstReadAt: before.firstReadAt ?? expect.any(String), needsRecheck: kind === 'RESET',
      readStateRevision: before.readStateRevision + 1, isRead: kind === 'VIEW',
    } });
  const region = page.getByRole('region', { name: '本人の既読状態', exact: true });
  await expect(region.getByRole('status')).toHaveText(kind === 'VIEW' ? '既読' : '未読');
  await expect(region.getByRole('button', { name: '現在の既読状態を再取得', exact: true })).toBeEnabled();
  const state = await currentReadState(context, path);
  expect(state).toEqual({ ...path, ...receipt.resultingReadState });
  expect((await getDocument({ ...options(context.human), path, query: { view: 'published' } })).data.revision).toBe(documentRevision);
  return { state, request, receipt, documentRevision };
}
export async function resetReadStateInGui(page: Page, context: RuntimeContext, before: ModelsCurrentReadState) {
  const result = await observeReadStateChange(page, context, before, 'RESET', () =>
    page.getByRole('region', { name: '本人の既読状態', exact: true }).getByRole('button', { name: '未読に戻す', exact: true }).click());
  await expect(page.getByRole('region', { name: '既読状態の操作結果', exact: true }))
    .toContainText('未読に戻しました。次に文書の詳細を開くと既読になります');
  return result;
}
export async function saveSnapshot(context: RuntimeContext, key: string, documentId: string) {
  let state: PersistedState;
  try { state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState; }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; state = { documents: [] }; }
  state.documents = state.documents.filter(item => item.key !== key);
  state.documents.push({ key, snapshot: await persistedSnapshot(context.human, documentId) });
  await writeFile(context.statePath, JSON.stringify(state, null, 2), { mode: 0o600 });
}
