import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,rm,symlink,stat} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
const {writeSmallReceipt,writeThousandReceipt,writeTenThousandReceipt}=await import('../receipt-export.mjs').catch(()=>({}));
function report(){
 const code='a'.repeat(40),identity={runId:'12345678-1234-4abc-8abc-123456789abc',sourceHead:code,databaseIdentitySha256:'d'.repeat(64),storageIdentitySha256:'e'.repeat(64)};
 const ids=['0198eada-1234-7000-8000-000000000001','0198eada-1234-7000-8000-000000000002'],negativeId='0198eada-1234-7000-8000-000000000003';
 return {schemaVersion:1,evidenceClass:'owned-real-process',status:'SUCCEEDED',runId:'12345678-1234-4abc-8abc-123456789abc',stage:'small',documentCount:2,fingerprint:{code,corpus:'b'.repeat(64),runtime:'c'.repeat(64)},startedAt:'2026-10-08T16:00:00.000Z',finishedAt:'2026-10-08T16:00:01.000Z',metricQualification:'complete-stage',productionSloClaim:false,qualityClaim:false,metrics:{totalElapsedMs:1000,peakRssBytes:4096,diskGrowthBytes:30,storageAllocatedGrowthBytes:10,databaseAllocatedGrowthBytes:20,databaseGrowthBytes:10,storageGrowthBytes:5},counts:{targetDocuments:2,confirmedCreatedDocuments:2,confirmedPublishedDocuments:2},evidence:{documentIds:ids,snapshots:{private:'PRIVATE_SENTINEL'}},restart:{identityRetained:true,processesReplaced:true,before:identity,after:{...identity},processes:{before:[100,200],after:[101,201]}},negativeCorpus:{status:'SUCCEEDED',contentQualityClaim:false,documentIds:[negativeId],counts:{targetDocuments:1,confirmedCreatedDocuments:1,confirmedHttp422Responses:1,confirmedRejectedDocuments:1,confirmedPublishedDocuments:0},documents:[{documentId:negativeId,failureDiagnostic:{operation:'publish',httpStatus:422,problemCode:'BUSINESS_RULE_REJECTED'},workerDiagnostic:{status:'worker-failure',failureCode:'unsupported_semantic_construct',qualification:false}}]}};
}
async function directory(t){const root=await mkdtemp(join(tmpdir(),'load-export-'));t.after(()=>rm(root,{recursive:true,force:true}));return root;}
test('writes only one private-mode fixed receipt file tied to the CI source code',async t=>{const root=await directory(t),source=report();assert.equal(typeof writeSmallReceipt,'function');const result=await writeSmallReceipt(root,source,source.fingerprint.code);const path=join(root,'tools/document-poc-runtime/.state/document-load-export/qualification.json'),bytes=await readFile(path,'utf8');assert.equal(result.sha256,JSON.parse(bytes).sha256);assert.equal(result.byteLength,Buffer.byteLength(bytes));assert.ok(!bytes.includes('PRIVATE_SENTINEL'));assert.equal((await stat(path)).mode&0o777,0o600);await assert.rejects(writeSmallReceipt(root,source,source.fingerprint.code));});
test('different source code or unsuccessful report creates no export',async t=>{const root=await directory(t),source=report();await assert.rejects(writeSmallReceipt(root,source,'f'.repeat(40)));await assert.rejects(writeSmallReceipt(root,{...source,status:'FAILED'},source.fingerprint.code));await assert.rejects(stat(join(root,'tools/document-poc-runtime/.state/document-load-export')));});
test('a symlinked evidence ancestor cannot redirect the fixed export',async t=>{const root=await directory(t),other=await directory(t),source=report();await mkdir(join(root,'tools/document-poc-runtime'),{recursive:true});await symlink(other,join(root,'tools/document-poc-runtime/.state'));await assert.rejects(writeSmallReceipt(root,source,source.fingerprint.code));await assert.rejects(stat(join(other,'document-load-export')));});

test('exports the complete successful two-stage receipt as one bounded fixed file',async t=>{
 const root=await directory(t),small=report(),thousand=structuredClone(small);
 const uuid=n=>`0198eada-1234-7000-8000-${String(n).padStart(12,'0')}`;
 thousand.stage=1000;thousand.documentCount=1000;thousand.counts={targetDocuments:1000,confirmedCreatedDocuments:1000,confirmedPublishedDocuments:1000};thousand.evidence.documentIds=Array.from({length:1000},(_,i)=>uuid(i+10));
 thousand.startedAt='2026-10-08T16:00:02.000Z';thousand.finishedAt='2026-10-08T16:00:03.000Z';thousand.restart.processes={before:[101,201],after:[102,202]};thousand.previousReport=small;
 thousand.negativeCorpus.documentIds=[uuid(2000)];thousand.negativeCorpus.documents[0].documentId=uuid(2000);
 const result=await writeThousandReceipt(root,{status:'SUCCEEDED',small,thousand},small.fingerprint.code);
 const text=await readFile(join(root,'tools/document-poc-runtime/.state/document-load-export/qualification.json'),'utf8');const envelope=JSON.parse(text);assert.equal(envelope.sha256,result.sha256);assert.equal(envelope.receipt.stages[1].evidence.documentIds.length,1000);assert.equal(envelope.receipt.stages[0].documentCount,2);assert.ok(!text.includes('PRIVATE_SENTINEL'));assert.ok(Buffer.byteLength(text)<1024*1024);
});

function tenThousandChain(){
 const small=report(),thousand=structuredClone(small),tenThousand=structuredClone(small);
 const uuid=n=>`0198eada-1234-7000-8000-${String(n).padStart(12,'0')}`;
 for(const [stage,count,offset,start,finish,before,after] of [[thousand,1000,4,'2026-10-08T16:00:02.000Z','2026-10-08T16:09:02.000Z',[101,201],[102,202]],[tenThousand,10000,1005,'2026-10-08T16:09:03.000Z','2026-10-08T17:39:03.000Z',[102,202],[103,203]]]){
  stage.stage=count;stage.documentCount=count;stage.counts={targetDocuments:count,confirmedCreatedDocuments:count,confirmedPublishedDocuments:count};stage.evidence.documentIds=Array.from({length:count},(_,i)=>uuid(i+offset));
  stage.startedAt=start;stage.finishedAt=finish;stage.restart.processes={before,after};stage.negativeCorpus.documentIds=[uuid(offset+count)];stage.negativeCorpus.documents[0].documentId=uuid(offset+count);
 }
 thousand.previousReport=small;tenThousand.previousReport=thousand;
 return {status:'SUCCEEDED',small,thousand,tenThousand};
}

test('exports all three successful stages through the exact projector argument as one private bounded file',async t=>{
 assert.equal(typeof writeTenThousandReceipt,'function');
 const root=await directory(t),source=tenThousandChain();source.privatePath='PRIVATE_SENTINEL';
 const result=await writeTenThousandReceipt(root,source,source.small.fingerprint.code);
 const path=join(root,'tools/document-poc-runtime/.state/document-load-export/qualification.json'),text=await readFile(path,'utf8'),envelope=JSON.parse(text);
 assert.equal(result.sha256,envelope.sha256);assert.equal(result.byteLength,Buffer.byteLength(text));assert.deepEqual(envelope.receipt.stages.map(stage=>stage.documentCount),[2,1000,10000]);assert.equal(envelope.receipt.stages[2].evidence.documentIds.length,10000);assert.ok(!text.includes('PRIVATE_SENTINEL'));assert.ok(result.byteLength<=1024*1024);assert.equal((await stat(path)).mode&0o777,0o600);
 await assert.rejects(writeTenThousandReceipt(root,source,source.small.fingerprint.code));
});

test('a missing, unsuccessful or incorrectly bound third stage creates no receipt directory',async t=>{
 assert.equal(typeof writeTenThousandReceipt,'function');
 for(const mutate of [c=>{c.status='NOT_ADMITTED';},c=>{delete c.tenThousand;},c=>{c.tenThousand.status='ABORTED';},c=>{c.tenThousand.counts.confirmedPublishedDocuments=9999;},c=>{delete c.tenThousand.metrics.peakRssBytes;},c=>{c.tenThousand.previousReport=structuredClone(c.thousand);}]){
  const root=await directory(t),source=tenThousandChain();mutate(source);await assert.rejects(writeTenThousandReceipt(root,source,source.small.fingerprint.code));await assert.rejects(stat(join(root,'tools/document-poc-runtime/.state/document-load-export')));
 }
 const root=await directory(t),source=tenThousandChain();await assert.rejects(writeTenThousandReceipt(root,source,'f'.repeat(40)));await assert.rejects(stat(join(root,'tools/document-poc-runtime/.state/document-load-export')));
});

test('three-stage export cannot follow a symlinked state directory',async t=>{
 assert.equal(typeof writeTenThousandReceipt,'function');
 const root=await directory(t),other=await directory(t),source=tenThousandChain();await mkdir(join(root,'tools/document-poc-runtime'),{recursive:true});await symlink(other,join(root,'tools/document-poc-runtime/.state'));await assert.rejects(writeTenThousandReceipt(root,source,source.small.fingerprint.code));await assert.rejects(stat(join(other,'document-load-export')));
});
