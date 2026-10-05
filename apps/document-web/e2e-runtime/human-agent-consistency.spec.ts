import { visualCheckpoint } from './visual-capture';
import { test, expect, type Page } from '@playwright/test';
import { createRequire } from 'node:module';
import {
  BinaryTransportBridge, getDocument, getDocumentAccessPolicy, getDocumentHistory, listDocumentVersions,
  patchDocumentMetadata, publishVersion, setDocumentAccessPolicy, withdrawVersion,
  type CommandsMetadataPatch, type CommandsPolicyExplicit, type CommandsSetAccessPolicy,
  type PublishResult, type VersionMutationResult,
} from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, saveSnapshot, uuidV7 } from './support';
import type { SharedState } from '../../document-mcp/test/consistency';

const require = createRequire(import.meta.url);
const { readHumanSharedState, readMcpSharedState, sharedDetail, assertSharedState, assertRevisionTransition,
  assertNoopState, assertMutationReplay } = require('../../document-mcp/dist/consistency.cjs') as typeof import('../../document-mcp/test/consistency');
const { interruptMutationResponse } = require('../../../tools/document-poc-runtime/response-loss.mjs') as {
  interruptMutationResponse(origin: string, path: string, body: CommandsMetadataPatch): Promise<{ responseLost: boolean; upstreamStatus: number; payloadSha256: string }>;
};

test.describe.configure({ mode: 'serial' });
test.beforeEach(async ({ page }) => { await page.setViewportSize({ width: 1440, height: 900 }); });

async function publishInGui(page: Page, documentId: string, versionNo: number): Promise<PublishResult> {
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: new RegExp(`WORKING · 版 ${versionNo}`) }).click();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname.endsWith(':publish') && response.request().method() === 'POST');
  await page.getByRole('dialog', { name: '公開を確認' }).getByRole('button', { name: '確定する' }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  await expect(page.getByRole('status')).toContainText('公開しました');
  return await response.json() as PublishResult;
}

async function checkpoint(page: Page, context: Awaited<ReturnType<typeof runtime>>, documentId: string, name: string): Promise<SharedState> {
  const human = await readHumanSharedState(context.human, documentId);
  const guiResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.pathname === `/v1/documents/${documentId}` && url.searchParams.get('view') === 'published' && response.request().method() === 'GET';
  });
  await page.goto(`/documents/${documentId}?view=published&tab=versions`);
  const gui = await guiResponse; expect(gui.status()).toBe(200);
  expect(sharedDetail(await gui.json())).toEqual(human.detail);
  await expect(page.getByRole('heading', { name: String(human.detail.title), level: 1 })).toBeVisible();
  await expect(page.getByRole('heading', { name: '正式改訂', exact: true })).toBeVisible();
  const revision = human.revisions.items[0]!;
  await expect(page.getByText(revision.label, { exact: true }).first()).toBeVisible();
  await page.getByRole('tab', { name: '概要', exact: true }).click();
  await expect(page.getByText(JSON.stringify((human.detail.metadata as Record<string, unknown>).extensions), { exact: true })).toBeVisible();
  const agent = await readMcpSharedState(context.agent, documentId);
  assertSharedState(human, agent.state);
  // Same run/DB/storage; protocol shapes and synthetic response values remain run-local.
  await test.info().attach(`${name}.json`, { contentType: 'application/json', body: Buffer.from(JSON.stringify({ runId: context.runId, name,
    mutationChannel: ['initial-publication', 'new-major'].includes(name) ? 'Human GUI' : 'Human Common API',
    guiDetail: human.detail, human, agent: agent.state, actualStdioTranscript: agent.transcript }, null, 2)) });
  return human;
}

// No intercepted browser route, mock server, DB write or Agent mutation is used.
// Metadata/withdrawal have no approved GUI mutation control; the Human API owns
// those writes and the real GUI projection is checked immediately afterward.
test('ordered Human GUI/API → actual MCP equality covers publication, interrupted metadata recovery, no-op, new major and withdrawal fallback', async ({ page }) => {
  test.setTimeout(240_000); // Test observation budget, not a business SLO.
  const context = await runtime(), common = options(context.human);
  const initialBytes = Buffer.from('Synthetic ordered acceptance original.\n');
  const nextBytes = Buffer.from('Synthetic ordered acceptance changed content.\n');
  const created = await new BinaryTransportBridge({ baseUrl: context.human }).createDocument({
    request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic ordered Human Agent acceptance', documentMetadata: { extensions: { c3Stage: 'C3 initial metadata' } }, versionMetadata: {} },
    file: new Blob([initialBytes]), originalFilename: 'c3-initial.txt', mediaType: 'text/plain',
  });
  const documentId = created.documentId, path = { documentId };
  const published = await publishInGui(page, documentId, 1);
  expect(published.documentVersionId).toBe(created.documentVersionId);
  const initial = await checkpoint(page, context, documentId, 'initial-publication');
  expect(initial.detail.documentId).toBe(documentId);
  expect(initial.detail.title).toBe('Synthetic ordered Human Agent acceptance');
  assertRevisionTransition(undefined, initial, { sourceKind: 'initialPublication', versionId: created.documentVersionId, major: 1, minor: 0, resultingRevision: published.resultingDocumentRevision, metadata: { extensions: { c3Stage: 'C3 initial metadata' } } });

  const mutation: CommandsMetadataPatch = { operationId: uuidV7(), expectedDocumentRevision: Number(initial.detail.revision),
    set: { extensions: { c3Stage: 'C3 changed metadata' } }, unset: [], reason: 'Synthetic interrupted-response recovery acceptance' };
  const interrupted = await interruptMutationResponse(context.human, `/v1/documents/${documentId}/metadata`, mutation);
  expect(interrupted.upstreamStatus).toBe(200); expect(interrupted.responseLost).toBe(true);
  expect(interrupted.payloadSha256).toBe(hash(Buffer.from(JSON.stringify(mutation))));
  // The client did not receive a successful result. Observe state, then recover
  // with the exact saved operation ID + payload, including the old expected OCC.
  const committed = await readHumanSharedState(context.human, documentId);
  expect(committed.detail.revision).toBe(Number(initial.detail.revision) + 1);
  expect(committed.detail.metadata).toEqual({ extensions: { c3Stage: 'C3 changed metadata' } });
  const recovered = (await patchDocumentMetadata({ ...common, path, body: mutation })).data;
  expect(recovered.operationId).toBe(mutation.operationId);
  const replay = (await patchDocumentMetadata({ ...common, path, body: mutation })).data;
  const minor = await checkpoint(page, context, documentId, 'metadata-minor-and-response-recovery');
  assertRevisionTransition(initial, minor, { sourceKind: 'metadataRevision', versionId: created.documentVersionId, major: 1, minor: 1, resultingRevision: recovered.resultingRevision, metadata: { extensions: { c3Stage: 'C3 changed metadata' } } });
  expect(minor.files).toEqual(initial.files);
  assertMutationReplay(committed, minor, recovered, replay, (await getDocumentHistory({ ...common, path, query: { pageSize: 200 } })).data);
  await test.info().attach('interrupted-response-recovery.json', { contentType: 'application/json', body: Buffer.from(JSON.stringify({ runId: context.runId,
    fault: 'real upstream response discarded before downstream headers; no automatic retry', interrupted, mutation, recovered, replay }, null, 2)) });

  const noop = (await patchDocumentMetadata({ ...common, path, body: { ...mutation, operationId: uuidV7(), expectedDocumentRevision: recovered.resultingRevision } })).data;
  const unchanged = await checkpoint(page, context, documentId, 'metadata-noop');
  assertNoopState(minor, unchanged, noop);

  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await page.getByLabel(/^差替ファイル:/).setInputFiles({ name: 'c3-next.txt', mimeType: 'text/plain', buffer: nextBytes });
  const uploadResponse = page.waitForResponse(response => new URL(response.url()).pathname === `/v1/documents/${documentId}/versions` && response.request().method() === 'POST');
  await page.getByRole('button', { name: '新しい作業版を作成', exact: true }).click();
  const uploaded = await uploadResponse; expect(uploaded.status()).toBe(201);
  const version = await uploaded.json() as VersionMutationResult;
  expect(version.documentId).toBe(documentId); expect(version.versionNo).toBe(2); expect(version.baseVersionId).toBe(created.documentVersionId);
  expect(version.resultingRevision).toBe(recovered.resultingRevision + 1);
  await expect(page.getByRole('status')).toContainText('新しい作業版を作成しました');
  const contentPublication = await publishInGui(page, documentId, 2);
  const major = await checkpoint(page, context, documentId, 'new-major');
  assertRevisionTransition(unchanged, major, { sourceKind: 'contentPublication', versionId: version.targetVersionId, major: 2, minor: 0, resultingRevision: contentPublication.resultingDocumentRevision, metadata: { extensions: { c3Stage: 'C3 changed metadata' } } });
  expect(contentPublication.resultingDocumentRevision).toBe(version.resultingRevision + 1);
  expect(major.files.find(item => item.versionId === created.documentVersionId)).toEqual(initial.files[0]);
  expect(major.files).toHaveLength(2);

  const withdrawn = (await withdrawVersion({ ...common, path: { documentId, versionId: version.targetVersionId }, body: {
    operationId: uuidV7(), expectedRevision: contentPublication.resultingDocumentRevision, reason: 'Synthetic withdrawal fallback acceptance',
  } })).data;
  expect(withdrawn.targetVersionId).toBe(version.targetVersionId);
  expect(withdrawn.formerCurrentVersionId).toBe(version.targetVersionId);
  expect(withdrawn.resultingCurrentVersionId).toBe(created.documentVersionId);
  expect(withdrawn.restorationWithheldReason).toBeNull();
  expect(withdrawn.resultingRevision).toBe(contentPublication.resultingDocumentRevision + 1);
  const fallback = await checkpoint(page, context, documentId, 'withdrawal-fallback');
  assertRevisionTransition(major, fallback, { sourceKind: 'withdrawFallback', versionId: created.documentVersionId, major: 3, minor: 0, resultingRevision: withdrawn.resultingRevision, metadata: { extensions: { c3Stage: 'C3 changed metadata' } } });
  expect(fallback.detail.metadata).toEqual(major.detail.metadata); expect(fallback.files).toEqual(major.files);
  const final = await persistedSnapshot(context.human, documentId);
  expect(final.versions.find(item => item.versionId === created.documentVersionId)!.files[0]!.hash).toBe(hash(initialBytes));
  expect(final.versions.find(item => item.versionId === version.targetVersionId)!.files[0]!.hash).toBe(hash(nextBytes));
  expect(final.versions.find(item => item.versionId === version.targetVersionId)!.lifecycleState).toBe('withdrawn');
  await saveSnapshot(context, 'c3-consistency', documentId);
});

test('stale GUI create capability is reauthorized after Human write revocation, retains the file and recovers after restoration', async ({ page }) => {
  const context = await runtime(), common = options(context.human);
  const bytes = Buffer.from('Synthetic capability-race fixture.\n');
  const created = await new BinaryTransportBridge({ baseUrl: context.human }).createDocument({
    request: { folderId: context.manifest.folders.shared.folderId, title: 'Synthetic stale Human capability', documentMetadata: {}, versionMetadata: {} },
    file: new Blob([bytes]), originalFilename: 'race-original.txt', mediaType: 'text/plain',
  });
  const documentId = created.documentId, path = { documentId };
  const detail = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  await publishVersion({ ...common, path: { documentId, versionId: created.documentVersionId }, body: { operationId: uuidV7(), expectedRevision: detail.revision } });
  const before = await persistedSnapshot(context.human, documentId);
  const policy = (await getDocumentAccessPolicy({ ...common, path })).data;
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await page.getByLabel(/^差替ファイル:/).setInputFiles({ name: 'race-denied.txt', mimeType: 'text/plain', buffer: Buffer.from('Synthetic changed text after permission restoration.\n') });
  await expect(page.getByRole('button', { name: '新しい作業版を作成', exact: true })).toBeEnabled();
  let revoked = false;
  const restore = async () => {
    const current = (await getDocumentAccessPolicy({ ...common, path })).data;
    const body: CommandsPolicyExplicit = { operationId: uuidV7(), expectedPolicyRevision: current.policyRevision, mode: 'explicit', reason: 'Restore synthetic Human permissions after race',
      grants: policy.effectiveGrants.map(({ subjectKind, identityProvider, subjectId, actions }) => ({ subjectKind, identityProvider, subjectId, actions })) };
    await setDocumentAccessPolicy({ ...common, path, body: body as unknown as CommandsSetAccessPolicy }); revoked = false;
  };
  try {
    const body: CommandsPolicyExplicit = { operationId: uuidV7(), expectedPolicyRevision: policy.policyRevision, mode: 'explicit', reason: 'Synthetic stale capability revocation race',
      grants: [{ subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-users', actions: ['read', 'readHistory', 'administer'] },
        { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-agents', actions: ['read', 'readHistory'] }] };
    await setDocumentAccessPolicy({ ...common, path, body: body as unknown as CommandsSetAccessPolicy }); revoked = true;
    // Write revocation hides authoring detail; retained Read still exposes the published capability projection.
    const current = (await getDocument({ ...common, path, query: { view: 'published' } })).data;
    expect(current.capabilities.createVersion.status).toBe('disabled');
    expect(current.revision).toBe(before.revision); // policy changes cannot masquerade as OCC conflicts
    const hiddenAuthoring = await getDocument({ ...common, throwOnError: false, path, query: { view: 'authoring' } });
    expect(hiddenAuthoring.response?.status).toBe(404); expect(hiddenAuthoring.error?.code).toBe('DOCUMENT_NOT_FOUND');
    const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === `/v1/documents/${documentId}/versions` && response.request().method() === 'POST');
    await page.getByRole('button', { name: '新しい作業版を作成', exact: true }).click();
    const response = await responsePromise;
    // A fresh operation loses its Internal snapshot before mutation. Replayed
    // operations or revocation after loading that snapshot instead reach403.
    expect(response.status()).toBe(404); expect((await response.json()).code).toBe('DOCUMENT_NOT_FOUND');
    await expect(page.getByRole('alert')).toContainText('文書が見つからないか、閲覧できません');
    await expect(page.getByRole('status').filter({ hasText: '新しい作業版を作成しました' })).toHaveCount(0);
    await expect(page.getByLabel(/^差替ファイル:/)).toHaveValue(/race-denied\.txt$/);
    expect(await persistedSnapshot(context.human, documentId)).toEqual(before);
    const versions = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
    expect(versions.items).toHaveLength(1);
    await visualCheckpoint(page, '12-permission-denied-file-retained-1440.png');
    await restore();
    const retryResponse = page.waitForResponse(result => new URL(result.url()).pathname === `/v1/documents/${documentId}/versions` && result.request().method() === 'POST');
    await page.getByRole('button', { name: '最新状態を確認', exact: true }).click();
    await page.getByLabel(/^差替ファイル:/).setInputFiles({ name: 'race-denied.txt', mimeType: 'text/plain', buffer: Buffer.from('Synthetic changed text after permission restoration.\n') });
    await page.getByRole('button', { name: '新しい作業版を作成', exact: true }).click();
    expect((await retryResponse).status()).toBe(201);
    await expect(page.getByRole('status')).toContainText('新しい作業版を作成しました');
    const after = (await listDocumentVersions({ ...common, path, query: { purpose: 'history', pageSize: 100 } })).data;
    expect(after.items).toHaveLength(2);
    await visualCheckpoint(page, '13-permission-restored-retry-success-1440.png');
  } finally { if (revoked) await restore(); }
});
