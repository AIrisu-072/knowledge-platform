import test from 'node:test';
import assert from 'node:assert/strict';
import * as safety from '../safety.mjs';
const roots=[10,20,30,40];
const roles={harness:10,human:20,agent:30,database:40};
const snapshot='10 0 100\n20 10 200\n30 10 300\n40 0 400\n21 20 50\n31 30 60\n11 10 70\n99 0 900\n';
test('role RSS partitions roots and all other descendants once, preserving aggregate RSS',()=>{
 const result=safety.processTreeRssSnapshot(roots,snapshot,roles);
 assert.equal(result.rssBytes,1180*1024);
 assert.deepEqual(result.rssByRoleBytes,{harness:100*1024,human:200*1024,agent:300*1024,database:400*1024,otherDescendants:180*1024});
 assert.equal(Object.values(result.rssByRoleBytes).reduce((a,b)=>a+b,0),result.rssBytes);
 assert.deepEqual(result.measuredPids,[10,11,20,21,30,31,40]);
 assert.deepEqual(safety.processTreeRssSnapshot([10,10,20,30,40],snapshot,roles),result);
 assert.equal(safety.processTreeRssSnapshot(roots,snapshot).rssByRoleBytes,undefined);
});
test('incomplete parent/PID observations and ambiguous role ownership fail closed',()=>{
 for(const broken of ['10 0\n',snapshot.replace('20 10 200','20 777 200'),snapshot+'10 0 100\n',snapshot.replace('10 0 100','10 21 100'),snapshot.replace('40 0 400\n','')])assert.throws(()=>safety.processTreeRssSnapshot(roots,broken,roles));
 for(const invalid of [{...roles,human:10},{...roles,agent:777},{harness:10,human:20,agent:30},{...roles,database:'40'},{...roles,secret:'PRIVATE_SENTINEL'}])assert.throws(()=>safety.processTreeRssSnapshot(roots,snapshot,invalid));
 assert.throws(()=>safety.processTreeRssSnapshot(roots,snapshot.replace('10 0 100','10 0 9007199254740991'),roles));
});
test('harness heap projection keeps only fixed finite integer byte counters',()=>{
 const raw={rss:500,heapTotal:400,heapUsed:200,external:100,arrayBuffers:50,secret:'PRIVATE_SENTINEL'};
 assert.deepEqual(safety.harnessHeapSnapshot(raw),{rss:500,heapTotal:400,heapUsed:200,external:100,arrayBuffers:50});
 assert.ok(!JSON.stringify(safety.harnessHeapSnapshot(raw)).includes('PRIVATE_SENTINEL'));
 for(const invalid of [undefined,null,{}, {...raw,heapUsed:NaN},{...raw,heapUsed:'1'},{...raw,external:-1},{...raw,arrayBuffers:Infinity},{...raw,heapTotal:Number.MAX_SAFE_INTEGER+1}])assert.throws(()=>safety.harnessHeapSnapshot(invalid));
});
test('unavailable role ownership keeps both RSS and heap null rather than reporting zeros',async()=>{
 const result=await safety.observeResources({storageRoot:'/nonexistent',databaseBytes:0,pids:[process.pid],rolePids:{harness:process.pid,human:process.pid,agent:process.pid,database:process.pid},maxStorageEntries:1});
 assert.equal(result.rssBytes,null);
 assert.equal(result.rssCoverage,'unavailable');
 assert.equal(result.rssByRoleBytes,null);
 assert.equal(result.harnessHeapBytes,null);
 const wrongHarness=await safety.observeResources({storageRoot:'/nonexistent',databaseBytes:0,pids:[process.pid],rolePids:{harness:2147483647,human:process.pid,agent:process.pid,database:process.pid},maxStorageEntries:1});
 assert.equal(wrongHarness.harnessHeapBytes,null);
 assert.equal(wrongHarness.rssBytes,null);
});
