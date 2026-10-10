import test from 'node:test';
import assert from 'node:assert/strict';
import {access, mkdir, mkdtemp, rm, writeFile, readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {runQualification} from '../controller.mjs';
const chain = await import('../local-chain.mjs').catch(() => ({}));
const MINUTE = 60_000, HOUR = 60*MINUTE, GiB = 1024**3;
const fingerprint = {code:'a'.repeat(40),corpus:'b'.repeat(64),runtime:'c'.repeat(64)};
const runId = '0198eada-1234-7000-8000-000000000001';
const dataset = {runId,sourceHead:fingerprint.code,databaseIdentitySha256:'d'.repeat(64),storageIdentitySha256:'e'.repeat(64),runtimeSource:'built-in-this-run',containerId:'owned-test-container'};
const corpus = {hash:fingerprint.corpus,assets:[]};
// Synthetic orchestration fixtures; these are never exported as real qualification evidence.
function report({plan,previousReport}, changes={}) {
  return {schemaVersion:1,runId,evidenceClass:'test-double',status:'SUCCEEDED',stage:plan.stage,documentCount:plan.documentCount,fingerprint:plan.fingerprint,plan,previousReport,
    restart:{identityRetained:true,processesReplaced:true,before:{...dataset},after:{...dataset},processes:{before:[100,200],after:[101,201]}},
    evidence:{documentIds:Array.from({length:plan.documentCount},(_,i)=>`test-document-${plan.stage}-${i}`)},
    metrics:{totalElapsedMs:1,diskGrowthBytes:0,peakRssBytes:1},...changes};
}
async function setup(t,changes={}) {
  assert.equal(typeof chain.runLocalScaleChain,'function');
  const directory=await mkdtemp(join(tmpdir(),'document-local-chain-'));
  t.after(()=>rm(directory,{recursive:true,force:true}));
  const calls=[],started=changes.now?.()??Date.now();
  return {directory,runId,fingerprint,corpus,probeFactory:()=>({}),calls,
    runtime:{evidenceClass:'test-double',identity:async()=>({...dataset,humanPid:101,agentPid:201})},
    workDeadlineAt:changes.workDeadlineAt??new Date(started+80*HOUR-15*MINUTE).toISOString(),now:()=>started,
    qualify:async options=>{calls.push(options);return report(options);},...changes};
}

test('local chain runs four fresh serial stages with complete reports and fixed reserves',async t=>{
  const options=await setup(t),result=await chain.runLocalScaleChain(options);
  assert.equal(result.status,'SUCCEEDED');
  assert.deepEqual(options.calls.map(({plan})=>plan.stage),['small',1000,10000,100000]);
  assert.deepEqual(options.calls.map(({plan})=>plan.documentCount),[2,1000,10000,100000]);
  assert.deepEqual(options.calls.map(({plan})=>plan.budgets.maxWallTimeMs),[5*MINUTE,120*MINUTE,330*MINUTE,72*HOUR]);
  assert.equal(options.calls[0].previousReport,undefined);
  assert.equal(options.calls[1].previousReport,result.small);
  assert.equal(options.calls[2].previousReport,result.thousand);
  assert.equal(options.calls[3].previousReport,result.tenThousand);
  assert.equal(result.hundredThousand.previousReport.previousReport.previousReport,result.small);
  for(const call of options.calls){
    assert.equal(call.runId,runId);assert.equal(call.corpus,corpus);assert.equal(call.runtime,options.runtime);
    assert.equal(call.probeFactory,options.probeFactory);assert.equal(call.plan.fingerprint,fingerprint);
    assert.equal(call.plan.safetyFactor,2);assert.equal(call.plan.budgets.diskReserveBytes,GiB);
    assert.equal(call.plan.budgets.minAvailableMemoryBytes,512*1024**2);assert.equal(call.plan.budgets.maxRssBytes,2*GiB);
    assert.equal(call.directory,join(options.directory,String(call.plan.stage)));await access(call.directory);
    assert.ok(Date.parse(call.plan.deadlineAt)<=Date.parse(options.workDeadlineAt));
  }
});
test('build preparation and lower stages consume the original fixed work cutoff',async t=>{
  const started=Date.now();let current=started+30*MINUTE;
  const options=await setup(t,{now:()=>current,workDeadlineAt:new Date(started+80*HOUR-15*MINUTE).toISOString()});
  options.qualify=async args=>{options.calls.push(args);current+=args.plan.stage==='small'?5*MINUTE:args.plan.stage===1000?120*MINUTE:args.plan.stage===10000?330*MINUTE:0;return report(args);};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'SUCCEEDED');
  assert.equal(Date.parse(result.small.plan.deadlineAt),started+35*MINUTE);
  assert.equal(result.hundredThousand.plan.deadlineAt,options.workDeadlineAt);
  assert.equal(result.hundredThousand.plan.budgets.maxWallTimeMs,4300*MINUTE);
});
test('a short remaining work budget caps both absolute deadline and monotonic timer',async t=>{
  const started=Date.now(),options=await setup(t,{now:()=>started,workDeadlineAt:new Date(started+MINUTE).toISOString()});
  options.qualify=async args=>report(args,{status:'ABORTED'});
  const result=await chain.runLocalScaleChain(options);
  assert.equal(result.small.plan.deadlineAt,options.workDeadlineAt);assert.equal(result.small.plan.budgets.maxWallTimeMs,MINUTE);
});
for(const value of [undefined,null,'tomorrow','2026-02-30T00:00:00.000Z',0,Infinity])test(`invalid work deadline refuses all local stages: ${value}`,async t=>{
  const options=await setup(t,{workDeadlineAt:value}),result=await chain.runLocalScaleChain(options);
  assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-deadline-invalid');assert.equal(options.calls.length,0);
  await assert.rejects(access(join(options.directory,'small')));
});
test('expired and overlong work cutoffs cannot admit a local stage',async t=>{
  for(const delta of [0,-1,80*HOUR-15*MINUTE+1,80*HOUR]){
    const started=Date.now(),options=await setup(t,{now:()=>started,workDeadlineAt:new Date(started+delta).toISOString()});
    const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(options.calls.length,0);
    assert.equal(result.failureCode,delta>0?'chain-deadline-invalid':'chain-deadline-exhausted');
  }
});
for(const [stage,count] of [['small',1],[1000,2],[10000,3]])for(const status of ['FAILED','ABORTED','NOT_ADMITTED','NOT_RUN','AWAITING_RESTART'])test(`${status} ${stage} prevents the 100k stage`,async t=>{
  const options=await setup(t);options.qualify=async args=>{options.calls.push(args);return report(args,{status:args.plan.stage===stage?status:'SUCCEEDED'});};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,status);
  assert.equal(result.hundredThousand,undefined);assert.equal(options.calls.length,count);
  await assert.rejects(access(join(options.directory,'100000')));
});
for(const field of [...Object.keys(dataset),'humanPid','agentPid','unexpectedField'])test(`100k rejects a changed current ${field}`,async t=>{
  const options=await setup(t);let reads=0;
  options.runtime.identity=async()=>({...dataset,humanPid:101,agentPid:201,...(++reads===3?{[field]:'changed'}:{})});
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');
  assert.equal(result.failureCode,'interstage-runtime-identity-mismatch');assert.equal(result.hundredThousand,undefined);assert.equal(options.calls.length,3);
});
test('a replacement dataset cannot qualify merely by matching the 10k report',async t=>{
  const options=await setup(t);let reads=0;
  options.runtime.identity=async()=>({...dataset,...(++reads===3?{containerId:'replacement'}:{}),humanPid:101,agentPid:201});
  options.qualify=async args=>{options.calls.push(args);const value=report(args);if(args.plan.stage===10000)value.restart.before.containerId=value.restart.after.containerId='replacement';return value;};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-runtime-identity-mismatch');assert.equal(options.calls.length,3);
});
for(const change of [{runId:'different'},{fingerprint:{...fingerprint,corpus:'different'}},{fingerprint:{...fingerprint,runtime:'different'}},{stage:100000},{documentCount:9999},{previousReport:null}])test(`100k rejects changed 10k report origin: ${JSON.stringify(change)}`,async t=>{
  const options=await setup(t);options.qualify=async args=>{options.calls.push(args);return report(args,args.plan.stage===10000?change:{});};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-report-identity-mismatch');assert.equal(options.calls.length,3);
});
test('missing lower-stage history cannot reach 100k',async t=>{
  const options=await setup(t);options.qualify=async args=>{options.calls.push(args);const value=report(args);if(args.plan.stage===10000)value.previousReport={...value.previousReport,previousReport:undefined};return value;};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-report-identity-mismatch');assert.equal(options.calls.length,3);
});
test('clock rollback between stages cannot refund any elapsed time',async t=>{
  const started=Date.now();let current=started;const options=await setup(t,{now:()=>current});
  options.qualify=async args=>{options.calls.push(args);current=started+(args.plan.stage===10000?1:2)*MINUTE;return report(args);};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-clock-invalid');assert.equal(options.calls.length,3);
});
test('a slow identity read cannot start 100k after the work cutoff',async t=>{
  const started=Date.now();let current=started,reads=0;const options=await setup(t,{now:()=>current});
  options.runtime.identity=async()=>{if(++reads===3)current=Date.parse(options.workDeadlineAt);return {...dataset,humanPid:101,agentPid:201};};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-deadline-exhausted');assert.equal(options.calls.length,3);
});
for(const stage of ['small',1000,10000,100000])test(`existing ${stage} evidence is not imported, resumed, or overwritten`,async t=>{
  const options=await setup(t);await mkdir(join(options.directory,String(stage)));await writeFile(join(options.directory,String(stage),'report.json'),'PRIVATE_SENTINEL');
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'FAILED');assert.equal(result.failureCode,'chain-prerequisite-failed');
  assert.equal(options.calls.length,['small',1000,10000,100000].indexOf(stage));assert.equal(await readFile(join(options.directory,String(stage),'report.json'),'utf8'),'PRIVATE_SENTINEL');assert.ok(!JSON.stringify(result).includes('PRIVATE_SENTINEL'));
});

const boundaryCases = [
  {name:'exact 216-minute/1-GiB boundary',elapsed:216*MINUTE,rss:GiB,expected:'SUCCEEDED'},
  {name:'one millisecond beyond the 216-minute boundary',elapsed:216*MINUTE+1,rss:GiB,expected:'NOT_ADMITTED'},
  {name:'one byte beyond the 1-GiB prior RSS boundary',elapsed:216*MINUTE,rss:GiB+1,expected:'NOT_ADMITTED'},
];
for(const boundary of boundaryCases)test(`unmodified admission applies factor2 to ${boundary.name}`,async t=>{
  // Freeze wall time only at this mathematical boundary: any real preparation
  // elapsed after plan creation correctly consumes the available 72-hour margin.
  const current=Date.now();t.mock.method(Date,'now',()=>current);
  const options=await setup(t,{now:()=>current});let restarted=false,observations=0;const mutations=[];
  options.runtime.evidenceClass='owned-real-process';
  options.runtime.observe=async()=>{observations++;return {observedAt:new Date().toISOString(),diskFreeBytes:10*GiB,storageDiskFreeBytes:10*GiB,databaseDiskFreeBytes:10*GiB,availableMemoryBytes:10*GiB,rssBytes:1,storageBytes:0,databaseBytes:0,rssCoverage:'process-tree',databaseFilesystemVerified:true};};
  options.runtime.identity=async()=>({...dataset,humanPid:restarted?102:101,agentPid:restarted?202:201});
  options.runtime.restart=async()=>{restarted=true;};
  options.qualify=async args=>{
    if(args.plan.stage!==100000)return report(args,{evidenceClass:'owned-real-process',metrics:{totalElapsedMs:args.plan.stage===10000?boundary.elapsed:1,diskGrowthBytes:0,peakRssBytes:args.plan.stage===10000?boundary.rss:1}});
    return runQualification({...args,execute:async({count})=>{mutations.push(count);return {documentIds:Array.from({length:count},(_,i)=>String(i))};},verify:async()=>{}});
  };
  const result=await chain.runLocalScaleChain(options);
  assert.equal(result.hundredThousand.admission.projection.multiplier,20);
  assert.equal(result.hundredThousand.admission.projection.totalElapsedMs,boundary.elapsed*20);
  assert.equal(result.hundredThousand.admission.projection.peakRssBytes,boundary.rss*2);
  assert.equal(result.status,boundary.expected);assert.ok(observations>=1);
  assert.deepEqual(mutations,boundary.expected==='SUCCEEDED'?[100000]:[]);
  if(boundary.expected==='NOT_ADMITTED')await assert.rejects(access(join(options.directory,'100000','operations.jsonl')));
});
test('fresh resource scarcity rejects 100k despite successful complete lower-stage reports',async t=>{
  const options=await setup(t,{now:Date.now});const mutations=[];
  options.runtime.evidenceClass='owned-real-process';
  options.runtime.observe=async()=>({observedAt:new Date().toISOString(),diskFreeBytes:GiB-1,storageDiskFreeBytes:GiB-1,databaseDiskFreeBytes:GiB-1,availableMemoryBytes:10*GiB,rssBytes:1,storageBytes:0,databaseBytes:0,rssCoverage:'process-tree',databaseFilesystemVerified:true});
  options.qualify=async args=>args.plan.stage!==100000?report(args,{evidenceClass:'owned-real-process'}):runQualification({...args,execute:async()=>{mutations.push(true);},verify:async()=>{}});
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.deepEqual(mutations,[]);
  assert.match(result.hundredThousand.admission.reasons.join(';'),/disk reserve/);await assert.rejects(access(join(options.directory,'100000','operations.jsonl')));
});
for(const change of [{runId:'other-run'},{stage:1000},{documentCount:1},{fingerprint:{...fingerprint,code:'f'.repeat(40)}},{status:'FAILED'}])test(`100k rechecks the origin of every retained earlier report: ${Object.keys(change)[0]}`,async t=>{
  const options=await setup(t);
  options.qualify=async args=>{
    options.calls.push(args);const value=report(args);
    if(args.plan.stage===10000)Object.assign(value.previousReport.previousReport,change);
    return value;
  };
  const result=await chain.runLocalScaleChain(options);
  assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-report-identity-mismatch');assert.equal(options.calls.length,3);
});
for(const value of [NaN,Infinity,1.5,'1',null])test(`invalid current clock refuses the first stage: ${value}`,async t=>{
  const options=await setup(t,{now:()=>value,workDeadlineAt:new Date(Date.now()+MINUTE).toISOString()});
  const result=await chain.runLocalScaleChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-clock-invalid');assert.equal(options.calls.length,0);
});
test('elapsed work deadline after the fourth stage prevents overall success',async t=>{
  const started=Date.now();let current=started;const options=await setup(t,{now:()=>current});
  options.qualify=async args=>{const value=report(args);if(args.plan.stage===100000)current=Date.parse(options.workDeadlineAt);return value;};
  const result=await chain.runLocalScaleChain(options);assert.equal(result.hundredThousand.status,'SUCCEEDED');assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-deadline-exhausted');
});

test('explicit local80h4 byte budget applies to all four stages without changing disk time or safety factor',async t=>{
  const options=await setup(t,{resourceBudget:{maxRssBytes:4*GiB,minAvailableMemoryBytes:4*GiB}});
  const result=await chain.runLocalScaleChain(options);
  assert.equal(result.status,'SUCCEEDED');assert.equal(options.calls.length,4);
  for(const {plan} of options.calls){assert.equal(plan.budgets.maxRssBytes,4*GiB);assert.equal(plan.budgets.minAvailableMemoryBytes,4*GiB);assert.equal(plan.budgets.diskReserveBytes,GiB);assert.equal(plan.safetyFactor,2);}
});
test('invalid explicit resource budgets refuse before creating stage directories',async t=>{
  for(const resourceBudget of [null,{}, {maxRssBytes:4*GiB}, {maxRssBytes:5*GiB,minAvailableMemoryBytes:4*GiB}, {maxRssBytes:4*GiB,minAvailableMemoryBytes:4*GiB,extra:true}]){
    const options=await setup(t,{resourceBudget});const result=await chain.runLocalScaleChain(options);
    assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'chain-resource-budget-invalid');assert.equal(options.calls.length,0);await assert.rejects(access(join(options.directory,'small')));
  }
});
