import test from 'node:test';
import assert from 'node:assert/strict';
import {access, mkdir, mkdtemp, rm, writeFile, readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {runQualification} from '../controller.mjs';
const chain = await import('../chain.mjs');
const MINUTE = 60_000, GiB = 1024 ** 3;
const fingerprint = {code:'a'.repeat(40),corpus:'b'.repeat(64),runtime:'c'.repeat(64)};
const runId = '0198eada-1234-7000-8000-000000000001';
const dataset = {runId,sourceHead:fingerprint.code,databaseIdentitySha256:'d'.repeat(64),storageIdentitySha256:'e'.repeat(64),runtimeSource:'built-in-this-run',containerId:'owned-test-container'};
const corpus = {hash:fingerprint.corpus,assets:[]};
// These orchestration fixtures are not real-process qualification evidence.
function report({plan,previousReport}, changes={}) {
  return {schemaVersion:1,runId,evidenceClass:'test-double',status:'SUCCEEDED',stage:plan.stage,documentCount:plan.documentCount,fingerprint:plan.fingerprint,plan,previousReport,
    restart:{identityRetained:true,processesReplaced:true,before:{...dataset},after:{...dataset},processes:{before:[100,200],after:[101,201]}},
    evidence:{documentIds:Array.from({length:plan.documentCount},(_,i)=>`test-document-${i}`)},metrics:{totalElapsedMs:1,diskGrowthBytes:0,peakRssBytes:1},...changes};
}
async function setup(t,changes={}) {
  assert.equal(typeof chain.runTenThousandChain,'function');
  const directory=await mkdtemp(join(tmpdir(),'document-ten-thousand-chain-'));
  t.after(()=>rm(directory,{recursive:true,force:true}));
  const calls=[],started=changes.now?.()??Date.now();
  return {directory,runId,fingerprint,corpus,probeFactory:()=>({}),calls,
    runtime:{evidenceClass:'test-double',identity:async()=>({...dataset,humanPid:101,agentPid:201})},
    workDeadlineAt:new Date(started+345*MINUTE).toISOString(),now:()=>started,
    qualify:async options=>{calls.push(options);return report(options);},...changes};
}
test('10k runs fresh small then 1000 then 10000 with complete reports and exclusive stage journals',async t=>{
  const options=await setup(t),result=await chain.runTenThousandChain(options);
  assert.equal(result.status,'SUCCEEDED');
  assert.deepEqual(options.calls.map(({plan})=>plan.stage),['small',1000,10000]);
  assert.deepEqual(options.calls.map(({plan})=>plan.documentCount),[2,1000,10000]);
  assert.equal(options.calls[0].previousReport,undefined);
  assert.equal(options.calls[1].previousReport,result.small);
  assert.equal(options.calls[2].previousReport,result.thousand);
  assert.equal(result.tenThousand.previousReport.previousReport,result.small);
  for(const call of options.calls){
    assert.equal(call.runId,runId);assert.equal(call.corpus,corpus);assert.equal(call.runtime,options.runtime);
    assert.equal(call.probeFactory,options.probeFactory);assert.equal(call.plan.fingerprint,fingerprint);
    assert.equal(call.plan.safetyFactor,2);assert.equal(call.plan.budgets.diskReserveBytes,GiB);
    assert.equal(call.plan.budgets.minAvailableMemoryBytes,512*1024**2);assert.equal(call.plan.budgets.maxRssBytes,2*GiB);
    assert.equal(call.directory,join(options.directory,String(call.plan.stage)));await access(call.directory);
    assert.ok(Date.parse(call.plan.deadlineAt)<=Date.parse(options.workDeadlineAt));
  }
  assert.deepEqual(options.calls.map(({plan})=>plan.budgets.maxWallTimeMs),[5,120,330].map(v=>v*MINUTE));
  assert.equal(result.hundredThousand,undefined);await assert.rejects(access(join(options.directory,'100000')));
});
test('build and preparation time already consumed stays deducted from the fixed work cutoff',async t=>{
  const started=Date.now();let current=started+30*MINUTE;
  const options=await setup(t,{now:()=>current,workDeadlineAt:new Date(started+345*MINUTE).toISOString()});
  options.qualify=async args=>{options.calls.push(args);if(args.plan.stage==='small')current+=MINUTE;if(args.plan.stage===1000)current+=16*MINUTE;return report(args);};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'SUCCEEDED');
  assert.equal(Date.parse(result.small.plan.deadlineAt),started+35*MINUTE);
  assert.equal(Date.parse(result.thousand.plan.deadlineAt),started+151*MINUTE);
  assert.equal(Date.parse(result.tenThousand.plan.deadlineAt),started+345*MINUTE);
  assert.equal(Date.parse(result.tenThousand.plan.deadlineAt)-current,298*MINUTE);
});
test('a short remaining work budget caps both deadline and monotonic stage timer',async t=>{
  const started=Date.now(),options=await setup(t,{now:()=>started,workDeadlineAt:new Date(started+MINUTE).toISOString()});
  options.qualify=async args=>report(args,{status:'ABORTED'});
  const result=await chain.runTenThousandChain(options);
  assert.equal(result.small.plan.deadlineAt,options.workDeadlineAt);assert.equal(result.small.plan.budgets.maxWallTimeMs,MINUTE);
});
for(const value of [undefined,null,'tomorrow','2026-02-30T00:00:00.000Z',0,Infinity])test(`missing or malformed work deadline fails before small: ${value}`,async t=>{
  const options=await setup(t,{workDeadlineAt:value}),result=await chain.runTenThousandChain(options);
  assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-deadline-invalid');assert.equal(options.calls.length,0);
  await assert.rejects(access(join(options.directory,'small')));
});
test('expired and implausibly long work cutoffs fail before any stage',async t=>{
  for(const delta of [0,-1,345*MINUTE+1]){
    const started=Date.now(),options=await setup(t,{now:()=>started,workDeadlineAt:new Date(started+delta).toISOString()});
    const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(options.calls.length,0);
    assert.equal(result.failureCode,delta>0?'chain-deadline-invalid':'chain-deadline-exhausted');
  }
});
for(const stage of ['small',1000])for(const status of ['FAILED','ABORTED','NOT_ADMITTED','NOT_RUN','AWAITING_RESTART'])test(`${status} ${stage} never starts the next stage`,async t=>{
  const options=await setup(t);options.qualify=async args=>{options.calls.push(args);return report(args,{status:args.plan.stage===stage?status:'SUCCEEDED'});};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,status);
  assert.equal(result.tenThousand,undefined);assert.equal(options.calls.length,stage==='small'?1:2);
  await assert.rejects(access(join(options.directory,'10000')));
});
for(const field of [...Object.keys(dataset),'humanPid','agentPid','unexpectedField'])test(`10k refuses changed interstage ${field}`,async t=>{
  const options=await setup(t);let reads=0;
  options.runtime.identity=async()=>({...dataset,humanPid:101,agentPid:201,...(++reads===2?{[field]:'changed'}:{})});
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');
  assert.equal(result.failureCode,'interstage-runtime-identity-mismatch');assert.equal(result.tenThousand,undefined);assert.equal(options.calls.length,2);
});
test('10k cannot accept a new dataset even when the 1000 report and current runtime agree on it',async t=>{
  const options=await setup(t);let reads=0;
  options.runtime.identity=async()=>({...dataset,...(++reads===2?{containerId:'replacement'}:{}),humanPid:101,agentPid:201});
  options.qualify=async args=>{options.calls.push(args);const value=report(args);if(args.plan.stage===1000)value.restart.before.containerId=value.restart.after.containerId='replacement';return value;};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-runtime-identity-mismatch');
  assert.equal(options.calls.length,2);
});
for(const change of [{runId:'different'},{fingerprint:{...fingerprint,corpus:'different'}},{stage:10000},{documentCount:999},{previousReport:null}])test(`changed 1000 report origin refuses 10k: ${Object.keys(change)[0]}`,async t=>{
  const options=await setup(t);options.qualify=async args=>{options.calls.push(args);return report(args,args.plan.stage===1000?change:{});};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-report-identity-mismatch');assert.equal(options.calls.length,2);
});
test('clock rollback between stages fails even when the clock remains later than chain start',async t=>{
  const started=Date.now();let current=started;const options=await setup(t,{now:()=>current});
  options.qualify=async args=>{options.calls.push(args);current=started+(args.plan.stage==='small'?2:1)*MINUTE;return report(args);};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-clock-invalid');assert.equal(options.calls.length,2);
});
test('slow identity lookup exhausting the cutoff cannot start 10k',async t=>{
  const started=Date.now();let current=started,reads=0;const options=await setup(t,{now:()=>current});
  options.runtime.identity=async()=>{if(++reads===2)current=Date.parse(options.workDeadlineAt);return {...dataset,humanPid:101,agentPid:201};};
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-deadline-exhausted');assert.equal(options.calls.length,2);
});
test('existing 10k evidence is not imported, resumed or overwritten',async t=>{
  const options=await setup(t);await mkdir(join(options.directory,'10000'));await writeFile(join(options.directory,'10000','report.json'),'PRIVATE_SENTINEL');
  const result=await chain.runTenThousandChain(options);assert.equal(result.status,'FAILED');assert.equal(result.failureCode,'chain-prerequisite-failed');
  assert.equal(options.calls.length,2);assert.equal(await readFile(join(options.directory,'10000','report.json'),'utf8'),'PRIVATE_SENTINEL');assert.ok(!JSON.stringify(result).includes('PRIVATE_SENTINEL'));
});
for(const remainingMinutes of [313,314])test(`unchanged real admission uses factor2 with ${remainingMinutes} minutes left`,async t=>{
  const options=await setup(t,{now:Date.now,workDeadlineAt:new Date(Date.now()+remainingMinutes*MINUTE).toISOString()});
  let restarted=false;const mutations=[];
  options.runtime.evidenceClass='owned-real-process';
  options.runtime.observe=async()=>({observedAt:new Date().toISOString(),diskFreeBytes:10*GiB,storageDiskFreeBytes:10*GiB,databaseDiskFreeBytes:10*GiB,availableMemoryBytes:10*GiB,rssBytes:1,storageBytes:0,databaseBytes:0,rssCoverage:'process-tree',databaseFilesystemVerified:true});
  options.runtime.identity=async()=>({...dataset,humanPid:restarted?102:101,agentPid:restarted?202:201});
  options.runtime.restart=async()=>{restarted=true;};
  options.qualify=async args=>{
    if(args.plan.stage!==10000)return report(args,{evidenceClass:'owned-real-process',metrics:{totalElapsedMs:args.plan.stage===1000?940943.441764:1,diskGrowthBytes:0,peakRssBytes:1}});
    return runQualification({...args,execute:async({count})=>{mutations.push(count);return {documentIds:Array.from({length:count},(_,i)=>String(i))};},verify:async()=>{}});
  };
  const result=await chain.runTenThousandChain(options);
  assert.equal(result.tenThousand.admission.projection.multiplier,20);
  assert.equal(result.tenThousand.admission.projection.totalElapsedMs,18818869);
  assert.equal(result.status,remainingMinutes===313?'NOT_ADMITTED':'SUCCEEDED');
  assert.deepEqual(mutations,remainingMinutes===313?[]:[10000]);
  if(remainingMinutes===313)await assert.rejects(access(join(options.directory,'10000','operations.jsonl')));
});
