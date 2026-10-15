import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtemp, readFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {Journal} from '../journal.mjs';

const {exerciseNegative, verifyNegativeRetained, sanitizeNegativeFailureCode} = await import('../negative.mjs').catch(error => {
  if (error.code !== 'ERR_MODULE_NOT_FOUND') throw error;
  return {};
});
const HUMAN = {subjectKind:'group', identityProvider:'poc', subjectId:'poc-users', actions:['read','readHistory','write','publish','administer']};
const FAILURE = {operation:'publish', httpStatus:422, problemCode:'BUSINESS_RULE_REJECTED'};
const binding = () => ({status:'observed', fileCount:1, versionCount:1, authoritativeItemCount:1, isWorking:true, requiresContentClassification:false, mediaTypeMatches:true, rawHashMatches:true, sizeMatches:true});
const rejection = (diagnostic = FAILURE, status = 422) => Object.assign(Error('PRIVATE_SENTINEL'), {status, diagnostic:{...diagnostic, detail:'PRIVATE_SENTINEL', body:'PRIVATE_SENTINEL'}});
function asset(id = 'negative-original') {
  const bytes = Buffer.from(`%PDF-1.7\n${id}`);
  return {id, bytes, sha256:createHash('sha256').update(bytes).digest('hex'), mediaType:'application/pdf', expectedOutcome:'reject-unsupported'};
}
function fake() {
  let next = 0;
  const docs = new Map(), calls = [], policies = new Map();
  return {
    docs, calls, policies,
    verifySessions:async () => {calls.push('sessions'); return {rootFolderId:'root', root:{revision:0}};},
    createFolder:async request => {calls.push('folder'); return {resourceId:request.folderId};},
    setFolderPolicy:async (id, request) => {calls.push('policy'); policies.set(id, structuredClone(request)); return {resultingRevision:1};},
    create:async input => {
      calls.push('create'); const n = ++next;
      const tuple = {documentId:`document-${n}`, documentVersionId:`version-${n}`, fileId:`file-${n}`};
      const file = {contentItemId:`item-${n}`, representationId:`representation-${n}`, logicalPath:'primary', ordinal:0, role:'authoritative', displayName:input.filename, mediaType:input.mediaType, sizeBytes:input.bytes.length, downloadedBytes:input.bytes.length, sha256:createHash('sha256').update(input.bytes).digest('hex')};
      const version = {versionId:tuple.documentVersionId, lifecycleState:'working', isCurrent:false, publishedAt:null};
      docs.set(tuple.documentId, {
        detail:{documentId:tuple.documentId, documentVersionId:tuple.documentVersionId, folderId:input.folderId, currentVersionId:null, lifecycleState:'working', revision:0, metadata:{syntheticFixture:'document-load-qualification-v1'}},
        policy:{bindingMode:'inherit', policyRevision:0, effectiveSource:{kind:'folder',id:input.folderId}, effectiveGrants:[{...HUMAN, presentation:{displayName:'PoC users'}}]},
        revisions:[], versions:[{summary:structuredClone(version), detail:structuredClone(version), files:[file]}],
      });
      return tuple;
    },
    snapshot:async (id, options) => {assert.deepEqual(options,{purpose:'authoring'}); calls.push('snapshot'); return structuredClone(docs.get(id));},
    publish:async () => {calls.push('publish'); throw rejection();},
    list:async (folderId, who = 'human') => {calls.push(`list-${who}`); return who === 'agent' ? [] : [...docs].filter(([,value]) => value.detail.folderId === folderId).map(([id]) => id);},
  };
}
async function context(t, assets = [asset()]) {
  assert.equal(typeof exerciseNegative, 'function', 'negative qualification is not implemented');
  assert.equal(typeof verifyNegativeRetained, 'function', 'negative restart verification is not implemented');
  const directory = await mkdtemp(join(tmpdir(),'negative-qualification-'));
  const journal = await Journal.open(join(directory,'operations.jsonl'), {run:'negative-test'});
  const probe = fake();
  const c = {
    probe, journal, directory, assets, runId:'test', checkpoint:async () => {probe.calls.push('checkpoint');},
    diagnosePublication:async () => {probe.calls.push('inspection'); return {rowCount:0, detail:'PRIVATE_SENTINEL'};},
    diagnoseWorker:async () => {probe.calls.push('worker'); return {status:'worker-failure', failureCode:'unsupported_semantic_construct', qualification:false, binding:binding(), reason:'PRIVATE_SENTINEL'};},
  };
  t.after(async () => {await c.journal.close(); await rm(directory,{recursive:true,force:true});});
  return c;
}

test('negative positive control records the real rejection separately and proves retained originals', async t => {
  const c = await context(t, [asset('first'),asset('second')]);
  const evidence = await exerciseNegative(c);
  assert.equal(evidence.status,'AWAITING_RESTART');
  assert.deepEqual(evidence.counts,{targetDocuments:2,confirmedCreatedDocuments:2,confirmedRejectedDocuments:2,confirmedPublishedDocuments:0,confirmedHttp422Responses:2});
  assert.deepEqual(c.probe.policies.get(evidence.folderId).grants,[HUMAN]);
  assert.equal(evidence.documents.length,2);
  for (const [index, item] of evidence.documents.entries()) {
    assert.equal(item.assetId,c.assets[index].id);
    assert.equal(item.sha256,c.assets[index].sha256);
    assert.deepEqual(item.failureDiagnostic,FAILURE);
    assert.deepEqual(item.inspectionDiagnostic,{status:'not-found'});
    assert.deepEqual(item.workerDiagnostic,{status:'worker-failure',failureCode:'unsupported_semantic_construct',qualification:false});
    assert.deepEqual(item.publicationPrerequisites,binding());
    assert.equal(item.snapshot.detail.currentVersionId,null);
    assert.equal(item.snapshot.versions[0].files[0].sha256,c.assets[index].sha256);
    assert.deepEqual(c.journal.get(`negative-publish:${index}`).result,{outcome:'rejected-unsupported',failureDiagnostic:FAILURE});
    assert.equal(c.journal.get(`negative-create:${index}`).recoverable,false);
  }
  assert.ok([...c.journal.states.keys()].every(key => key.startsWith('negative-')));
  assert.deepEqual(await verifyNegativeRetained({...c,evidence}),{status:'SUCCEEDED',verifiedDocuments:2,snapshotCount:2,confirmedPublishedDocuments:0});
});

test('empty negative corpus is explicitly NOT_RUN without touching runtime or journal', async t => {
  const c = await context(t, []), evidence = await exerciseNegative(c);
  assert.equal(evidence.status,'NOT_RUN'); assert.equal(c.probe.calls.length,0); assert.equal(c.journal.states.size,0);
  assert.equal(evidence.counts.confirmedHttp422Responses,0);
  assert.equal((await verifyNegativeRetained({...c,evidence})).status,'NOT_RUN');
});

for (const [name, change] of [
  ['more than twenty assets', c => {c.assets = Array.from({length:21},(_,i) => asset(`a${i}`));}],
  ['a positive expectation', c => {c.assets[0].expectedOutcome = 'publish';}],
  ['duplicate negative originals', c => {c.assets.push(c.assets[0]);}],
]) test(`negative admission rejects ${name} before runtime work`, async t => {
  const c = await context(t); change(c); await assert.rejects(exerciseNegative(c), /negative-assets-invalid/); assert.equal(c.probe.calls.length,0);
});

test('unexpected acceptance is durably known, fails qualification, and cannot be published twice', async t => {
  const c = await context(t); let publishes = 0;
  c.probe.publish = async () => {publishes++; return {published:true,body:'PRIVATE_SENTINEL'};};
  await assert.rejects(exerciseNegative(c), /negative-publication-unexpected-success/);
  assert.deepEqual(c.journal.get('negative-publish:0').result,{outcome:'unexpected-published'});
  await c.journal.close();
  c.journal = await Journal.open(join(c.directory,'operations.jsonl'),{run:'negative-test'});
  await assert.rejects(exerciseNegative(c), /negative-publication-unexpected-success/);
  assert.equal(publishes,1);
  assert.equal(c.probe.calls.filter(call => call === 'create').length,1);
  assert.equal(c.probe.calls.includes('worker'),false);
  assert.doesNotMatch(await readFile(join(c.directory,'operations.jsonl'),'utf8'),/PRIVATE_SENTINEL/);
});

for (const [name, error] of [
  ['quality gate', rejection({...FAILURE,problemCode:'PUBLISH_QUALITY_REJECTED'})],
  ['another operation', rejection({...FAILURE,operation:'create'})],
  ['missing sanitized diagnostic', Object.assign(Error('PRIVATE_SENTINEL'),{status:422})],
  ['HTTP status mismatch', rejection(FAILURE,503)],
  ['unknown Problem code', rejection({...FAILURE,problemCode:'PRIVATE_SENTINEL'})],
]) test(`an unexplained 422 (${name}) cannot qualify`, async t => {
  const c = await context(t); c.probe.publish = async () => {throw error;};
  await assert.rejects(exerciseNegative(c), value => {assert.match(value.message,/negative-publication-rejection-mismatch/); assert.doesNotMatch(JSON.stringify(value)+value.stack,/PRIVATE_SENTINEL/); return true;});
  assert.equal(Object.hasOwn(c.journal.get('negative-publish:0'),'result'),false);
});

for (const [name, diagnostic] of [
  ['saved DSI evidence',{rowCount:1,pdf:true,rawHashMatches:true,sizeMatches:true,unresolvedTrackedChanges:0,embeddedComments:0,invalidSignatures:0,unverifiableSignatures:0,diagnosticCount:0}],
  ['unavailable DSI evidence',{}],
]) test(`${name} does not prove the required missing DSI`, async t => {
  const c = await context(t); c.diagnosePublication = async () => diagnostic;
  await assert.rejects(exerciseNegative(c), /negative-inspection-not-absent/);
  assert.deepEqual(c.journal.get('negative-publish:0').result.failureDiagnostic,FAILURE);
  assert.equal(c.probe.calls.includes('worker'),false);
});

for (const [name, diagnostic] of [
  ['wrong worker reason',{status:'worker-failure',failureCode:'requires_ocr',qualification:false,binding:binding()}],
  ['unavailable worker',{status:'unavailable',qualification:false,binding:binding()}],
  ['successful worker',{status:'inspected',qualification:false,binding:binding()}],
  ['worker qualification claim',{status:'worker-failure',failureCode:'unsupported_semantic_construct',qualification:true,binding:binding()}],
]) test(`${name} cannot establish unsupported semantics`, async t => {
  const c = await context(t); c.diagnoseWorker = async () => diagnostic;
  await assert.rejects(exerciseNegative(c), /negative-worker-rejection-mismatch/);
});

for (const field of ['fileCount','versionCount','authoritativeItemCount','isWorking','requiresContentClassification','mediaTypeMatches','rawHashMatches','sizeMatches']) {
  test(`publication binding mismatch (${field}) fails closed`, async t => {
    const c = await context(t), diagnose = c.diagnoseWorker;
    c.diagnoseWorker = async () => {const result = await diagnose(); result.binding[field] = typeof result.binding[field] === 'boolean' ? !result.binding[field] : 2; return result;};
    await assert.rejects(exerciseNegative(c), /negative-publication-binding-mismatch/);
  });
}

test('an explicitly unavailable binding cannot be promoted by otherwise matching scalar values', async t => {
  const c = await context(t), diagnose = c.diagnoseWorker;
  c.diagnoseWorker = async () => {const result = await diagnose(); result.binding.status = 'unavailable'; return result;};
  await assert.rejects(exerciseNegative(c), /negative-publication-binding-mismatch/);
});

test('known rejection remains journaled when extra diagnostics fail and is not sent again', async t => {
  const c = await context(t), diagnose = c.diagnosePublication;
  c.diagnosePublication = async () => {throw Error('PRIVATE_SENTINEL');};
  await assert.rejects(exerciseNegative(c), /negative-inspection-unavailable/);
  c.diagnosePublication = diagnose;
  const evidence = await exerciseNegative(c);
  assert.equal(evidence.counts.confirmedHttp422Responses,1);
  assert.equal(c.probe.calls.filter(call => call === 'create').length,1);
  assert.equal(c.probe.calls.filter(call => call === 'publish').length,1);
});

test('unknown initial create survives a real journal reopen and cannot be retried', async t => {
  const c = await context(t); let creates = 0;
  c.probe.create = async () => {creates++; throw Error('PRIVATE_SENTINEL');};
  await assert.rejects(exerciseNegative(c), /negative-prerequisite-failed/);
  await c.journal.close();
  c.journal = await Journal.open(join(c.directory,'operations.jsonl'),{run:'negative-test'});
  await assert.rejects(exerciseNegative(c), /negative-create-outcome-unknown/);
  assert.equal(creates,1);
});

for (const [name, mutate] of [
  ['document identity', snapshot => {snapshot.detail.documentId = 'wrong';}],
  ['version identity', snapshot => {snapshot.versions[0].detail.versionId = 'wrong';}],
  ['published pointer', snapshot => {snapshot.detail.currentVersionId = snapshot.detail.documentVersionId;}],
  ['version lifecycle', snapshot => {snapshot.versions[0].summary.lifecycleState = 'published';}],
  ['formal publication history', snapshot => {snapshot.revisions.push({revisionId:'unexpected'});}],
  ['original hash', snapshot => {snapshot.versions[0].files[0].sha256 = '0'.repeat(64);}],
  ['original length', snapshot => {snapshot.versions[0].files[0].downloadedBytes++;}],
  ['unexpected original', snapshot => {snapshot.versions[0].files.push(structuredClone(snapshot.versions[0].files[0]));}],
  ['agent grant', snapshot => {snapshot.policy.effectiveGrants.push({...HUMAN,subjectId:'poc-agents'});}],
]) test(`baseline ${name} mismatch prevents publication`, async t => {
  const c = await context(t), snapshot = c.probe.snapshot;
  c.probe.snapshot = async (...args) => {const result = await snapshot(...args); mutate(result); return result;};
  await assert.rejects(exerciseNegative(c), /negative-snapshot-invalid/);
  assert.equal(c.probe.calls.includes('publish'),false);
});

test('a rejected publication cannot change the authoring snapshot or revision', async t => {
  const c = await context(t), publish = c.probe.publish;
  c.probe.publish = async (...args) => {c.probe.docs.get(args[0]).detail.revision++; return publish(...args);};
  await assert.rejects(exerciseNegative(c), /negative-rejection-state-changed/);
});

for (const [name, mutate] of [
  ['revision', snapshot => {snapshot.detail.revision++;}],
  ['metadata', snapshot => {snapshot.detail.metadata.additional = 'PRIVATE_SENTINEL';}],
  ['original', snapshot => {snapshot.versions[0].files[0].sha256 = '0'.repeat(64);}],
  ['access policy', snapshot => {snapshot.policy.policyRevision++;}],
]) test(`restart detects retained ${name} tampering without disclosing values`, async t => {
  const c = await context(t), evidence = await exerciseNegative(c);
  mutate(c.probe.docs.get(evidence.documentIds[0]));
  await assert.rejects(verifyNegativeRetained({...c,evidence}), error => {assert.match(error.message,/negative-retained-state-changed/); assert.doesNotMatch(error.stack+JSON.stringify(error),/PRIVATE_SENTINEL/); return true;});
});

test('restart fails when a missing DSI can no longer be established', async t => {
  const c = await context(t), evidence = await exerciseNegative(c);
  c.diagnosePublication = async () => ({rowCount:2});
  await assert.rejects(verifyNegativeRetained({...c,evidence}), /negative-inspection-not-absent/);
});

test('diagnostic callbacks receive only owned file identity and expected original binding', async t => {
  const c = await context(t), seen = [], diagnose = c.diagnoseWorker;
  c.diagnoseWorker = async (...args) => {seen.push(args); return diagnose(...args);};
  const evidence = await exerciseNegative(c);
  assert.deepEqual(seen,[[evidence.documents[0].fileId,{assetId:c.assets[0].id,sha256:c.assets[0].sha256}]]);
  const persisted = await readFile(join(c.directory,'operations.jsonl'),'utf8');
  assert.doesNotMatch(JSON.stringify(evidence)+persisted,/PRIVATE_SENTINEL|%PDF|\"bytes\"|\"body\"|\"credentials\"/);
});

test('every external operation is checkpoint guarded and a stop prevents all creates', async t => {
  const c = await context(t); await exerciseNegative(c);
  for (const [index, call] of c.probe.calls.entries()) if (call !== 'checkpoint') assert.equal(c.probe.calls[index-1],'checkpoint',`unguarded ${call}`);
  const d = await context(t);
  d.checkpoint = async () => {throw Object.assign(Error('PRIVATE_SENTINEL'),{code:'resource-budget-exhausted'});};
  await assert.rejects(exerciseNegative(d), error => {assert.equal(error.code,'resource-budget-exhausted'); assert.doesNotMatch(error.stack,/PRIVATE_SENTINEL/); return true;});
  assert.equal(d.probe.calls.length,0);
});

test('negative failure category accepts only an internal fixed-code error and rejects raw or spoofed diagnostics', async t => {
  assert.equal(typeof sanitizeNegativeFailureCode,'function','negative failure sanitizer is not implemented');
  const c = await context(t); c.probe.publish = async () => ({body:'PRIVATE_SENTINEL'});
  let failure;
  try {await exerciseNegative(c);} catch (error) {failure = error;}
  assert.equal(sanitizeNegativeFailureCode(failure),'negative-publication-unexpected-success');
  for (const value of [undefined,null,'PRIVATE_SENTINEL','negative-publication-unexpected-success',{},
    {code:'negative-publication-unexpected-success',name:'NegativeQualificationError',message:'PRIVATE_SENTINEL'},
    Object.assign(Error('PRIVATE_SENTINEL'),{code:'negative-publication-unexpected-success',name:'NegativeQualificationError'}),
    {code:'negative-PRIVATE_SENTINEL'},new Error('negative-publication-unexpected-success')]) {
    assert.equal(sanitizeNegativeFailureCode(value),undefined);
  }
  failure.code = 'negative-PRIVATE_SENTINEL';
  assert.equal(sanitizeNegativeFailureCode(failure),undefined);
});

test('negative failure sanitizer preserves a safe prerequisite category without exposing a transport cause', async t => {
  assert.equal(typeof sanitizeNegativeFailureCode,'function','negative failure sanitizer is not implemented');
  const c = await context(t); c.probe.create = async () => {throw Object.assign(Error('PRIVATE_SENTINEL'),{body:'PRIVATE_SENTINEL',credentials:'PRIVATE_SENTINEL'});};
  await assert.rejects(exerciseNegative(c), error => {
    assert.equal(sanitizeNegativeFailureCode(error),'negative-prerequisite-failed');
    assert.doesNotMatch(JSON.stringify(error)+error.stack,/PRIVATE_SENTINEL/);
    return true;
  });
});

test('negative corpus folder names are distinct across co-located stages',async t=>{
 const names=[];
 for(const stageLabel of ['small',1000]){
  const c=await context(t);c.stageLabel=stageLabel;c.probe.createFolder=async request=>{names.push(request.name);return{resourceId:request.folderId};};await exerciseNegative(c);
 }
 assert.equal(new Set(names.map(name=>name.toLowerCase())).size,2);
});
