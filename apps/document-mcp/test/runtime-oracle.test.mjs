import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const {guiOracle,assertSnapshotMatches,assertHistoryMatches,assertComparisons}=createRequire(import.meta.url)('../dist/oracle.cjs');
const snapshot={documentId:'regulation',title:'Synthetic',metadata:{changed:true},currentVersionId:'v3',revision:7,revisions:[{revisionId:'r3'},{revisionId:'r2'},{revisionId:'r1'}],versions:[{versionId:'v3',versionNo:3,files:[{mediaType:'text/plain'}]}],publications:[{sourceKey:'p3',actionCode:'document.version.published',actor:'poc-human',provenanceQuality:'operationLedger'}]};
const pdf={documentId:'pdf',currentVersionId:'p2',revisions:[{},{}],versions:[{versionId:'p2',versionNo:2,files:[{mediaType:'application/pdf'}]}]};
const state={documents:[{key:'regulation',snapshot},{key:'pdf',snapshot:pdf}]};
test('post-GUI oracle rejects missing PDF and seed-only or stale state',()=>{assert.throws(()=>guiOracle({documents:[state.documents[0]]},'regulation'));const stale=structuredClone(state);stale.documents[0].snapshot.versions[0].versionNo=2;assert.throws(()=>guiOracle(stale,'regulation'));assert.throws(()=>guiOracle(state,'wrong-document'));assert.equal(guiOracle(state,'regulation').regulation.currentVersionId,'v3');assert.throws(()=>assertSnapshotMatches(snapshot,{...snapshot,currentVersionId:'v2'},{items:snapshot.revisions,nextCursor:null}));});
test('oracle requires complete saved revision/publication history',()=>{assert.throws(()=>assertSnapshotMatches(snapshot,snapshot,{items:snapshot.revisions.slice(1),nextCursor:null}));assert.throws(()=>assertHistoryMatches(snapshot,{items:[],nextCursor:null}));assertSnapshotMatches(snapshot,snapshot,{items:snapshot.revisions,nextCursor:null});assertHistoryMatches(snapshot,{items:[{...snapshot.publications[0],actor:{principalId:'poc-human'}}],nextCursor:null});});
const revision={projection:'diff',baseRevision:{revisionId:'r1'},targetRevision:{revisionId:'r3'},verdict:'different',coverage:'full',resultDigest:'digest',changes:[{}],unverifiedRegions:[],auditEventId:'audit1'};
const version={projection:'display',verdict:'different',coverage:'full',resultDigest:'digest',items:[{}],unverifiedRegions:[],nextCursor:null,auditEventId:'audit2'};
test('comparison oracle rejects successful but degraded, empty or mismatched results',()=>{assert.throws(()=>assertComparisons({...revision,verdict:'unknown',coverage:'none'},version,revision,version,'r1','r3'));assert.throws(()=>assertComparisons(revision,{...version,items:[]},revision,version,'r1','r3'));assert.throws(()=>assertComparisons(revision,version,{...revision,resultDigest:'wrong'},version,'r1','r3'));assert.throws(()=>assertComparisons(revision,version,revision,version,'r2','r3'));assertComparisons(revision,version,{...revision,auditEventId:'new1'},{...version,auditEventId:'new2'},'r1','r3');});
test('published metadata oracle requires the exact metadataRevision and human operation history',()=>{const {assertMetadataUpdate}=createRequire(import.meta.url)('../dist/oracle.cjs');const before={revision:2,currentVersionId:'v1',displayRevision:{revisionId:'old',major:1,minor:0}};const after={revision:3,currentVersionId:'v1',metadata:{pocAgentObservation:'run'},displayRevision:{revisionId:'new',major:1,minor:1,sourceKind:'metadataRevision'}};const revisions={items:[after.displayRevision],nextCursor:null};const history={items:[{sourceKey:'management:op',actionCode:'document.metadata.changed',actor:{principalId:'poc-human'},provenanceQuality:'operationLedger',details:{changed:true,resulting_revision:3}}],nextCursor:null};assert.throws(()=>assertMetadataUpdate(before,{...after,displayRevision:{...after.displayRevision,sourceKind:'contentPublication'}},revisions,history,'run','op'));assert.throws(()=>assertMetadataUpdate(before,after,revisions,{items:[],nextCursor:null},'run','op'));assertMetadataUpdate(before,after,revisions,history,'run','op');});

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
