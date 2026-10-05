import type { CreateDocumentResult } from './document-workspace';
import { problemFromUnknown } from './problem-mapping';

// This is an unresolved local submission marker, never a document/authorization cache.
// Keep only generated recovery IDs; titles, binary files and principal data stay out of storage.
const receiptKey = 'knowledge-platform.document-registration';
export type CreationReceipt = { state: 'unknown' | 'created'; ids?: CreateDocumentResult };
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function creationIds(value: unknown): CreateDocumentResult | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const candidate = value as Partial<CreateDocumentResult>;
  if (![candidate.documentId, candidate.documentVersionId, candidate.fileId].every(id => typeof id === 'string' && uuid.test(id))) return undefined;
  return { documentId: candidate.documentId!, documentVersionId: candidate.documentVersionId!, fileId: candidate.fileId! };
}

export function readCreationReceipt(): CreationReceipt | null {
  try {
    const raw = window.sessionStorage.getItem(receiptKey);
    if (raw === null) return null;
    const value = JSON.parse(raw) as Partial<CreationReceipt> | null;
    const ids = creationIds(value?.ids);
    return { state: value?.state === 'created' && ids ? 'created' : 'unknown', ...(ids ? { ids } : {}) };
  } catch {
    // Storage failure must not silently remove a possible in-flight submission.
    return { state: 'unknown' };
  }
}

export function saveCreationReceipt(receipt: CreationReceipt): void {
  window.sessionStorage.setItem(receiptKey, JSON.stringify(receipt));
}

export function clearCreationReceipt(): void {
  window.sessionStorage.removeItem(receiptKey);
}

export function creationRecovery(error: unknown): CreateDocumentResult | undefined {
  const problem = problemFromUnknown(error);
  return problem?.code === 'COMMIT_OUTCOME_UNKNOWN' ? creationIds(problem.recovery) : undefined;
}

export function creationWasRejected(error: unknown): boolean {
  const problem = problemFromUnknown(error);
  if (!problem) return false;
  const rejected: Record<string, number> = {
    VALIDATION_FAILED: 422, AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403,
    FOLDER_NOT_FOUND: 404, UNSUPPORTED_MEDIA_TYPE: 415, BUSINESS_RULE_REJECTED: 422,
  };
  return rejected[problem.code] === problem.status;
}
