import type { AccessPolicyRead, CommandsPolicyExplicit, CommandsPolicyInherit, FolderDetail, MutationResult, PolicyGrantInput } from '@knowledge-platform/document-api-client';
import { rootFolderValidation, validOccurredAt, wasRejected, type SelectedFolderContext } from './document-root-folder';
import { problemFromUnknown } from './problem-mapping';

export const policyActions = ['read', 'readHistory', 'write', 'publish', 'administer'] as const;
export type FolderPolicyRequest = CommandsPolicyExplicit | CommandsPolicyInherit;
export type FolderPolicySnapshot = { folder: FolderDetail; policy: AccessPolicyRead };
export const policySubjectKey = (grant: Pick<PolicyGrantInput, 'subjectKind' | 'identityProvider' | 'subjectId'>) => JSON.stringify([grant.subjectKind, grant.identityProvider, grant.subjectId]);
export const policySubjectLabel = (grant: PolicyGrantInput) => `${grant.subjectKind} / ${grant.identityProvider} / ${grant.subjectId}`;
export function policyGrantInput(grant: PolicyGrantInput): PolicyGrantInput {
  return { subjectKind: grant.subjectKind, identityProvider: grant.identityProvider, subjectId: grant.subjectId, actions: [...grant.actions] };
}
function grantSet(grants: readonly PolicyGrantInput[]) {
  return JSON.stringify(grants.map(grant => [policySubjectKey(grant), [...grant.actions].sort()]).sort((a, b) => String(a[0]) < String(b[0]) ? -1 : String(a[0]) > String(b[0]) ? 1 : 0));
}
export function canManageFolderPolicy(folder: FolderDetail | undefined, rootId?: string): folder is FolderDetail {
  return Boolean(folder?.folderId && folder.folderId !== rootId && folder.parentFolderId && Number.isSafeInteger(folder.revision) && folder.revision >= 0 && folder.capabilities?.manageAccess?.status === 'available');
}
export function validateFolderPolicy(policy: AccessPolicyRead | undefined, targetId: string): policy is AccessPolicyRead {
  const identity = (value: unknown) => typeof value === 'string' && Boolean(value.trim()) && !/[\p{Cc}\p{Cs}]/u.test(value);
  return Boolean(policy && policy.target?.kind === 'folder' && policy.target.id === targetId
    && ['inherit', 'explicit'].includes(policy.bindingMode) && Number.isSafeInteger(policy.policyRevision) && policy.policyRevision >= 0
    && (policy.policyId === null || identity(policy.policyId)) && identity(policy.effectivePolicyId)
    && policy.effectiveSource?.kind === 'folder' && identity(policy.effectiveSource.id)
    && Array.isArray(policy.effectiveGrants) && policy.effectiveGrants.length > 0
    && policy.effectiveGrants.every(grant => grant && ['principal', 'group', 'role'].includes(grant.subjectKind) && identity(grant.identityProvider) && identity(grant.subjectId)
      && Array.isArray(grant.actions) && grant.actions.length > 0 && new Set(grant.actions).size === grant.actions.length && grant.actions.every(action => policyActions.includes(action)))
    && new Set(policy.effectiveGrants.map(policySubjectKey)).size === policy.effectiveGrants.length);
}
export function policySnapshotEqual(a: FolderPolicySnapshot, b: FolderPolicySnapshot): boolean {
  return a.folder.folderId === b.folder.folderId && a.folder.parentFolderId === b.folder.parentFolderId && a.folder.revision === b.folder.revision && a.folder.name === b.folder.name
    && a.policy.target.kind === b.policy.target.kind && a.policy.target.id === b.policy.target.id && a.policy.bindingMode === b.policy.bindingMode
    && a.policy.policyId === b.policy.policyId && a.policy.policyRevision === b.policy.policyRevision && a.policy.effectivePolicyId === b.policy.effectivePolicyId
    && a.policy.effectiveSource.kind === b.policy.effectiveSource.kind && a.policy.effectiveSource.id === b.policy.effectiveSource.id
    && grantSet(a.policy.effectiveGrants) === grantSet(b.policy.effectiveGrants);
}
export function policyChanged(baseline: AccessPolicyRead, request: FolderPolicyRequest): boolean {
  return baseline.bindingMode !== request.mode || request.mode === 'explicit' && grantSet(baseline.effectiveGrants) !== grantSet(request.grants);
}
export function policyRevisionError(revision: number, changed: boolean): string | null {
  return !Number.isSafeInteger(revision) || revision < 0 || changed && revision >= Number.MAX_SAFE_INTEGER ? 'policy revisionの次の値を安全に扱えません。管理者へ確認してください。' : null;
}
export function policyReasonValidation(reason: string): string | null { return rootFolderValidation('資料', reason, 'アクセス設定の変更理由'); }
export type FolderAccessPolicyOperation = {
  targetFolderId: string; context: SelectedFolderContext; request: Readonly<FolderPolicyRequest>; expectedChanged: boolean;
  status: 'pending' | 'unknown' | 'rejected' | 'succeeded'; error?: unknown; result?: MutationResult; refresh?: 'pending' | 'complete' | 'failed';
};
const stores = new WeakMap<object, ReturnType<typeof createStore>>();
function createStore() {
  let operation: FolderAccessPolicyOperation | undefined;
  const listeners = new Set<() => void>();
  const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
  const emit = () => { window.removeEventListener('beforeunload', warn); if (operation?.status === 'pending' || operation?.status === 'unknown') window.addEventListener('beforeunload', warn); listeners.forEach(listener => listener()); };
  return {
    get: () => operation,
    put: (next: FolderAccessPolicyOperation) => { operation = next; emit(); },
    clearSettled: (expected: FolderAccessPolicyOperation) => { if (operation !== expected || operation.status !== 'rejected' && operation.status !== 'succeeded') return false; operation = undefined; emit(); return true; },
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}
export function folderAccessPolicyOperations(owner: object) { let store = stores.get(owner); if (!store) { store = createStore(); stores.set(owner, store); } return store; }
function freezeRequest(request: FolderPolicyRequest): Readonly<FolderPolicyRequest> {
  if (request.mode === 'inherit') return Object.freeze({ operationId: request.operationId, expectedPolicyRevision: request.expectedPolicyRevision, reason: request.reason, mode: request.mode });
  const grants = request.grants.map(grant => { const fixed = policyGrantInput(grant); Object.freeze(fixed.actions); return Object.freeze(fixed); });
  Object.freeze(grants); return Object.freeze({ operationId: request.operationId, expectedPolicyRevision: request.expectedPolicyRevision, reason: request.reason, mode: request.mode, grants });
}
export async function sendFolderAccessPolicyOperation(input: {
  store: ReturnType<typeof folderAccessPolicyOperations>; targetFolderId: string; context: SelectedFolderContext; request: FolderPolicyRequest; baseline?: AccessPolicyRead;
  send: (folderId: string, request: FolderPolicyRequest) => Promise<MutationResult>;
  invalidate: (operation: FolderAccessPolicyOperation) => Promise<unknown>;
}): Promise<void> {
  const { store, send, invalidate } = input; const previous = store.get();
  if (previous && previous.status !== 'unknown') return;
  const expectedChanged = previous?.expectedChanged ?? (input.baseline ? policyChanged(input.baseline, input.request) : true);
  if (!previous) { const error = policyRevisionError(input.request.expectedPolicyRevision, expectedChanged); if (error) throw new Error(error); }
  const fixed = previous ?? { targetFolderId: input.targetFolderId, context: Object.freeze({ ...input.context }), request: freezeRequest(input.request), expectedChanged };
  store.put({ ...fixed, status: 'pending', error: undefined });
  let confirmed: FolderAccessPolicyOperation;
  try {
    const result = await send(fixed.targetFolderId, fixed.request);
    if (!result || result.operationId !== fixed.request.operationId || result.resourceId !== fixed.targetFolderId || result.changed !== fixed.expectedChanged
      || !Number.isSafeInteger(result.resultingRevision) || result.resultingRevision !== fixed.request.expectedPolicyRevision + (fixed.expectedChanged ? 1 : 0) || !validOccurredAt(result.occurredAt)) throw new Error('アクセス設定の結果を照合できません。');
    confirmed = { ...fixed, status: 'succeeded', result, error: undefined, refresh: 'pending' }; store.put(confirmed);
  } catch (error) { store.put({ ...fixed, status: !previous && wasRejected(error) && !problemFromUnknown(error)?.exactRetry ? 'rejected' : 'unknown', error }); return; }
  let refresh: FolderAccessPolicyOperation['refresh'] = 'complete';
  try { await invalidate(confirmed); } catch { refresh = 'failed'; }
  if (store.get() === confirmed) store.put({ ...confirmed, refresh });
}
