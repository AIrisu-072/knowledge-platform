import type { ReadStateMutationResult } from '@knowledge-platform/document-api-client';
import { documentReadStateOperations, sendReadStateOperation, validCurrentReadState, type ReadStateIntent } from '../src/application/document-read-state';
const instant = '2026-10-07T00:00:00Z';
const intent: ReadStateIntent = { documentId: 'doc', versionId: 'version', title: '固定した公開文書', versionNo: 1, kind: 'VIEW', body: { operationId: '019a0107-0000-7000-8000-000000000020', expectedReadStateRevision: 0 } };
function receipt(fixed: ReadStateIntent = intent): ReadStateMutationResult { return { operationId: fixed.body.operationId, documentId: fixed.documentId, versionId: fixed.versionId, kind: fixed.kind, expectedReadStateRevision: fixed.body.expectedReadStateRevision, changed: true, occurredAt: instant, resultingReadState: { firstReadAt: instant, needsRecheck: fixed.kind === 'RESET', isRead: fixed.kind === 'VIEW', readStateRevision: fixed.body.expectedReadStateRevision + 1 } }; }
const problem = (status: number, code: string) => ({ type: 'about:blank', title: 'Synthetic', status, code, traceId: 'synthetic', retryable: false });

test('pending_and_deeply_fixed_intent_are_recorded_before_the_first_await', async () => {
  const store = documentReadStateOperations({}); let resolve!: (result: ReadStateMutationResult) => void;
  const sending = sendReadStateOperation({ store, intent, send: () => new Promise(r => { resolve = r; }), invalidate: async () => {} });
  const pending = store.get('doc', 'version'); expect(pending?.status).toBe('pending'); expect(Object.isFrozen(pending!.intent)).toBe(true); expect(Object.isFrozen(pending!.intent.body)).toBe(true);
  expect(store.clearSettled('doc', 'version', pending!)).toBe(false); resolve(receipt()); await sending; expect(store.get('doc', 'version')?.status).toBe('succeeded');
});
test('unknown_reuses_original_path_body_title_and_version_when_the_proposal_changes', async () => {
  const store = documentReadStateOperations({}); const sent: ReadStateIntent[] = [];
  await sendReadStateOperation({ store, intent, send: async fixed => { sent.push(fixed); throw new Error('response lost'); }, invalidate: async () => {} });
  const original = store.get('doc', 'version')!.intent;
  await sendReadStateOperation({ store, intent: { ...intent, title: '変更された題名', body: { operationId: 'new', expectedReadStateRevision: 8 } }, send: async fixed => { sent.push(fixed); return receipt(fixed); }, invalidate: async () => {} });
  expect(sent).toEqual([original, original]); expect(sent[0]).toBe(sent[1]); expect(store.get('doc', 'version')?.status).toBe('succeeded');
});
test.each([[401, 'AUTHENTICATION_REQUIRED'], [403, 'FORBIDDEN'], [404, 'DOCUMENT_NOT_FOUND'], [409, 'REVISION_CONFLICT'], [422, 'BUSINESS_RULE_REJECTED']] as const)('first_%i_is_rejected_but_unknown_then_rejection_stays_unknown', async (status, code) => {
  const store = documentReadStateOperations({}); const reject = async () => { throw problem(status, code); }; const invalidate = async () => {};
  await sendReadStateOperation({ store, intent, send: reject, invalidate }); const rejected = store.get('doc', 'version')!; expect(rejected.status).toBe('rejected'); expect(store.clearSettled('doc', 'version', rejected)).toBe(true);
  await sendReadStateOperation({ store, intent, send: async () => { throw new Error('lost'); }, invalidate }); await sendReadStateOperation({ store, intent, send: reject, invalidate }); expect(store.get('doc', 'version')?.status).toBe('unknown');
});
test.each(['document', 'version', 'operation', 'kind', 'expected', 'first', 'delta', 'flag', 'date', 'unsafe'] as const)('malformed_%s_receipt_is_unknown', async field => {
  const store = documentReadStateOperations({}); const result = receipt();
  if (field === 'document') result.documentId = 'other'; if (field === 'version') result.versionId = 'other'; if (field === 'operation') result.operationId = 'other'; if (field === 'kind') result.kind = 'RESET'; if (field === 'expected') result.expectedReadStateRevision = 3;
  if (field === 'first') (result.resultingReadState as { firstReadAt: string | null }).firstReadAt = null; if (field === 'delta') result.resultingReadState.readStateRevision = 3; if (field === 'flag') result.resultingReadState.needsRecheck = true; if (field === 'date') result.occurredAt = '2026-02-30T00:00:00Z'; if (field === 'unsafe') result.resultingReadState.readStateRevision = Number.MAX_SAFE_INTEGER + 1;
  await sendReadStateOperation({ store, intent, send: async () => result, invalidate: async () => {} }); expect(store.get('doc', 'version')?.status).toBe('unknown');
});
test('verified_success_read_failure_does_not_send_the_mutation_again', async () => {
  const store = documentReadStateOperations({}); let posts = 0;
  const request = { store, intent, send: async () => { posts++; return receipt(); }, invalidate: async () => { throw new Error('current read unavailable'); } };
  await sendReadStateOperation(request); await sendReadStateOperation(request); expect(posts).toBe(1); expect(store.get('doc', 'version')).toMatchObject({ status: 'succeeded', refresh: 'failed' });
});
test('unresolved_intent_cannot_be_replaced_or_cleared', async () => {
  const owner = {}; const store = documentReadStateOperations(owner);
  await sendReadStateOperation({ store, intent, send: async () => { throw new Error('lost'); }, invalidate: async () => {} }); const fixed = store.get('doc', 'version')!;
  store.put({ ...fixed, intent: { ...intent, body: { ...intent.body, operationId: 'replacement' } } }); expect(store.get('doc', 'version')).toBe(fixed); expect(store.clearSettled('doc', 'version', fixed)).toBe(false); expect(store.listUnresolved()).toEqual([fixed]); expect(documentReadStateOperations(owner)).toBe(store);
});
test('current_snapshot_requires_consistent_virtual_state_target_safe_integer_and_date', () => {
  const virtual = { documentId: 'doc', versionId: 'version', firstReadAt: null, needsRecheck: false, isRead: false, readStateRevision: 0 }; expect(validCurrentReadState(virtual, 'doc', 'version')).toBe(true);
  for (const invalid of [{ ...virtual, needsRecheck: true }, { ...virtual, isRead: true }, { ...virtual, readStateRevision: 1 }, { ...virtual, readStateRevision: 0.5 }, { ...virtual, readStateRevision: Number.MAX_SAFE_INTEGER + 1 }, { ...virtual, documentId: 'other' }]) expect(validCurrentReadState(invalid, 'doc', 'version')).toBe(false);
});
