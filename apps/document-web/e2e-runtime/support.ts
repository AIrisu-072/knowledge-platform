import { readFile, writeFile } from 'node:fs/promises';
import { createHash, randomBytes } from 'node:crypto';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, listDocumentRevisions,
  listDocumentVersions, listVersionFiles,
} from '@knowledge-platform/document-api-client';
import type { Manifest } from '../../../tools/document-poc-seed/src/seed';

export type RuntimeContext = { runId: string; human: string; agent: string; manifestPath: string; drainFixturePath: string; statePath: string };
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
export type PersistedState = { documents: Array<{ key: string; snapshot: Awaited<ReturnType<typeof persistedSnapshot>> }> };
export async function saveSnapshot(context: RuntimeContext, key: string, documentId: string) {
  let state: PersistedState;
  try { state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState; }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; state = { documents: [] }; }
  state.documents = state.documents.filter(item => item.key !== key);
  state.documents.push({ key, snapshot: await persistedSnapshot(context.human, documentId) });
  await writeFile(context.statePath, JSON.stringify(state, null, 2), { mode: 0o600 });
}
