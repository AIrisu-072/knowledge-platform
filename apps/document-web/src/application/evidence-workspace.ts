import type { DocumentDetail, FileList } from '@knowledge-platform/document-api-client';
import type { RevisionRef } from './work-workspace';
export type { RevisionRef } from './work-workspace';
export type SelectedHandoff = { evidenceRevisionRefs: RevisionRef[]; findingRevisionRefs: RevisionRef[]; decisionRevisionRefs: RevisionRef[] };
type SupportedFinding = RevisionRef & { evidenceRevisionRefs: RevisionRef[] };
type SupportedDecision = RevisionRef & { findingId: string; findingRevision: 1; evidenceRevisionRefs: RevisionRef[] };
const contains = (items: RevisionRef[], ref: RevisionRef) => items.some((item) => item.id === ref.id && item.revision === ref.revision);
export function selectedHandoffIsClosed(selection: SelectedHandoff, evidence: RevisionRef[], findings: SupportedFinding[], decisions: SupportedDecision[]): boolean {
  const { evidenceRevisionRefs: e, findingRevisionRefs: f, decisionRevisionRefs: d } = selection;
  if (e.length + f.length + d.length > 100 || [e, f, d].some((refs) => refs.length > 100 || new Set(refs.map((ref) => ref.id)).size !== refs.length)) return false;
  return e.every((ref) => contains(evidence, ref)) && f.every((ref) => {
    const finding = findings.find((item) => contains([item], ref));
    return finding && finding.evidenceRevisionRefs.every((support) => contains(e, support));
  }) && d.every((ref) => {
    const decision = decisions.find((item) => contains([item], ref));
    return decision && contains(f, { id: decision.findingId, revision: decision.findingRevision }) && decision.evidenceRevisionRefs.every((support) => contains(e, support));
  });
}
export function sourceFromPublishedDocument(documentId: string, document: DocumentDetail, file: FileList['items'][number]) {
  const revision = document.displayRevision;
  if (document.documentId !== documentId || !revision || revision.documentVersionId !== document.documentVersionId || file.role !== 'AUTHORITATIVE') return null;
  return { sourceRef: { providerId: 'document' as const, resourceId: documentId, revisionId: revision.revisionId, versionId: revision.documentVersionId }, authoritativeLocator: { kind: 'contentItem' as const, contentItemId: file.contentItemId, representationId: file.representationId } };
}

export function toggleReference(items: RevisionRef[], ref: RevisionRef, selected: boolean): RevisionRef[] {
  return selected ? contains(items, ref) ? items : [...items, { id: ref.id, revision: ref.revision }] : items.filter((item) => item.id !== ref.id);
}

export type { EvidenceRecord, Finding, HumanDecision } from './work-workspace';

type ScopedRecord = { id: string; revision: number; taskId: string; attemptId: string; contextId: string };
export function recordsMatchTask(task: { id: string; attemptId: string; contextId: string }, records: { evidence: ScopedRecord[]; findings: ScopedRecord[]; decisions: ScopedRecord[] }, snapshot?: SelectedHandoff): boolean {
  const allowed = (items: ScopedRecord[], selected: RevisionRef[] = []) => items.every((item) => item.contextId === task.contextId && ((item.taskId === task.id && item.attemptId === task.attemptId) || selected.some((ref) => ref.id === item.id && ref.revision === item.revision)));
  return allowed(records.evidence, snapshot?.evidenceRevisionRefs) && allowed(records.findings, snapshot?.findingRevisionRefs) && allowed(records.decisions, snapshot?.decisionRevisionRefs);
}
