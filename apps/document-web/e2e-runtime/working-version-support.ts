import { readFile, writeFile } from 'node:fs/promises';
import { BinaryTransportBridge, getDocument, getDocumentHistory, getVersionEditManifest,
  listDocumentRevisions, listDocumentVersions, listVersionFiles, publishVersion,
} from '@knowledge-platform/document-api-client';
import { hash, options, uuidV7, type runtime, type RuntimeContext } from './support';

export const workingEditorStatePath = (context: RuntimeContext) => `${context.statePath}.working-editor.json`;
export const sourceNames = ['原名/主 文書\t資料.txt', '原名/付録.txt'] as const;
export const renditionNames = ['主 文書の変換.txt', '付録の変換.txt'] as const;
export const originalBytes = [Buffer.from('【合成データ】主文書\n初回の本文です。\n'), Buffer.from('【合成データ】付録\n未変更の本文です。\n')];
export const renditionBytes = [Buffer.from('【合成データ】主文書の変換\n'), Buffer.from('【合成データ】付録の変換\n')];

export async function createMultiOriginalFixture(context: Awaited<ReturnType<typeof runtime>>) {
  const bridge = new BinaryTransportBridge({ baseUrl: context.human }), common = options(context.human);
  const title = '【合成データ】複数原本編集';
  const created = await bridge.createDocument({ request: { folderId: context.manifest.folders.shared.folderId, title,
    documentMetadata: {}, versionMetadata: {} }, file: new Blob([originalBytes[0]!]), originalFilename: 'seed.txt', mediaType: 'text/plain' });
  const documentId = created.documentId, versionId = created.documentVersionId;
  const detail = (await getDocument({ ...common, path: { documentId }, query: { view: 'authoring' } })).data;
  // Setup uses the existing full-manifest API. The GUI does not add/remove items.
  const files = new Map<string, Blob>();
  const items = sourceNames.map((originalFilename, ordinal) => {
    const partId = `original-${ordinal}`, renditionPartId = `rendition-${ordinal}`;
    files.set(partId, new Blob([originalBytes[ordinal]!], { type: 'text/plain' }));
    files.set(renditionPartId, new Blob([renditionBytes[ordinal]!], { type: 'text/plain' }));
    return { logicalPath: ordinal === 0 ? 'primary' : 'appendix', ordinal, fileId: uuidV7(), partId,
      mediaType: 'text/plain', originalFilename,
      renditions: [{ fileId: uuidV7(), partId: renditionPartId, mediaType: 'text/plain', originalFilename: renditionNames[ordinal]! }] };
  });
  const initialized = await bridge.updateWorkingVersion(documentId, versionId, { request: {
    operationId: uuidV7(), targetVersionId: versionId, expectedRevision: detail.revision, title, items }, files });
  if (initialized.baseVersionId !== null || initialized.targetVersionId !== versionId) throw Error('Initial update must retain the null-base version');
  await publishVersion({ ...common, path: { documentId, versionId }, body: { operationId: uuidV7(), expectedRevision: initialized.resultingRevision } });
  return { documentId, versionId, title };
}

export async function workingEditorSnapshot(baseUrl: string, documentId: string) {
  const common = options(baseUrl), path = { documentId }, bridge = new BinaryTransportBridge({ baseUrl });
  const detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
  const revisions = (await listDocumentRevisions({ ...common, path, query: { pageSize: 100 } })).data;
  const history = (await getDocumentHistory({ ...common, path, query: { pageSize: 100 } })).data;
  if (versions.nextCursor || revisions.nextCursor || history.nextCursor) throw Error('Unexpected synthetic editor pagination');
  const files = [];
  for (const version of versions.items) {
    const listed = (await listVersionFiles({ ...common, path: { ...path, versionId: version.versionId }, query: { purpose: 'history' } })).data;
    for (const file of listed.items) {
      const blob = await bridge.downloadVersionFileBlob({ ...path, versionId: version.versionId,
        contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'history' });
      files.push({ versionId: version.versionId, ...file, hash: hash(new Uint8Array(await blob.arrayBuffer())) });
    }
  }
  const working = versions.items.find(version => version.lifecycleState === 'working');
  const manifestVersionId = working?.versionId ?? detail.currentVersionId;
  if (!manifestVersionId) throw Error('Synthetic editor fixture requires an editable manifest');
  const manifest = (await getVersionEditManifest({ ...common, path: { ...path, versionId: manifestVersionId },
    query: { purpose: working ? 'authoring' : 'published' } })).data;
  return { documentId, revision: detail.revision, currentVersionId: detail.currentVersionId, manifest,
    versions: versions.items.map(({ firstReadAt: _firstReadAt, ...version }) => version),
    revisions: revisions.items, files,
    operations: history.items.filter(item => ['document.version.created', 'document.version.updated', 'document.version.published'].includes(item.actionCode))
      .map(item => ({ sourceKey: item.sourceKey, actionCode: item.actionCode, actor: item.actor?.principalId, details: item.details })) };
}

export type WorkingEditorState = { documents: Array<{ key: 'initial' | 'multiple'; snapshot: Awaited<ReturnType<typeof workingEditorSnapshot>> }> };
export async function saveWorkingEditorSnapshot(context: RuntimeContext, key: 'initial' | 'multiple', documentId: string) {
  let state: WorkingEditorState;
  try { state = JSON.parse(await readFile(workingEditorStatePath(context), 'utf8')) as WorkingEditorState; }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; state = { documents: [] }; }
  state.documents = state.documents.filter(item => item.key !== key);
  state.documents.push({ key, snapshot: await workingEditorSnapshot(context.human, documentId) });
  await writeFile(workingEditorStatePath(context), JSON.stringify(state, null, 2), { mode: 0o600 });
}
