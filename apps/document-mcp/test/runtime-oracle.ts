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
