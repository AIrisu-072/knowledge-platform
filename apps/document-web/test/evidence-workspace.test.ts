import { selectedHandoffIsClosed, sourceFromPublishedDocument } from '../src/application/evidence-workspace';

const evidence = { id: 'evidence-1', revision: 1 as const };
const finding = { id: 'finding-1', revision: 1 as const, evidenceRevisionRefs: [evidence] };
const decision = { id: 'decision-1', revision: 1 as const, findingId: finding.id, findingRevision: 1 as const, evidenceRevisionRefs: [evidence] };
const document = { documentId: 'document-1', documentVersionId: 'version-1', displayRevision: { revisionId: 'revision-1', documentVersionId: 'version-1', label: '1.0' } };
const file = { contentItemId: 'content-1', representationId: 'representation-1', role: 'AUTHORITATIVE', displayName: 'source.txt' };

test('explicit handoff membership is closed only when selected exact support revisions are present', () => {
  const selected = { evidenceRevisionRefs: [], findingRevisionRefs: [finding], decisionRevisionRefs: [] };
  expect(selectedHandoffIsClosed(selected, [evidence], [finding], [decision])).toBe(false);
  expect(selectedHandoffIsClosed({ ...selected, evidenceRevisionRefs: [evidence] }, [evidence], [finding], [decision])).toBe(true);
  expect(selectedHandoffIsClosed({ ...selected, evidenceRevisionRefs: [{ ...evidence, revision: 2 } as never] }, [evidence], [finding], [decision])).toBe(false);
});
test('decision sharing requires its exact finding and support, while sharing nothing is valid', () => {
  const empty = { evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [] };
  expect(selectedHandoffIsClosed(empty, [evidence], [finding], [decision])).toBe(true);
  expect(selectedHandoffIsClosed({ ...empty, evidenceRevisionRefs: [evidence], decisionRevisionRefs: [decision] }, [evidence], [finding], [decision])).toBe(false);
  expect(selectedHandoffIsClosed({ evidenceRevisionRefs: [evidence], findingRevisionRefs: [finding], decisionRevisionRefs: [decision] }, [evidence], [finding], [decision])).toBe(true);
});
test('source binds a published revision to the selected actual authoritative file without invented locations', () => {
  expect(sourceFromPublishedDocument('document-1', document as never, file as never)).toEqual({ sourceRef: { providerId: 'document', resourceId: 'document-1', revisionId: 'revision-1', versionId: 'version-1' }, authoritativeLocator: { kind: 'contentItem', contentItemId: 'content-1', representationId: 'representation-1' } });
  expect(sourceFromPublishedDocument('document-1', { ...document, displayRevision: null } as never, file as never)).toBeNull();
  expect(sourceFromPublishedDocument('document-1', { ...document, documentId: 'other-document' } as never, file as never)).toBeNull();
  expect(sourceFromPublishedDocument('document-1', document as never, { ...file, role: 'DERIVED' } as never)).toBeNull();
  expect(sourceFromPublishedDocument('document-1', { ...document, displayRevision: { ...document.displayRevision, documentVersionId: 'other-version' } } as never, file as never)).toBeNull();
});

test('handoff caps the combined selected set at one hundred records', () => {
  const evidence = Array.from({ length: 100 }, (_, index) => ({ id: `e-${index}`, revision: 1 as const }));
  const findings = [{ id: 'finding', revision: 1 as const, evidenceRevisionRefs: [evidence[0]!] }];
  expect(selectedHandoffIsClosed({ evidenceRevisionRefs: evidence, findingRevisionRefs: findings, decisionRevisionRefs: [] }, evidence, findings, [])).toBe(false);
});

test('record bodies from another attempt need exact current received snapshot membership', () => {
  const { recordsMatchTask } = require('../src/application/evidence-workspace');
  const task = { id: 'task-1', attemptId: 'attempt-1', contextId: 'context-1' };
  const own = { id: 'evidence-1', revision: 1, taskId: task.id, attemptId: task.attemptId, contextId: task.contextId };
  const old = { ...own, attemptId: 'older-attempt' };
  const records = { evidence: [old], findings: [], decisions: [] };
  expect(recordsMatchTask(task, records, undefined)).toBe(false);
  expect(recordsMatchTask(task, { ...records, evidence: [own] }, undefined)).toBe(true);
  expect(recordsMatchTask(task, records, { evidenceRevisionRefs: [{ id: old.id, revision: 1 }], findingRevisionRefs: [], decisionRevisionRefs: [] })).toBe(true);
  expect(recordsMatchTask(task, { ...records, evidence: [{ ...old, contextId: 'other-context' }] }, { evidenceRevisionRefs: [{ id: old.id, revision: 1 }], findingRevisionRefs: [], decisionRevisionRefs: [] })).toBe(false);
});
