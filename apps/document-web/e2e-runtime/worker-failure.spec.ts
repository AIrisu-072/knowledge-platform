import { test, expect } from '@playwright/test';
import { createRequire } from 'node:module';
import { dirname } from 'node:path';
import {
  BinaryTransportBridge, compareDocumentVersions, getDocument, publishVersion,
  type VersionUpload,
} from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, saveSnapshot, uuidV7 } from './support';

type Worker = 'dsi' | 'diff';
type Observation = { worker: Worker; sha256Before: string; sha256After: string; modeBefore: number; disabledMode: number; modeAfter: number };
const { withUnavailableWorker, assertWorkerFailure, assertRecoveredWorkerDiff } = createRequire(import.meta.url)('../../../tools/document-poc-runtime/worker-controls.mjs') as {
  withUnavailableWorker<T>(directory: string, worker: Worker, expectedHash: string, action: () => Promise<T>): Promise<{ value: T; observation: Observation }>;
  assertWorkerFailure(error: unknown, worker: Worker, forbidden: string[]): { status: number; code: string; retryable: boolean };
  assertRecoveredWorkerDiff(result: unknown, baseText: string, targetText: string): void;
};

function bridge(origin: string, observe?: (response: Response) => void) {
  return new BinaryTransportBridge({ baseUrl: origin, fetch: async (input, init) => {
    const response = await fetch(input, { ...init, redirect: 'error', credentials: 'omit',
      signal: init?.signal ? AbortSignal.any([init.signal, AbortSignal.timeout(50_000)]) : AbortSignal.timeout(50_000) });
    observe?.(response); return response;
  } });
}
async function health(context: Awaited<ReturnType<typeof runtime>>, readyStatus: number) {
  for (const origin of [context.human, context.agent]) for (const endpoint of ['live', 'ready']) {
    const status = endpoint === 'ready' ? readyStatus : 200;
    const result = await fetch(`${origin}/health/${endpoint}`, { signal: AbortSignal.timeout(35_000) });
    expect(result.status).toBe(status);
    expect(await result.json()).toEqual({ status: status === 200 ? 'ok' : 'unavailable' });
  }
}
async function fixture(worker: Worker) {
  const context = await runtime(), common = options(context.human), binary = bridge(context.human);
  const directory = dirname(process.env.KP_POC_RUNTIME_CONTEXT!);
  const title = `Synthetic ${worker} worker acceptance ${context.runId}`;
  const baseBytes = Buffer.from(`${title}\noriginal-value\n`), changedBytes = Buffer.from(`${title}\nupdated-value\n`);
  const created = await binary.createDocument({ request: { folderId: context.manifest.folders.shared.folderId, title, documentMetadata: {}, versionMetadata: {} },
    file: new Blob([baseBytes]), originalFilename: 'worker-base.txt', mediaType: 'text/plain' });
  const documentId = created.documentId, path = { documentId };
  const draft = (await getDocument({ ...common, path, query: { view: 'authoring' } })).data;
  const published = (await publishVersion({ ...common, path: { documentId, versionId: created.documentVersionId }, body: { operationId: uuidV7(), expectedRevision: draft.revision } })).data;
  const upload: VersionUpload = { request: { operationId: uuidV7(), targetVersionId: uuidV7(), expectedRevision: published.resultingDocumentRevision, title,
    items: [{ logicalPath: 'primary', ordinal: 0, fileId: uuidV7(), partId: 'primary', mediaType: 'text/plain', originalFilename: 'worker-change.txt' }] },
    files: new Map([['primary', new Blob([changedBytes], { type: 'text/plain' })]]) };
  return { context, common, binary, directory, documentId, created, upload, baseBytes, changedBytes, title };
}

// Permission faults affect only the hash-verified copied production executable
// in this run's private directory. No worker implementation is replaced.
test('actual unavailable DSI returns503 without Version/Revision/publication success, then exact upload recovery succeeds', async () => {
  const f = await fixture('dsi');
  const before = await persistedSnapshot(f.context.human, f.documentId);
  expect(before.versions).toHaveLength(1); expect(before.revisions).toHaveLength(1); expect(before.publications).toHaveLength(1);
  await health(f.context, 200);
  const started = Date.now();
  const fault = await withUnavailableWorker(f.directory, 'dsi', f.context.workerHashes.dsi, async () => {
    await health(f.context, 503);
    let failure: unknown, transport: Response | undefined;
    const unavailable = bridge(f.context.human, response => { transport = response; });
    try { await unavailable.createVersion(f.documentId, f.upload); } catch (error) { failure = error; }
    if (!transport) throw Error('Actual DSI failure response required');
    expect((failure as { status?: number })?.status).toBe(transport.status);
    const problem = assertWorkerFailure({ status: transport.status, contentType: transport.headers.get('content-type'),
      instance: new URL(transport.url).pathname, problem: (failure as { problem?: unknown })?.problem }, 'dsi',
      [f.directory, f.title, 'worker-base.txt', 'worker-change.txt', 'original-value', 'updated-value']);
    expect(await persistedSnapshot(f.context.human, f.documentId)).toEqual(before);
    return problem;
  });
  await health(f.context, 200);
  // Reuse every generated ID, original expected OCC and byte payload. Prepared
  // immutable FileObjects may exist after failure; no business Version may exist.
  const recovered = await f.binary.createVersion(f.documentId, f.upload);
  const replay = await f.binary.createVersion(f.documentId, f.upload);
  expect(replay).toEqual(recovered);
  expect(recovered.operationId).toBe(f.upload.request.operationId);
  expect(recovered.targetVersionId).toBe(f.upload.request.targetVersionId);
  expect(recovered.resultingRevision).toBe(before.revision + 1);
  const working = await persistedSnapshot(f.context.human, f.documentId);
  expect(working.versions).toHaveLength(2); expect(working.revisions).toEqual(before.revisions);
  expect(working.currentVersionId).toBe(before.currentVersionId); expect(working.publications).toEqual(before.publications);
  const publication = (await publishVersion({ ...f.common, path: { documentId: f.documentId, versionId: recovered.targetVersionId },
    body: { operationId: uuidV7(), expectedRevision: recovered.resultingRevision } })).data;
  const after = await persistedSnapshot(f.context.human, f.documentId);
  expect(after.currentVersionId).toBe(recovered.targetVersionId); expect(after.revision).toBe(publication.resultingDocumentRevision);
  expect(after.revisions).toHaveLength(2); expect(after.publications).toHaveLength(2);
  expect(after.versions.find(item => item.versionId === f.created.documentVersionId)!.files[0]!.hash).toBe(hash(f.baseBytes));
  expect(after.versions.find(item => item.versionId === recovered.targetVersionId)!.files[0]!.hash).toBe(hash(f.changedBytes));
  expect(await persistedSnapshot(f.context.agent, f.documentId)).toEqual(after);
  await saveSnapshot(f.context, 'c3-dsi-recovery', f.documentId);
  await test.info().attach('dsi-failure-recovery.json', { contentType: 'application/json', body: Buffer.from(JSON.stringify({ runId: f.context.runId,
    fault: fault.observation, failure: fault.value, recovered, replay, publication, elapsedMs: Date.now() - started }, null, 2)) });
});

test('actual unavailable Diff returns500 with no fragment or false unchanged result, then the uncached pair recovers', async () => {
  const f = await fixture('diff');
  const next = await f.binary.createVersion(f.documentId, f.upload);
  await publishVersion({ ...f.common, path: { documentId: f.documentId, versionId: next.targetVersionId },
    body: { operationId: uuidV7(), expectedRevision: next.resultingRevision } });
  // This new document/pair has never been compared; a cache hit cannot mask the
  // unavailable executable. Snapshot reads do not invoke the comparison API.
  const before = await persistedSnapshot(f.context.human, f.documentId);
  const body = { baseVersionId: f.created.documentVersionId, targetVersionId: next.targetVersionId,
    profile: 'document-diff-v0' as const, projection: 'display' as const, pageSize: 100 };
  const request = { ...f.common, path: { documentId: f.documentId }, body, get signal() { return AbortSignal.timeout(50_000); } };
  await health(f.context, 200);
  const started = Date.now();
  const fault = await withUnavailableWorker(f.directory, 'diff', f.context.workerHashes.diff, async () => {
    await health(f.context, 503);
    const failure = await compareDocumentVersions({ ...request, throwOnError: false });
    if (!failure.response) throw Error('Actual Diff failure response required');
    const problem = assertWorkerFailure({ status: failure.response.status, contentType: failure.response.headers.get('content-type'),
      instance: new URL(failure.response.url).pathname, problem: failure.error }, 'diff',
      [f.directory, f.title, 'worker-base.txt', 'worker-change.txt', 'original-value', 'updated-value']);
    expect(await persistedSnapshot(f.context.human, f.documentId)).toEqual(before);
    return problem;
  });
  await health(f.context, 200);
  const recovered = (await compareDocumentVersions(request)).data;
  assertRecoveredWorkerDiff(recovered, 'original-value', 'updated-value');
  expect(recovered.resultDigest).toMatch(/^[a-f0-9]{64}$/);
  const evidence = (await compareDocumentVersions({ ...request, body: { ...body, projection: 'diff' } })).data;
  expect(evidence.projection).toBe('diff');
  if (evidence.projection !== 'diff') throw Error('Recovered source evidence projection required');
  expect(evidence.resultDigest).toBe(recovered.resultDigest);
  expect(evidence.changes.some(item => item.base?.documentId === f.documentId && item.base.versionId === f.created.documentVersionId)).toBe(true);
  expect(evidence.changes.some(item => item.target?.documentId === f.documentId && item.target.versionId === next.targetVersionId)).toBe(true);
  expect(await persistedSnapshot(f.context.human, f.documentId)).toEqual(before);
  expect(await persistedSnapshot(f.context.agent, f.documentId)).toEqual(before);
  await saveSnapshot(f.context, 'c3-diff-recovery', f.documentId);
  await test.info().attach('diff-failure-recovery.json', { contentType: 'application/json', body: Buffer.from(JSON.stringify({ runId: f.context.runId,
    fault: fault.observation, failure: fault.value, pair: body, resultDigest: recovered.resultDigest, elapsedMs: Date.now() - started }, null, 2)) });
});
