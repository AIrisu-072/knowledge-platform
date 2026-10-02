import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
const {guiOracle,assertSnapshotMatches,assertHistoryMatches,assertComparisons,metadataObservationPatch,assertMetadataUpdate}=createRequire(import.meta.url)('../dist/oracle.cjs');
const snapshot={documentId:'regulation',title:'Synthetic',metadata:{changed:true},currentVersionId:'v3',revision:7,revisions:[{revisionId:'r3'},{revisionId:'r2'},{revisionId:'r1'}],versions:[{versionId:'v3',versionNo:3,files:[{mediaType:'text/plain'}]}],publications:[{sourceKey:'p3',actionCode:'document.version.published',actor:'poc-human',provenanceQuality:'operationLedger'}]};
const pdf={documentId:'pdf',currentVersionId:'p2',revisions:[{},{}],versions:[{versionId:'p2',versionNo:2,files:[{mediaType:'application/pdf'}]}]};
const state={documents:[{key:'regulation',snapshot},{key:'pdf',snapshot:pdf}]};
test('post-GUI oracle rejects missing PDF and seed-only or stale state',()=>{assert.throws(()=>guiOracle({documents:[state.documents[0]]},'regulation'));const stale=structuredClone(state);stale.documents[0].snapshot.versions[0].versionNo=2;assert.throws(()=>guiOracle(stale,'regulation'));assert.throws(()=>guiOracle(state,'wrong-document'));assert.equal(guiOracle(state,'regulation').regulation.currentVersionId,'v3');assert.throws(()=>assertSnapshotMatches(snapshot,{...snapshot,currentVersionId:'v2'},{items:snapshot.revisions,nextCursor:null}));});
test('oracle requires complete saved revision/publication history',()=>{assert.throws(()=>assertSnapshotMatches(snapshot,snapshot,{items:snapshot.revisions.slice(1),nextCursor:null}));assert.throws(()=>assertHistoryMatches(snapshot,{items:[],nextCursor:null}));assertSnapshotMatches(snapshot,snapshot,{items:snapshot.revisions,nextCursor:null});assertHistoryMatches(snapshot,{items:[{...snapshot.publications[0],actor:{principalId:'poc-human'}}],nextCursor:null});});
const revision={projection:'diff',baseRevision:{revisionId:'r1'},targetRevision:{revisionId:'r3'},verdict:'different',coverage:'full',resultDigest:'digest',changes:[{}],unverifiedRegions:[],auditEventId:'audit1'};
const version={projection:'display',verdict:'different',coverage:'full',resultDigest:'digest',items:[{}],unverifiedRegions:[],nextCursor:null,auditEventId:'audit2'};
test('comparison oracle rejects successful but degraded, empty or mismatched results',()=>{assert.throws(()=>assertComparisons({...revision,verdict:'unknown',coverage:'none'},version,revision,version,'r1','r3'));assert.throws(()=>assertComparisons(revision,{...version,items:[]},revision,version,'r1','r3'));assert.throws(()=>assertComparisons(revision,version,{...revision,resultDigest:'wrong'},version,'r1','r3'));assert.throws(()=>assertComparisons(revision,version,revision,version,'r2','r3'));assertComparisons(revision,version,{...revision,auditEventId:'new1'},{...version,auditEventId:'new2'},'r1','r3');});
test('published metadata oracle requires the exact metadataRevision and human operation history',()=>{const {assertMetadataUpdate}=createRequire(import.meta.url)('../dist/oracle.cjs');const before={revision:2,currentVersionId:'v1',metadata:{},displayRevision:{revisionId:'old',major:1,minor:0}};const after={revision:3,currentVersionId:'v1',metadata:{extensions:{pocAgentObservation:'run'}},displayRevision:{revisionId:'new',major:1,minor:1,sourceKind:'metadataRevision'}};const revisions={items:[after.displayRevision],nextCursor:null};const history={items:[{sourceKey:'management:op',actionCode:'document.metadata.changed',actor:{principalId:'poc-human'},provenanceQuality:'operationLedger',details:{changed:true,resulting_revision:3}}],nextCursor:null};assert.throws(()=>assertMetadataUpdate(before,{...after,displayRevision:{...after.displayRevision,sourceKind:'contentPublication'}},revisions,history,'run','op'));assert.throws(()=>assertMetadataUpdate(before,after,revisions,{items:[],nextCursor:null},'run','op'));assertMetadataUpdate(before,after,revisions,history,'run','op');});

// Harness contracts only: the normative management boundary and actual repository/HTTP
// projection define these expectations; no simulated result qualifies real acceptance.
const metadataBefore = {
  revision: 2, currentVersionId: 'v1',
  metadata: { category: 'synthetic', legacy_key: { must: 'stay' }, extensions: { retained: { nested: [1, true] }, pocAgentObservation: 'previous-run' } },
  displayRevision: { revisionId: 'old', major: 1, minor: 0 },
};
const metadataAfter = {
  ...metadataBefore, revision: 3,
  metadata: { ...metadataBefore.metadata, extensions: { ...metadataBefore.metadata.extensions, pocAgentObservation: 'run' } },
  displayRevision: { revisionId: 'new', major: 1, minor: 1, sourceKind: 'metadataRevision' },
};
const metadataRevisions = { items: [metadataAfter.displayRevision], nextCursor: null };
const metadataHistory = { items: [{ sourceKey: 'management:op', actionCode: 'document.metadata.changed', actor: { principalId: 'poc-human' }, provenanceQuality: 'operationLedger', details: { changed: true, resulting_revision: 3 } }], nextCursor: null };
const checkMetadata = after => assertMetadataUpdate(metadataBefore, after, metadataRevisions, metadataHistory, 'run', 'op');

test('synthetic metadata PATCH uses the allowed extensions object and preserves its existing keys', () => {
  // CommandsMetadataPatch.set is an open map in the generated schema. Its business
  // allowlist is defined by the existing management validator and normative spec.
  const validator = readFileSync(new URL('../../../crates/document-application/src/management_digest.rs', import.meta.url), 'utf8');
  const allowed = [...validator.match(/const ALLOWED: \[&str; 4\] = \[([^\]]+)\]/s)[1].matchAll(/"([^"]+)"/g)].map(match => match[1]);
  assert.deepEqual(allowed, ['document_type', 'owning_department', 'category', 'extensions']);
  const before = structuredClone(metadataBefore);
  const patch = metadataObservationPatch(before, 'run', 'op');
  assert.deepEqual(patch, {
    operationId: 'op', expectedDocumentRevision: 2,
    set: { extensions: { retained: { nested: [1, true] }, pocAgentObservation: 'run' } },
    unset: [], reason: 'Synthetic Human to Agent consistency acceptance',
  });
  assert.ok(Object.keys(patch.set).every(key => allowed.includes(key)));
  assert.deepEqual(before, metadataBefore, 'constructing a request must not mutate the saved pre-update state');
  assert.deepEqual(metadataObservationPatch({ revision: 2, metadata: {} }, 'run', 'op').set, { extensions: { pocAgentObservation: 'run' } });
});

test('metadata oracle accepts the exact nested marker with retained metadata and revision/history evidence', () => {
  checkMetadata(metadataAfter);
});

test('metadata oracle rejects a top-level marker or a stale nested marker even if the top-level marker matches', () => {
  assert.throws(() => checkMetadata({ ...metadataAfter, metadata: { ...metadataBefore.metadata, pocAgentObservation: 'run' } }));
  assert.throws(() => checkMetadata({ ...metadataAfter, metadata: { pocAgentObservation: 'run' } }));
  assert.throws(() => checkMetadata({ ...metadataAfter, metadata: { ...metadataAfter.metadata, extensions: { ...metadataAfter.metadata.extensions, pocAgentObservation: 'other-run' } } }));
});

test('metadata oracle rejects loss of existing extension or unrelated metadata keys', () => {
  assert.throws(() => checkMetadata({ ...metadataAfter, metadata: { ...metadataAfter.metadata, extensions: { pocAgentObservation: 'run' } } }));
  const missingLegacy = structuredClone(metadataAfter);
  delete missingLegacy.metadata.legacy_key;
  assert.throws(() => checkMetadata(missingLegacy));
});


const sharedState = (major, minor, revisionId, versionId, sourceKind, revision = 2, previous = []) => {
  const issued = { revisionId, documentVersionId: versionId, major, minor, label: `${major}.${minor}`, sourceKind };
  return { detail: { documentId: 'document', currentVersionId: versionId, documentVersionId: versionId, revision, title: 'Synthetic', metadata: { stage: 'initial' }, displayRevision: issued, displayVersion: { versionId, versionNo: 1 } }, revisions: { items: [issued, ...previous], nextCursor: null }, files: [{ versionId, files: { items: [{ contentItemId: 'item', representationId: 'file', sizeBytes: 42 }] } }] };
};
test('ordered transition oracle pins exact IDs, major/minor, OCC result and append-only history', () => {
  const { assertRevisionTransition } = createRequire(import.meta.url)('../dist/oracle.cjs');
  const first = sharedState(1, 0, 'r1', 'v1', 'initialPublication');
  const expected = { sourceKind: 'initialPublication', versionId: 'v1', major: 1, minor: 0, resultingRevision: 2, metadata: { stage: 'initial' } };
  assertRevisionTransition(undefined, first, expected);
  for (const [field, value] of [['major', 2], ['minor', 1], ['sourceKind', 'legacyBackfill'], ['versionId', 'wrong'], ['resultingRevision', 99]]) {
    assert.throws(() => assertRevisionTransition(undefined, first, { ...expected, [field]: value }), field);
  }
  const minor = sharedState(1, 1, 'r2', 'v1', 'metadataRevision', 3, first.revisions.items);
  assertRevisionTransition(first, minor, { sourceKind: 'metadataRevision', versionId: 'v1', major: 1, minor: 1, resultingRevision: 3, metadata: { stage: 'initial' } });
  const reused = structuredClone(minor); reused.revisions.items[0].revisionId = 'r1'; reused.detail.displayRevision = reused.revisions.items[0];
  assert.throws(() => assertRevisionTransition(first, reused, { sourceKind: 'metadataRevision', versionId: 'v1', major: 1, minor: 1, resultingRevision: 3, metadata: { stage: 'initial' } }));
  const changedHistory = structuredClone(minor); changedHistory.revisions.items[1].label = 'wrong';
  assert.throws(() => assertRevisionTransition(first, changedHistory, { sourceKind: 'metadataRevision', versionId: 'v1', major: 1, minor: 1, resultingRevision: 3, metadata: { stage: 'initial' } }));
});
test('shared-state equality excludes actor-local read state but never loses IDs, metadata or file summaries', () => {
  const { sharedDetail, assertSharedState } = createRequire(import.meta.url)('../dist/oracle.cjs');
  const first = sharedState(1, 0, 'r1', 'v1', 'initialPublication');
  assert.deepEqual(sharedDetail({ ...first.detail, readState: 'read', capabilities: {} }), sharedDetail({ ...first.detail, readState: 'unread' }));
  assertSharedState(first, structuredClone(first));
  for (const mutate of [s => { s.detail.metadata.stage = 'stale'; }, s => { s.detail.currentVersionId = 'v0'; }, s => { s.revisions.items.pop(); }, s => { s.files[0].files.items[0].representationId = 'wrong'; }, s => { s.files[0].files.items[0].sizeBytes++; }]) {
    const stale = structuredClone(first); mutate(stale); assert.throws(() => assertSharedState(first, stale));
  }
});
test('no-op and interrupted-operation replay cannot add a revision or alter any shared state', () => {
  const { assertNoopState, assertMutationReplay } = createRequire(import.meta.url)('../dist/oracle.cjs');
  const before = sharedState(1, 1, 'r2', 'v1', 'metadataRevision', 3, [{ revisionId: 'r1' }]);
  assertNoopState(before, structuredClone(before), { changed: false, resultingRevision: 3 });
  assert.throws(() => assertNoopState(before, before, { changed: true, resultingRevision: 3 }));
  const changed = structuredClone(before); changed.revisions.items.unshift({ revisionId: 'extra' });
  assert.throws(() => assertNoopState(before, changed, { changed: false, resultingRevision: 3 }));
  const result = { operationId: 'same', resourceId: 'document', resultingRevision: 3, changed: true, occurredAt: 'same-time' };
  const history = { items: [{ sourceKey: 'management:same', actionCode: 'document.metadata.changed', details: { changed: true, resulting_revision: 3 }, actor: { principalId: 'poc-human' }, provenanceQuality: 'operationLedger' }], nextCursor: null };
  assertMutationReplay(before, before, result, result, history);
  assert.throws(() => assertMutationReplay(before, changed, result, result, history));
  assert.throws(() => assertMutationReplay(before, before, result, { ...result, operationId: 'new' }, history));
  assert.throws(() => assertMutationReplay(before, before, result, result, { ...history, items: [...history.items, ...history.items] }));
});
test('ordered transition rejects unchanged or wrong metadata even when all channel snapshots agree', () => {
  const { assertRevisionTransition } = createRequire(import.meta.url)('../dist/oracle.cjs');
  const before = sharedState(1, 0, 'r1', 'v1', 'initialPublication');
  const falseChange = sharedState(1, 1, 'r2', 'v1', 'metadataRevision', 3, before.revisions.items);
  const expected = { sourceKind: 'metadataRevision', versionId: 'v1', major: 1, minor: 1, resultingRevision: 3, metadata: { stage: 'requested-new-value' } };
  assert.throws(() => assertRevisionTransition(before, falseChange, expected), /metadata/);
  const correct = structuredClone(falseChange); correct.detail.metadata = expected.metadata;
  assertRevisionTransition(before, correct, expected);
});
