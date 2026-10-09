import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,rm,stat,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {summarizeResources,runQualification} from '../controller.mjs';
import {summarizeTimings} from '../safety.mjs';
import {summarizeScans} from '../scan-diagnostics.mjs';
const {BoundedTelemetry}=await import('../telemetry.mjs').catch(()=>({}));
const observation=(i=0)=>({observedAt:new Date().toISOString(),rssBytes:500+i,diskFreeBytes:100000-i,storageDiskFreeBytes:200000-i,databaseDiskFreeBytes:100000-i,databaseBytes:100+i,storageBytes:i,availableMemoryBytes:100000,rssCoverage:'process-tree',databaseFilesystemVerified:true,resourceObservationMs:i%7,storageScan:{complete:true,elapsedMs:i%5,entries:i+1},limitations:[]});
async function directory(t){const path=await mkdtemp(join(tmpdir(),'load-telemetry-'));t.after(()=>rm(path,{recursive:true,force:true}));return path;}
async function telemetry(t){assert.equal(typeof BoundedTelemetry?.open,'function');const path=join(await directory(t),'observations.jsonl');const value=await BoundedTelemetry.open(path);t.after(()=>value.close().catch(()=>{}));return value;}
const timingSummary=values=>Object.fromEntries([...new Set(values.map(t=>t.operation))].map(operation=>{const samples=values.filter(t=>t.operation===operation);return[operation,{...summarizeTimings(samples.map(t=>t.elapsedMs)),statuses:Object.fromEntries([...new Set(samples.map(t=>String(t.status)))].map(status=>[status,samples.filter(t=>String(t.status)===status).length]))}];}));

test('private append-only observations preserve every sample and verified close digest with a bounded recent window',async t=>{
 const value=await telemetry(t),samples=Array.from({length:513},(_,i)=>({...observation(i),privateDiagnostic:`sample-${i}`}));
 for(const sample of samples)await value.recordObservation(sample);
 assert.equal(value.recentObservations.length,256);assert.deepEqual(value.recentObservations,samples.slice(-256));
 const proof=await value.close(),bytes=await readFile(proof.path);
 assert.deepEqual(bytes.toString().trim().split('\n').map(line=>JSON.parse(line)),samples);
 assert.equal(proof.count,samples.length);assert.equal(proof.bytes,bytes.length);assert.equal(proof.sha256,createHash('sha256').update(bytes).digest('hex'));
 assert.equal((await stat(proof.path)).mode&0o777,0o600);assert.deepEqual(await value.close(),proof);
 await assert.rejects(value.recordObservation(observation()),/closed/);
 await assert.rejects(BoundedTelemetry.open(proof.path));assert.deepEqual(await readFile(proof.path),bytes);
});

test('online resource and scan reducers match the existing whole-array reducers exactly at every prefix',async t=>{
 const value=await telemetry(t),samples=[];assert.equal(value.summary(0),null);assert.deepEqual(value.scanSummary(),summarizeScans([]));
 for(let i=0;i<300;i++){
  const sample={...observation(i),rssBytes:(i*173)%901,storageBytes:(i*37)%1701,databaseBytes:(i*23)%601,storageDiskFreeBytes:20000-(i*53)%1101,databaseDiskFreeBytes:10000-(i*83)%701};
  samples.push(sample);await value.recordObservation(sample);
  assert.deepEqual(value.summary(i+0.25),summarizeResources(samples,i+0.25));assert.deepEqual(value.scanSummary(),summarizeScans(samples));
 }
});

test('close waits for in-flight appends before returning the durable count and digest',async t=>{
 const value=await telemetry(t),samples=Array.from({length:32},(_,i)=>observation(i));
 const appends=samples.map(sample=>value.recordObservation(sample)),proof=await value.close();await Promise.all(appends);
 const bytes=await readFile(proof.path);assert.equal(proof.count,32);assert.deepEqual(bytes.toString().trim().split('\n').map(JSON.parse),samples);assert.equal(proof.sha256,createHash('sha256').update(bytes).digest('hex'));
});

test('invalid resource samples remain disqualifying after leaving the recent window',async t=>{
 for(const [key,invalid] of [['rssBytes',null],['diskFreeBytes',-1],['storageDiskFreeBytes',NaN],['databaseDiskFreeBytes',Infinity],['databaseBytes',Number.MAX_SAFE_INTEGER+1],['storageBytes','12']]){
  const value=await telemetry(t),samples=[observation(),{...observation(),[key]:invalid},...Array.from({length:260},(_,i)=>observation(i))];
  for(const sample of samples)await value.recordObservation(sample);
  assert.equal(value.summary(300),summarizeResources(samples,300));assert.equal(value.summary(300),null);
 }
});

test('invalid scan data remains unavailable after leaving the recent window',async t=>{
 const value=await telemetry(t),samples=[observation(),{...observation(),storageScan:{complete:false,elapsedMs:null,entries:null}},...Array.from({length:260},(_,i)=>observation(i))];
 for(const sample of samples)await value.recordObservation(sample);
 assert.deepEqual(value.scanSummary(),summarizeScans(samples));assert.deepEqual(value.scanSummary(),{status:'unavailable'});
});

test('numeric-only timing retention preserves exact nearest-rank means, percentiles and status counts',async t=>{
 const value=await telemetry(t),samples=Array.from({length:50003},(_,i)=>({operation:['create','detail','publish','__proto__'][i%4],elapsedMs:i%17===0?0:(i*131)%10007/13,status:[200,422,'network-error',undefined][i%4],discardedPayload:'private'.repeat(20)}));
 assert.deepEqual(value.timingSummary(),{});
 for(const sample of samples)value.recordTiming(sample);
 assert.deepEqual(value.timingSummary(),timingSummary(samples));
 value.recordTiming({operation:'create',elapsedMs:1/3,status:201});samples.push({operation:'create',elapsedMs:1/3,status:201});
 assert.deepEqual(value.timingSummary(),timingSummary(samples));
});

test('invalid timing remains a sticky failure even when an API callback swallows the error',async t=>{
 const value=await telemetry(t);value.recordTiming({operation:'create',elapsedMs:1,status:200});
 assert.throws(()=>value.recordTiming({operation:'create',elapsedMs:NaN,status:200}));
 assert.throws(()=>value.timingSummary());await assert.rejects(value.close());
});

test('timing percentiles retain the existing stable ordering of positive and negative zero',async t=>{
 const value=await telemetry(t),samples=[0,-0,0,-0].map(elapsedMs=>({operation:'read',elapsedMs,status:200}));
 for(const sample of samples)value.recordTiming(sample);
 assert.deepEqual(value.timingSummary(),timingSummary(samples));
});

test('real filesystem failures cannot be turned into a successful close proof',async t=>{
 assert.equal(typeof BoundedTelemetry?.open,'function');
 const root=await directory(t);const occupied=join(root,'occupied');await writeFile(occupied,'keep');
 await assert.rejects(BoundedTelemetry.open(occupied));assert.equal(await readFile(occupied,'utf8'),'keep');
 const cyclic={};cyclic.self=cyclic;const value=await telemetry(t);
 await assert.rejects(value.recordObservation(cyclic));await assert.rejects(value.close());
});

function plan(){return{stage:'small',documentCount:2,fingerprint:{code:'code',corpus:'corpus',runtime:'runtime'},safetyFactor:2,budgets:{diskReserveBytes:100,minAvailableMemoryBytes:100,maxRssBytes:10000,maxWallTimeMs:30000},deadlineAt:new Date(Date.now()+30000).toISOString()};}
async function controllerOptions(t,mode='bounded-disk'){
 const root=await directory(t);let restarted=false;let calls=0;
 return {directory:root,runId:'telemetry-test',plan:plan(),corpus:{hash:'corpus',assets:[]},runtime:{telemetryMode:mode,observe:async()=>observation(calls++),identity:async()=>({dataset:'same',humanPid:restarted?101:100,agentPid:restarted?201:200}),restart:async()=>{restarted=true;}},probeFactory:({onTiming})=>{onTiming({operation:'create',elapsedMs:12,status:200});return{};},execute:async()=>({status:'AWAITING_RESTART'}),verify:async()=>({status:'SUCCEEDED'})};
}
test('controller opts in explicitly, closes private observation proof before final success and preserves default report shape',async t=>{
 for(const mode of ['bounded-disk',undefined,'other']){
  const options=await controllerOptions(t,mode);if(mode===undefined)delete options.runtime.telemetryMode;
  const report=await runQualification(options);assert.equal(report.status,'SUCCEEDED');assert.deepEqual(JSON.parse(await readFile(join(options.directory,'report.json'),'utf8')),JSON.parse(JSON.stringify(report)));
  if(mode==='bounded-disk'){
   assert.equal(report.telemetry.count,2);const raw=await readFile(report.telemetry.path);assert.equal(report.telemetry.sha256,createHash('sha256').update(raw).digest('hex'));
   assert.deepEqual(report.metrics,summarizeResources(raw.toString().trim().split('\n').map(JSON.parse),report.metrics.totalElapsedMs));assert.deepEqual(report.scanDiagnostics,summarizeScans(report.observations));
  }else{assert.equal(Object.hasOwn(report,'telemetry'),false);assert.equal(Object.hasOwn(report,'scanDiagnostics'),false);}
 }
});

test('controller cannot pass when private observation storage cannot open or timing data is invalid',async t=>{
 for(const failure of ['occupied','timing']){
  const options=await controllerOptions(t);
  if(failure==='occupied')await writeFile(join(options.directory,'observations.jsonl'),'preserve-existing');
  else options.probeFactory=({onTiming})=>{try{onTiming({operation:'create',elapsedMs:NaN,status:200});}catch{}return{};};
  const report=await runQualification(options);assert.equal(report.status,'FAILED');assert.equal(report.metricQualification,'measurement-unavailable');assert.equal(report.throughputDocumentsPerSecond,null);assert.equal(JSON.parse(await readFile(join(options.directory,'report.json'),'utf8')).status,'FAILED');
 }
});

test('a swallowed timing callback failure aborts the controller before further workload',async t=>{
 const options=await controllerOptions(t);let aborted=false,executed=false;
 options.probeFactory=({signal,onTiming})=>{try{onTiming({operation:'create',elapsedMs:NaN,status:200});}catch{}aborted=signal.aborted;return{};};
 options.execute=async()=>{executed=true;return{};};
 const report=await runQualification(options);assert.equal(report.status,'FAILED');assert.equal(aborted,true);assert.equal(executed,false);
});

test('controller keeps all scan/resource maxima after observations leave the bounded debug window',async t=>{
 const options=await controllerOptions(t),samples=[];let calls=0;
 options.runtime.observe=async()=>{const sample={...observation(),rssBytes:calls===0?999:500,resourceObservationMs:calls===0?999:1};calls++;samples.push(sample);return sample;};
 options.execute=async({checkpoint})=>{for(let i=0;i<300;i++)await checkpoint(true);return{documentIds:['a','b']};};
 const report=await runQualification(options);assert.equal(report.status,'SUCCEEDED');assert.equal(report.observations.length,256);assert.equal(report.telemetry.count,302);assert.equal(report.sampleIntervalMs,1000);
 assert.deepEqual(report.metrics,summarizeResources(samples,report.metrics.totalElapsedMs));assert.deepEqual(report.scanDiagnostics,summarizeScans(samples));assert.deepEqual(report.evidence.documentIds,['a','b']);
});

test('real mid-stream filesystem write failure produces no close proof and cannot publish controller success',{skip:process.platform!=='linux'},async t=>{
 assert.equal(typeof BoundedTelemetry?.open,'function');const root=await directory(t);
 const result=await promisify(execFile)('bash',['-c','ulimit -f 16; exec "$@"','telemetry-limit',process.execPath,new URL('./fixtures/telemetry-write-failure.mjs',import.meta.url).pathname,root],{maxBuffer:1024*1024});
 assert.deepEqual(JSON.parse(result.stdout),{appendFailed:true,closeFailed:true,controllerStatus:'FAILED',metricQualification:'measurement-unavailable',durableStatus:'FAILED'});
});

test('80-hour observation and large API volume run has bounded child RSS and writes all 288,000 actual observations', {timeout:120000},async t=>{
 assert.equal(typeof BoundedTelemetry?.open,'function');const root=await directory(t);
 const result=await promisify(execFile)(process.execPath,['--max-old-space-size=96',new URL('./fixtures/telemetry-memory.mjs',import.meta.url).pathname,root],{maxBuffer:1024*1024,timeout:110000});
 const report=JSON.parse(result.stdout);assert.equal(report.count,288000);assert.equal(report.lines,288000);assert.equal(report.recentCount,256);assert.equal(report.timingCount,1200000);assert.ok(report.maxRssBytes<200*1024*1024,`child RSS ${report.maxRssBytes} exceeded 200 MiB`);assert.equal(report.digestMatches,true);
 t.diagnostic(`child maximum RSS ${report.maxRssBytes} bytes; 288000 durable samples and 1200000 exact timings`);
});
