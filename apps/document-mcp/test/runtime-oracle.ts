/** Fixtures and assertions for owned synthetic acceptance; never imported by product entrypoints. */
import assert from 'node:assert/strict';
import type { CommandsMetadataPatch, PublishedDocumentDetail, ModelsDocumentRevisionPage } from '@knowledge-platform/document-api-client';
function object(value: unknown): Record<string, unknown> {
  assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
}
function objects(value: unknown): Array<Record<string, unknown>> {
  assert.ok(Array.isArray(value));
  return value.map(object);
}
export function guiOracle(state: unknown, documentId: string): { regulation: Record<string, unknown>; pdf: Record<string, unknown> } {
  const documents = objects(object(state).documents);
  assert.equal(documents.filter(item => item.key === 'regulation').length, 1);
  assert.equal(documents.filter(item => item.key === 'pdf').length, 1);
  const regulation = object(documents.find(item => item.key === 'regulation')!.snapshot);
  const pdf = object(documents.find(item => item.key === 'pdf')!.snapshot);
  assert.equal(regulation.documentId, documentId);
  assert.equal(objects(regulation.revisions).length, 3, 'The real GUI must have published its third regulation Version');
  const current = objects(regulation.versions).find(version => version.versionId === regulation.currentVersionId);
  assert.equal(current?.versionNo, 3);
  assert.equal(typeof pdf.documentId, 'string');
  assert.notEqual(pdf.documentId, documentId);
  assert.equal(objects(pdf.revisions).length, 2);
  assert.ok(objects(pdf.versions).some(version => objects(version.files).some(file => file.mediaType === 'application/pdf')));
  return { regulation, pdf };
}
export function assertSnapshotMatches(snapshot: unknown, detail: unknown, revisions: unknown): void {
  const expected = object(snapshot), actual = object(detail), page = object(revisions);
  for (const key of ['documentId', 'title', 'metadata', 'currentVersionId', 'revision']) assert.deepEqual(actual[key], expected[key], key);
  assert.equal(page.nextCursor, null);
  assert.deepEqual(page.items, expected.revisions);
}
export function assertHistoryMatches(snapshot: unknown, history: unknown): void {
  const page = object(history);
  assert.equal(page.nextCursor, null);
  const publications = objects(page.items).filter(item => item.actionCode === 'document.version.published').map(item => ({
    sourceKey: item.sourceKey, actionCode: item.actionCode, actor: item.actor ? object(item.actor).principalId : undefined, provenanceQuality: item.provenanceQuality,
  }));
  assert.deepEqual(publications, object(snapshot).publications);
}
function semantics(value: unknown): Record<string, unknown> {
  return Object.fromEntries(Object.entries(object(value)).filter(([key]) => key !== 'auditEventId' && !key.endsWith('AuditEventId')));
}
export function assertComparisons(revision: unknown, version: unknown, humanRevision: unknown, humanVersion: unknown, base: string, target: string): void {
  const r = object(revision), v = object(version);
  assert.equal(r.projection, 'diff');
  assert.equal(object(r.baseRevision).revisionId, base);
  assert.equal(object(r.targetRevision).revisionId, target);
  assert.equal(v.projection, 'display');
  for (const result of [r, v]) {
    assert.equal(result.verdict, 'different', 'Known changed fixture must produce confirmed difference');
    assert.equal(result.coverage, 'full', 'Real worker failure or timeout must not pass acceptance');
    assert.deepEqual(result.unverifiedRegions, []);
  }
  assert.ok(objects(r.changes).length > 0);
  assert.ok(objects(v.items).length > 0);
  assert.equal(v.nextCursor, null);
  assert.deepEqual(semantics(r), semantics(humanRevision));
  assert.deepEqual(semantics(v), semantics(humanVersion));
}
export function metadataObservationPatch(before: Pick<PublishedDocumentDetail, 'revision' | 'metadata'>, runId: string, operationId: string): CommandsMetadataPatch {
  const metadata = object(before.metadata ?? {});
  // Management v0 permits extensions and replaces that entire object, so retain its keys.
  const extensions = metadata.extensions === undefined ? {} : object(metadata.extensions);
  return {
    operationId, expectedDocumentRevision: before.revision,
    set: { extensions: { ...extensions, pocAgentObservation: runId } },
    unset: [], reason: 'Synthetic Human to Agent consistency acceptance',
  };
}
export function assertMetadataUpdate(before: unknown, after: unknown, revisions: unknown, history: unknown, runId: string, operationId: string): void {
  const prior = object(before), current = object(after), page = object(revisions), events = object(history);
  const priorMetadata = object(prior.metadata ?? {}), currentMetadata = object(current.metadata);
  const extensions = priorMetadata.extensions === undefined ? {} : object(priorMetadata.extensions);
  assert.equal(object(currentMetadata.extensions).pocAgentObservation, runId);
  assert.deepEqual(currentMetadata, { ...priorMetadata, extensions: { ...extensions, pocAgentObservation: runId } });
  assert.ok(Number(current.revision) > Number(prior.revision));
  assert.equal(current.currentVersionId, prior.currentVersionId);
  const oldRevision = object(prior.displayRevision), revision = object(current.displayRevision);
  assert.notEqual(revision.revisionId, oldRevision.revisionId);
  assert.equal(revision.sourceKind, 'metadataRevision');
  assert.equal(revision.major, oldRevision.major);
  assert.equal(revision.minor, Number(oldRevision.minor) + 1);
  assert.equal(page.nextCursor, null);
  assert.ok(objects(page.items).some(item => item.revisionId === revision.revisionId && item.sourceKind === 'metadataRevision'));
  assert.equal(events.nextCursor, null);
  const entry = objects(events.items).find(item => item.sourceKey === `management:${operationId}`);
  assert.ok(entry);
  assert.equal(entry.actionCode, 'document.metadata.changed');
  assert.equal(entry.provenanceQuality, 'operationLedger');
  assert.equal(object(entry.actor).principalId, 'poc-human');
  assert.equal(object(entry.details).changed, true);
  assert.equal(object(entry.details).resulting_revision, current.revision);
}

// Projection shared by the two authorized profiles. Read acknowledgement and
// capability presentation are actor-local and deliberately excluded.
export function sharedDetail(value: unknown): Record<string, unknown> {
  const detail = object(value);
  return Object.fromEntries(['documentId', 'documentVersionId', 'title', 'metadata', 'revision', 'currentVersionId', 'displayVersion', 'displayRevision']
    .map(key => [key, detail[key]]));
}
export function assertSharedState(human: unknown, agent: unknown): void {
  const expected = object(human), actual = object(agent);
  assert.equal(object(expected.revisions).nextCursor, null, 'Synthetic history must be complete');
  assert.equal(object(actual.revisions).nextCursor, null, 'Agent history must be complete');
  assert.deepEqual(actual, expected, 'Actual MCP must match the current Human state at this checkpoint');
}
export function assertRevisionTransition(before: unknown | undefined, after: unknown, expected: {
  sourceKind: string; versionId: string; major: number; minor: number; resultingRevision: number; metadata: Record<string, unknown>;
}): void {
  const state = object(after), detail = object(state.detail), revisions = object(state.revisions);
  const items = objects(revisions.items), issued = object(detail.displayRevision);
  assert.equal(revisions.nextCursor, null);
  assert.equal(detail.currentVersionId, expected.versionId);
  assert.equal(detail.documentVersionId, expected.versionId);
  assert.equal(detail.revision, expected.resultingRevision);
  assert.deepEqual(detail.metadata, expected.metadata, 'metadata must equal the requested fixture value');
  assert.equal(issued.documentVersionId, expected.versionId);
  assert.equal(issued.sourceKind, expected.sourceKind);
  assert.equal(issued.major, expected.major); assert.equal(issued.minor, expected.minor);
  assert.equal(issued.label, `${expected.major}.${expected.minor}`);
  assert.deepEqual(items[0], issued);
  const old = before === undefined ? [] : objects(object(object(before).revisions).items);
  assert.equal(items.length, old.length + 1);
  assert.deepEqual(items.slice(1), old, 'Every previously issued Revision must remain identical');
  assert.equal(new Set(items.map(item => item.revisionId)).size, items.length, 'No reused Revision ID');
  if (before !== undefined) assert.equal(detail.documentId, object(object(before).detail).documentId);
}
export function assertNoopState(before: unknown, after: unknown, result: unknown): void {
  const mutation = object(result);
  assert.equal(mutation.changed, false);
  assert.equal(mutation.resultingRevision, object(object(before).detail).revision);
  assertSharedState(before, after);
}
export function assertMutationReplay(committed: unknown, after: unknown, firstRecovery: unknown, replay: unknown, history: unknown): void {
  const recovered = object(firstRecovery), state = object(object(committed).detail);
  assert.equal(recovered.changed, true);
  assert.equal(recovered.resourceId, state.documentId);
  assert.equal(recovered.resultingRevision, state.revision);
  assert.deepEqual(replay, firstRecovery, 'The exact operation ID and payload must return the saved result');
  assertSharedState(committed, after);
  const page = object(history); assert.equal(page.nextCursor, null);
  const entries = objects(page.items).filter(item => item.sourceKey === `management:${recovered.operationId}`);
  assert.equal(entries.length, 1, 'Recovery must not duplicate operation history');
  const entry = entries[0]!;
  assert.equal(entry.actionCode, 'document.metadata.changed');
  assert.equal(entry.provenanceQuality, 'operationLedger');
  assert.equal(object(entry.actor).principalId, 'poc-human');
  assert.equal(object(entry.details).changed, true);
  assert.equal(object(entry.details).resulting_revision, state.revision);
}

/** Inputs are selected only from an authorized, complete revision page. */
export function comparisonBodies(page: ModelsDocumentRevisionPage) {
  assert.equal(page.nextCursor, null);
  assert.ok(page.items.length >= 2);
  const base = page.items.at(-1)!;
  const target = page.items[0]!;
  assert.notEqual(base.revisionId, target.revisionId);
  assert.notEqual(base.documentVersionId, target.documentVersionId);
  return {
    revision: { baseRevisionId: base.revisionId, targetRevisionId: target.revisionId, projection: 'diff' as const },
    version: { baseVersionId: base.documentVersionId, targetVersionId: target.documentVersionId,
      profile: 'document-diff-v0' as const, projection: 'display' as const, pageSize: 100 },
  };
}
export function assertDocumentHidden(value: unknown): void {
  const result = object(value);
  assert.equal(result.isError, true);
  const content = objects(result.content);
  assert.equal(content.length, 1);
  assert.equal(content[0]!.type, 'text');
  assert.equal(typeof content[0]!.text, 'string');
  const problem = object(result.structuredContent);
  assert.deepEqual(JSON.parse(content[0]!.text as string), problem);
  assert.equal(problem.status, 404);
  assert.equal(problem.code, 'DOCUMENT_NOT_FOUND');
  assert.ok(Object.keys(problem).every(key => ['status', 'code', 'traceId', 'retryable'].includes(key)));
  assert.ok(Object.keys(result).every(key => ['isError', 'structuredContent', 'content'].includes(key)));
}
