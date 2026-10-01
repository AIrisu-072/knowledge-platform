/** Assertions for the owned synthetic acceptance artifacts; never imported by product entrypoints. */
import assert from 'node:assert/strict';
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
