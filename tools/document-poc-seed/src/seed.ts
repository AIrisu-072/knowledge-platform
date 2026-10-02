import { createHash, randomBytes } from 'node:crypto';
import { mkdir, open, readFile, rename, unlink } from 'node:fs/promises';
import { dirname } from 'node:path';
import {
  BinaryTransportBridge, createFolder, getDocument, getDocumentAccessPolicy,
  getFolderAccessPolicy, getRootFolder, getSession, getDocumentVersion, listDocumentRevisions, listDocuments,
  listDocumentVersions, listFolderChildren, listVersionFiles, publishVersion, recoverDocumentCreation, setFolderAccessPolicy,
} from '@knowledge-platform/document-api-client';
import type {
  CommandsCreateFolder, CommandsPolicyExplicit, CommandsPublishVersion,
  CommandsVersionWrite, CreateDocumentResult, MutationResult, CommandsSetAccessPolicy,
  PolicyGrantInput, ModelsFolder, ModelsVersion, PublishResult, VersionMutationResult,
} from '@knowledge-platform/document-api-client';

export class SeedSafetyError extends Error {}

export const FIXTURES = [
  { key: 'regulation', folder: 'shared', title: '規程サンプル', filename: 'regulation.txt', content: '【合成データ】規程サンプル\n第1条 この文書はPoC検証専用です。\n第2条 共有文書の更新履歴を確認します。\n', nextContent: '【合成データ】規程サンプル\n第1条 この文書はPoC検証専用です。\n第2条 共有文書の更新履歴と新旧対照表を確認します。\n' },
  { key: 'manual', folder: 'shared', title: 'マニュアルサンプル', filename: 'manual.txt', content: '【合成データ】マニュアルサンプル\n1. 文書一覧を開きます。\n2. 本文ファイルと版を確認します。\n' },
  { key: 'notice', folder: 'shared', title: '通達サンプル', filename: 'notice.txt', content: '【合成データ】通達サンプル\nPoC検証では実在する顧客情報を使用しません。\n' },
  { key: 'sandbox', folder: 'sandbox', title: 'Agent検証用文書', filename: 'agent-sandbox.txt', content: '【合成データ】Agent検証用文書\nAgentは文書の読取りと履歴確認のみを実施します。\n' },
  { key: 'humanOnly', folder: 'humanOnly', title: 'Human専用検証文書', filename: 'human-only.txt', content: '【合成データ】Human専用検証文書\nAgentからの参照拒否を確認するための合成文書です。\n' },
] as const;
export const FOLDERS = { shared: 'PoC Shared', sandbox: 'Agent Sandbox', humanOnly: 'Human Only' } as const;
export const HUMAN_GRANT = { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-users', actions: ['read', 'readHistory', 'write', 'publish', 'administer'] } as const;
export const AGENT_GRANT = { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-agents', actions: ['read', 'readHistory'] } as const;
export const fixtureHash = sha256(JSON.stringify({ FIXTURES, FOLDERS, HUMAN_GRANT, AGENT_GRANT }));
export function sha256(value: string | Uint8Array): string { return createHash('sha256').update(value).digest('hex'); }
export function newUuidV7(): string {
  const bytes = randomBytes(16);
  bytes.writeUIntBE(Date.now(), 0, 6);
  bytes[6] = (bytes[6]! & 0x0f) | 0x70;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = bytes.toString('hex');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export type Step<T, R> = { request: T; result?: R };
export type FolderState = {
  folderId: string; createOperationId: string; policyOperationId: string;
  create?: Step<CommandsCreateFolder, MutationResult>;
  policy?: Step<CommandsPolicyExplicit, MutationResult>;
};
export type DocumentState = {
  initialPublishOperationId: string; nextPublishOperationId: string;
  nextVersionOperationId: string; nextVersionId: string; nextFileId: string;
  create?: { pending: true; result?: CreateDocumentResult };
  initialPublish?: Step<CommandsPublishVersion, PublishResult>;
  nextVersion?: Step<CommandsVersionWrite, VersionMutationResult>;
  nextPublish?: Step<CommandsPublishVersion, PublishResult>;
  snapshot?: unknown;
};
export type Manifest = {
  schemaVersion: 1; fixtureHash: string; baseUrl: string; rootFolderId?: string;
  folders: Record<keyof typeof FOLDERS, FolderState>;
  documents: Record<string, DocumentState>;
};
export function normalizeBaseUrl(baseUrl: string): string {
  const url = new URL(baseUrl);
  if (url.protocol !== 'http:' || !['127.0.0.1', '[::1]', 'localhost'].includes(url.hostname)
    || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new SeedSafetyError('Seed requires a loopback HTTP origin without credentials, path or query');
  }
  return url.origin;
}
export async function saveManifest(path: string, manifest: Manifest): Promise<void> {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temp = `${path}.${newUuidV7()}.tmp`;
  const file = await open(temp, 'wx', 0o600);
  try { await file.writeFile(JSON.stringify(manifest, null, 2) + '\n'); await file.sync(); }
  finally { await file.close(); }
  await rename(temp, path);
  const directory = await open(dirname(path), 'r');
  try { await directory.sync(); } finally { await directory.close(); }
}
export async function openManifest(path: string, baseUrl: string): Promise<Manifest> {
  const normalized = normalizeBaseUrl(baseUrl);
  let existing: string;
  try { existing = await readFile(path, 'utf8'); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    const manifest: Manifest = {
      schemaVersion: 1, fixtureHash, baseUrl: normalized,
      folders: Object.fromEntries(Object.keys(FOLDERS).map((key) => [key, {
        folderId: newUuidV7(), createOperationId: newUuidV7(), policyOperationId: newUuidV7(),
      }])) as Manifest['folders'],
      documents: Object.fromEntries(FIXTURES.map(({ key }) => [key, {
        initialPublishOperationId: newUuidV7(), nextPublishOperationId: newUuidV7(),
        nextVersionOperationId: newUuidV7(), nextVersionId: newUuidV7(), nextFileId: newUuidV7(),
      }])),
    };
    await saveManifest(path, manifest);
    return manifest;
  }
  const manifest = JSON.parse(existing) as Manifest;
  if (manifest.schemaVersion !== 1 || manifest.fixtureHash !== fixtureHash || manifest.baseUrl !== normalized
    || JSON.stringify(Object.keys(manifest.folders ?? {}).sort()) !== JSON.stringify(Object.keys(FOLDERS).sort())
    || JSON.stringify(Object.keys(manifest.documents ?? {}).sort()) !== JSON.stringify(FIXTURES.map(f => f.key).sort())) {
    throw new SeedSafetyError('Manifest conflict: fixture definition, endpoint or schema changed; do not overwrite');
  }
  return manifest;
}
type SeedOptions = { manifestPath: string; baseUrl: string; fetch?: typeof fetch };
type Fixture = typeof FIXTURES[number];
async function payload<T>(request: Promise<{ data: T }>): Promise<T> { return (await request).data; }
function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') return `{${Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(',')}}`;
  return JSON.stringify(value);
}
function equal(actual: unknown, expected: unknown, label: string): void {
  if (canonical(actual) !== canonical(expected)) throw new SeedSafetyError(`Fixture conflict: ${label}; no data was overwritten`);
}
function normalizedGrants(grants: readonly PolicyGrantInput[]) {
  return grants.map(g => ({ subjectKind: g.subjectKind, identityProvider: g.identityProvider,
    subjectId: g.subjectId, actions: [...g.actions].sort() }))
    .sort((a, b) => canonical(a).localeCompare(canonical(b)));
}
function expectedGrants(key: string): PolicyGrantInput[] {
  return JSON.parse(JSON.stringify(key === 'humanOnly' ? [HUMAN_GRANT] : [HUMAN_GRANT, AGENT_GRANT])) as PolicyGrantInput[];
}
function metadata(fixture: Fixture) { return { syntheticFixture: 'document-poc-runtime-v0', fixtureKey: fixture.key, fixtureHash }; }

export async function runSeed(options: SeedOptions): Promise<Manifest> {
  normalizeBaseUrl(options.baseUrl);
  await mkdir(dirname(options.manifestPath), { recursive: true, mode: 0o700 });
  const lockPath = `${options.manifestPath}.lock`;
  const lock = await open(lockPath, 'wx', 0o600).catch((error: NodeJS.ErrnoException) => {
    if (error.code === 'EEXIST') throw new SeedSafetyError('Manifest locked: another seeder may be running. Do not remove the lock until that process has stopped');
    throw error;
  });
  try {
    await lock.writeFile(`${process.pid}\n`); await lock.sync();
    return await runLockedSeed(options);
  } finally { await lock.close(); await unlink(lockPath); }
}
async function runLockedSeed(options: SeedOptions): Promise<Manifest> {
  const manifest = await openManifest(options.manifestPath, options.baseUrl);
  const save = () => saveManifest(options.manifestPath, manifest);
  const common = { baseUrl: manifest.baseUrl, throwOnError: true as const, ...(options.fetch ? { fetch: options.fetch } : {}) };
  const bridge = new BinaryTransportBridge({ baseUrl: manifest.baseUrl, fetch: options.fetch });
  const session = await payload(getSession(common));
  if (session.principal.identityProvider !== 'poc' || session.principal.principalId !== 'poc-human'
    || session.invocationKind !== 'human_interactive') throw new SeedSafetyError('Seed requires the fixed poc-human instance');
  const root = await payload(getRootFolder(common));
  if (manifest.rootFolderId) equal(root.folderId, manifest.rootFolderId, 'root folder identity');
  const rootPolicy = await payload(getFolderAccessPolicy({ ...common, path: { folderId: root.folderId } }));
  equal(rootPolicy.bindingMode, 'explicit', 'root requires explicit bootstrap');
  equal(normalizedGrants(rootPolicy.effectiveGrants), normalizedGrants(expectedGrants('shared')), 'root bootstrap policy');
  manifest.rootFolderId = root.folderId;
  await save();
  async function children(): Promise<ModelsFolder[]> {
    let cursor: string | undefined;
    const items: ModelsFolder[] = [];
    do {
      const page = await payload(listFolderChildren({ ...common, path: { folderId: root.folderId }, query: { pageSize: 200, ...(cursor ? { cursor } : {}) } }));
      items.push(...page.items); cursor = page.nextCursor ?? undefined;
    } while (cursor);
    return items;
  }
  for (const [key, name] of Object.entries(FOLDERS)) {
    const state = manifest.folders[key as keyof typeof FOLDERS];
    const siblings = await children();
    const existing = siblings.find(f => f.folderId === state.folderId);
    if (siblings.some(f => f.name === name && f.folderId !== state.folderId)) throw new SeedSafetyError(`Fixture conflict: folder ${key} already exists without this manifest`);
    if (existing) {
      equal(existing.name, name, `folder ${key} name`);
      equal(existing.parentFolderId, root.folderId, `folder ${key} parent`);
      if (!state.create) throw new SeedSafetyError('Fixture conflict: unrecorded folder identity');
    } else if (state.create?.result) throw new SeedSafetyError('Fixture conflict: previously created folder is missing');
    if (!state.create) {
      state.create = { request: { operationId: state.createOperationId, folderId: state.folderId, parentFolderId: root.folderId,
        expectedParentRevision: root.revision, name, reason: 'Synthetic Document PoC fixture' } };
      await save();
    }
    if (!state.create.result) {
      state.create.result = await payload(createFolder({ ...common, body: state.create.request }));
      await save();
    }
    const path = { folderId: state.folderId };
    let policy = await payload(getFolderAccessPolicy({ ...common, path }));
    const grants = expectedGrants(key);
    if (!state.policy) {
      equal(policy.bindingMode, 'inherit', `new folder ${key} policy`);
      equal(policy.policyRevision, 0, `new folder ${key} policy revision`);
      state.policy = { request: { operationId: state.policyOperationId, expectedPolicyRevision: policy.policyRevision,
        mode: 'explicit', grants, reason: 'Synthetic Document PoC fixed-profile policy' } };
      await save();
    }
    if (!state.policy.result) {
      // Existing operation IDs perform authorized exact replay after an unknown response.
      // The generated discriminator bug is the same narrow cast used by the GUI adapter.
      state.policy.result = await payload(setFolderAccessPolicy({ ...common, path,
        body: state.policy.request as unknown as CommandsSetAccessPolicy }));
      await save();
      policy = await payload(getFolderAccessPolicy({ ...common, path }));
    }
    equal(policy.bindingMode, 'explicit', `folder ${key} policy mode`);
    equal(policy.policyRevision, state.policy.result.resultingRevision, `folder ${key} policy revision`);
    equal(normalizedGrants(policy.effectiveGrants), normalizedGrants(grants), `folder ${key} grants`);
  }

  for (const fixture of FIXTURES) {
    const state = manifest.documents[fixture.key]!;
    const folderId = manifest.folders[fixture.folder].folderId;
    if (state.create && !state.create.result) {
      throw new SeedSafetyError('Initial create outcome unknown: the API generates IDs server-side. Preserve this manifest and recreate only the disposable PoC database/storage; do not retry POST or guess IDs');
    }
    let cursor: string | undefined;
    do {
      const page = await payload(listDocuments({ ...common, query: { view: 'authoring', folderId, includeDescendants: false, pageSize: 100, ...(cursor ? { cursor } : {}) } }));
      if (page.items.some(d => d.title === fixture.title && d.documentId !== state.create?.result?.documentId)) throw new SeedSafetyError(`Fixture conflict: document ${fixture.key} already exists without this manifest`);
      cursor = page.nextCursor ?? undefined;
    } while (cursor);
    if (!state.create) {
      state.create = { pending: true };
      await save(); // A crash after this point must never blindly repeat initial creation.
      state.create.result = await bridge.createDocument({ request: { folderId, title: fixture.title,
        documentMetadata: metadata(fixture), versionMetadata: { syntheticFixture: fixture.key } },
        file: new Blob([fixture.content], { type: 'text/plain' }), originalFilename: fixture.filename, mediaType: 'text/plain' });
      await save();
    }
    const created = state.create.result!;
    const documentId = created.documentId;
    const path = { documentId };
    equal(await payload(recoverDocumentCreation({ ...common, path,
      query: { documentVersionId: created.documentVersionId, fileId: created.fileId } })), created,
      `document ${fixture.key} creation identity tuple`);
    async function inspect() {
      const detail = await payload(getDocument({ ...common, path, query: { view: 'authoring' } }));
      equal(detail.title, fixture.title, `document ${fixture.key} title`);
      equal(detail.folderId, folderId, `document ${fixture.key} folder`);
      equal(detail.metadata, metadata(fixture), `document ${fixture.key} metadata`);
      const policy = await payload(getDocumentAccessPolicy({ ...common, path }));
      equal(policy.bindingMode, 'inherit', `document ${fixture.key} policy mode`);
      equal(policy.policyRevision, 0, `document ${fixture.key} policy revision`);
      equal(policy.effectiveSource, { kind: 'folder', id: folderId }, `document ${fixture.key} policy source`);
      equal(normalizedGrants(policy.effectiveGrants), normalizedGrants(expectedGrants(fixture.folder)), `document ${fixture.key} grants`);
      const versions: ModelsVersion[] = [];
      let cursor: string | undefined;
      do {
        const page = await payload(listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100, ...(cursor ? { cursor } : {}) } }));
        versions.push(...page.items); cursor = page.nextCursor ?? undefined;
      } while (cursor);
      const fingerprints = [];
      for (const version of versions) {
        const initial = version.versionId === created.documentVersionId;
        const expectedContent = initial ? fixture.content : ('nextContent' in fixture && state.nextVersion && version.versionId === state.nextVersionId ? fixture.nextContent : undefined);
        if (expectedContent === undefined) throw new SeedSafetyError(`Fixture conflict: unexpected version on ${fixture.key}`);
        if (!['working', 'published'].includes(version.lifecycleState)) throw new SeedSafetyError(`Fixture conflict: unexpected lifecycle on ${fixture.key}`);
        const versionDetail = await payload(getDocumentVersion({ ...common, path: { documentId, versionId: version.versionId }, query: { purpose: 'history' } }));
        equal(versionDetail.title, fixture.title, `document ${fixture.key} version title`);
        equal(versionDetail.metadata, initial ? { syntheticFixture: fixture.key } : {}, `document ${fixture.key} version metadata`);
        const files = await payload(listVersionFiles({ ...common, path: { documentId, versionId: version.versionId }, query: { purpose: 'history' } }));
        equal(files.items.length, 1, `document ${fixture.key} file count`);
        const file = files.items[0]!;
        // FileId, ContentItemId and RepresentationId are distinct server identities.
        // The creation recovery operation verifies the initial file tuple; file reads use
        // the authoritative listing's item/representation IDs, then verify exact bytes.
        equal(file.mediaType, 'text/plain', `document ${fixture.key} media type`);
        equal(file.displayName, fixture.filename, `document ${fixture.key} filename`);
        const blob = await bridge.downloadVersionFileBlob({ documentId, versionId: version.versionId,
          contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'history' });
        const hash = sha256(new Uint8Array(await blob.arrayBuffer()));
        equal(hash, sha256(expectedContent), `document ${fixture.key} content hash`);
        fingerprints.push({ versionId: version.versionId, versionNo: version.versionNo,
          lifecycleState: version.lifecycleState, files: files.items, hash });
      }
      if (!versions.some(v => v.versionId === created.documentVersionId)) throw new SeedSafetyError('Fixture conflict: initial version is missing');
      const revisions = await payload(listDocumentRevisions({ ...common, path, query: { pageSize: 100 } }));
      equal(revisions.nextCursor, null, `document ${fixture.key} unexpected revision count`);
      return { title: detail.title, folderId: detail.folderId, revision: detail.revision, currentVersionId: detail.currentVersionId,
        metadata: detail.metadata, policy: { bindingMode: policy.bindingMode, policyRevision: policy.policyRevision,
          effectiveGrants: normalizedGrants(policy.effectiveGrants) },
        versions: fingerprints.sort((a, b) => a.versionNo - b.versionNo), revisions: revisions.items };
    }
    let current = await inspect();
    if (state.snapshot) { equal(current, state.snapshot, `document ${fixture.key} saved state`); continue; }
    if (!state.initialPublish) {
      state.initialPublish = { request: { operationId: state.initialPublishOperationId, expectedRevision: current.revision } };
      await save();
    }
    if (!state.initialPublish.result) {
      state.initialPublish.result = await payload(publishVersion({ ...common, path: { documentId, versionId: created.documentVersionId }, body: state.initialPublish.request }));
      await save();
    }
    if ('nextContent' in fixture) {
      current = await inspect();
      if (!state.nextVersion) {
        state.nextVersion = { request: { operationId: state.nextVersionOperationId, targetVersionId: state.nextVersionId,
          expectedRevision: current.revision, title: fixture.title,
          items: [{ logicalPath: 'primary', ordinal: 0, fileId: state.nextFileId, partId: 'primary', mediaType: 'text/plain', originalFilename: fixture.filename }] } };
        await save();
      }
      if (!state.nextVersion.result) {
        state.nextVersion.result = await bridge.createVersion(documentId, { request: state.nextVersion.request,
          files: new Map([['primary', new Blob([fixture.nextContent], { type: 'text/plain' })]]) });
        await save();
      }
      current = await inspect();
      if (!state.nextPublish) {
        state.nextPublish = { request: { operationId: state.nextPublishOperationId, expectedRevision: current.revision } };
        await save();
      }
      if (!state.nextPublish.result) {
        state.nextPublish.result = await payload(publishVersion({ ...common, path: { documentId, versionId: state.nextVersionId }, body: state.nextPublish.request }));
        await save();
      }
    }
    state.snapshot = await inspect();
    await save();
  }
  return manifest;
}
