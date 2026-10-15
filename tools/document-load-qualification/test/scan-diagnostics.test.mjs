import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {observeResources} from '../safety.mjs';
const diagnostics=await import('../scan-diagnostics.mjs').catch(()=>({}));

test('bounded scan summary retains real maxima and rejects missing measurements without leaking raw fields',()=>{
 assert.equal(typeof diagnostics.summarizeScans,'function');
 const observations=[{resourceObservationMs:12,storageScan:{complete:true,entries:4,elapsedMs:2},secret:'PRIVATE'},
 {resourceObservationMs:20,storageScan:{complete:true,entries:7,elapsedMs:3}}];
 assert.deepEqual(diagnostics.summarizeScans(observations),{status:'measured',observationCount:2,maxResourceObservationMs:20,maxStorageScanMs:3,maxStorageEntries:7});
 for(const value of [[],[{storageScan:{complete:false,entries:null,elapsedMs:null}}],[...observations,{resourceObservationMs:NaN,storageScan:{complete:true,entries:1,elapsedMs:2}}]])assert.deepEqual(diagnostics.summarizeScans(value),{status:'unavailable'});
});

test('opt-in scan diagnostics measure traversed entries without changing logical byte measurement',async t=>{
 const root=await mkdtemp(join(tmpdir(),'load-scan-'));t.after(()=>rm(root,{recursive:true,force:true}));
 await mkdir(join(root,'objects'));await writeFile(join(root,'objects','a'),'abcd');await writeFile(join(root,'objects','b'),'123456');
 const value=await observeResources({storageRoot:root,databaseRoot:root,pids:[process.pid],databaseBytes:0,includeScanDiagnostics:true});
 assert.equal(value.storageBytes,10);assert.equal(value.storageScan?.entries,4);assert.ok(Number.isFinite(value.storageScan?.elapsedMs));assert.ok(value.storageScan.elapsedMs>=0);assert.equal(value.storageScan.complete,true);
 const previous=await observeResources({storageRoot:root,databaseRoot:root,pids:[process.pid],databaseBytes:0});assert.equal(Object.hasOwn(previous,'storageScan'),false);
});
test('failed bounded scan has no fabricated complete timing/count evidence',async t=>{
 const root=await mkdtemp(join(tmpdir(),'load-scan-'));t.after(()=>rm(root,{recursive:true,force:true}));await writeFile(join(root,'a'),'a');
 const value=await observeResources({storageRoot:root,databaseRoot:root,pids:[process.pid],databaseBytes:0,maxStorageEntries:1,includeScanDiagnostics:true});
 assert.equal(value.storageBytes,null);assert.deepEqual(value.storageScan,{complete:false,elapsedMs:null,entries:null});
});
