import { readFile, writeFile } from 'node:fs/promises';
import {
  BinaryTransportBridge, getDocument, getDocumentHistory, listDocumentRevisions,
  listDocumentVersions, listVersionFiles, publishVersion,
} from '@knowledge-platform/document-api-client';
import { hash, options, uuidV7, type runtime, type RuntimeContext } from './support';

// Separate synthetic documents and restart state leave the accepted fixtures unchanged.
export async function createLifecycleFixture(context: Awaited<ReturnType<typeof runtime>>, title: string) {
  const common = options(context.human), bridge = new BinaryTransportBridge({ baseUrl: context.human });
  const firstBytes = Buffer.from('【合成データ】公開操作の検証\n第1条 初版の内容です。\n');
  const secondBytes = Buffer.from('【合成データ】公開操作の検証\n第1条 第二版の内容です。\n');
  const created = await bridge.createDocument({
    request: { folderId: context.manifest.folders.shared.folderId, title, documentMetadata: {}, versionMetadata: {} },
    file: new Blob([firstBytes]), originalFilename: 'lifecycle-first.txt', mediaType: 'text/plain',
  });
  const documentId = created.documentId, firstVersionId = created.documentVersionId, path = { documentId };
  let detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  await publishVersion({ ...common, path: { ...path, versionId: firstVersionId }, body: { operationId: uuidV7(), expectedRevision: detail.revision } });
  detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  const secondVersionId = uuidV7();
  await bridge.createVersion(documentId, {
    request: { operationId: uuidV7(), targetVersionId: secondVersionId, expectedRevision: detail.revision, title,
      items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'text/plain', originalFilename: 'lifecycle-second.txt' }] },
    files: new Map([['primary', new Blob([secondBytes], { type: 'text/plain' })]]),
  });
  detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  await publishVersion({ ...common, path: { ...path, versionId: secondVersionId }, body: { operationId: uuidV7(), expectedRevision: detail.revision } });
  return { documentId, firstVersionId, secondVersionId, hashes: [hash(secondBytes), hash(firstBytes)] };
}

export async function lifecycleSnapshot(baseUrl: string, documentId: string) {
  const common = options(baseUrl), path = { documentId };
  const detail = await getDocument({ ...common, throwOnError: false, path, query: { view: 'published' } });
  const published = detail.data
    ? { status: detail.response?.status, currentVersionId: detail.data.currentVersionId, revision: detail.data.revision }
    : { status: detail.response?.status, code: detail.error?.code };
  const versionPage = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
  const revisionPage = (await listDocumentRevisions({ ...common, path, query: { pageSize: 100 } })).data;
  const historyPage = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  if (versionPage.nextCursor || revisionPage.nextCursor || historyPage.nextCursor) throw Error('Synthetic lifecycle fixture unexpectedly requires pagination');
  const bridge = new BinaryTransportBridge({ baseUrl });
  const versions = [];
  for (const version of versionPage.items) {
    const files = (await listVersionFiles({ ...common, path: { ...path, versionId: version.versionId }, query: { purpose: 'history' } })).data;
    const originals = [];
    for (const file of files.items) {
      const original = await bridge.downloadVersionFileBlob({ ...path, versionId: version.versionId,
        contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'history' });
      originals.push({ ...file, hash: hash(new Uint8Array(await original.arrayBuffer())) });
    }
    // firstReadAt belongs to each actor; content, lifecycle, revisions and ledger IDs do not.
    const { firstReadAt: _firstReadAt, ...stableVersion } = version;
    versions.push({ ...stableVersion, files: originals });
  }
  const history = historyPage.items.filter(item => ['document.version.published', 'document.version.withdrawn', 'document.publication.ended'].includes(item.actionCode))
    .map(item => ({ sourceKey: item.sourceKey, actionCode: item.actionCode, actor: item.actor?.principalId,
      occurredAt: item.occurredAt, details: item.details, provenanceQuality: item.provenanceQuality }));
  return { documentId, published, versions, revisions: revisionPage.items, history };
}

export type LifecycleState = { documents: Array<{ key: 'withdrawn' | 'ended'; snapshot: Awaited<ReturnType<typeof lifecycleSnapshot>> }> };
export const lifecycleStatePath = (context: RuntimeContext) => `${context.statePath}.lifecycle.json`;
export async function saveLifecycleSnapshot(context: RuntimeContext, key: 'withdrawn' | 'ended', snapshot: Awaited<ReturnType<typeof lifecycleSnapshot>>) {
  let state: LifecycleState;
  try { state = JSON.parse(await readFile(lifecycleStatePath(context), 'utf8')) as LifecycleState; }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; state = { documents: [] }; }
  state.documents = state.documents.filter(item => item.key !== key);
  state.documents.push({ key, snapshot });
  await writeFile(lifecycleStatePath(context), JSON.stringify(state, null, 2), { mode: 0o600 });
}
