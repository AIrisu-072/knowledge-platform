import test from 'node:test';
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp,mkdir,readFile,writeFile,lstat,rm,symlink,rename,chmod} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {setTimeout as delay} from 'node:timers/promises';

const launcher=await import('../local-launch.mjs').catch(()=>({}));
const moduleUrl=new URL('../local-launch.mjs',import.meta.url).href;
const executeFile=promisify(execFile);
const sourceHead='a'.repeat(40);
const runId='631081c1-bc45-4475-9f92-d87912342d43';
const dependencies={platform:'linux',arch:'x64',execute:async(command,args)=>({stdout:command==='findmnt'?'ext4\n':args.includes('rev-parse')?sourceHead+'\n':''})};
test('watchdog consumes preparation time and reserves its final120 seconds inside the80-hour limit',()=>{
 assert.equal(typeof launcher.remainingLocalWatchdogSeconds,'function');const started=Date.now();
 assert.equal(launcher.remainingLocalWatchdogSeconds(started,started),80*3600-120);
 assert.equal(launcher.remainingLocalWatchdogSeconds(started,started+42100),80*3600-120-43);
 assert.throws(()=>launcher.remainingLocalWatchdogSeconds(started,started+80*3600*1000),/exhausted/);
});
async function fixture(t) {
  const root=await mkdtemp(join(tmpdir(),'local-launch-')); t.after(()=>rm(root,{recursive:true,force:true}));
  const repositoryDirectory=join(root,'repository'),evidenceRoot=join(root,'evidence-root');
  await mkdir(repositoryDirectory,{mode:0o700}); await mkdir(evidenceRoot,{mode:0o700});
  return {root,repositoryDirectory,evidenceRoot};
}
async function contextFixture(t) {
  const {root}=await fixture(t),launchDirectory=join(root,'launch');await mkdir(launchDirectory,{mode:0o700});
  const evidenceDirectory=join(launchDirectory,'evidence');await mkdir(evidenceDirectory,{mode:0o700});
  const env={KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true',KP_DOCUMENT_LOAD_SOURCE_HEAD:sourceHead,
    KP_DOCUMENT_LOAD_LOCAL_STARTED_AT:String(Date.now()),KP_DOCUMENT_LOAD_LOCAL_RUN_ID:runId,
    KP_DOCUMENT_LOAD_LAUNCH_DIR:launchDirectory,KP_POC_EVIDENCE_DIR:evidenceDirectory};
  return {root,launchDirectory,evidenceDirectory,env};
}
async function waitForFile(path,expected) {
  const deadline=Date.now()+5000;
  while(Date.now()<deadline) {
    try {if(await readFile(path,'utf8')===expected)return expected;} catch(error) {if(error.code!=='ENOENT')throw error;}
    await delay(10);
  }
  assert.fail('Timed out waiting for the fake runtime observation');
}

test('local context requires explicit mode, immutable source/run/start identity, and private real ext4 evidence',async t=>{
  assert.equal(typeof launcher.validateLocalLaunchContext,'function');
  const {env,launchDirectory,evidenceDirectory}=await contextFixture(t);
  const context=await launcher.validateLocalLaunchContext(env,dependencies);
  assert.deepEqual(context,{startedAt:Number(env.KP_DOCUMENT_LOAD_LOCAL_STARTED_AT),runId,sourceHead,launchDirectory,evidenceDirectory});
  for(const override of [{KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'false'},{GITHUB_ACTIONS:'true'},{KP_DOCUMENT_LOAD_SOURCE_HEAD:'bad'},
    {KP_DOCUMENT_LOAD_LOCAL_RUN_ID:'bad'},{KP_DOCUMENT_LOAD_LOCAL_STARTED_AT:String(Date.now()+100000)},
    {KP_DOCUMENT_LOAD_LOCAL_STARTED_AT:'1'}, {KP_POC_EVIDENCE_DIR:launchDirectory},{TEST_DATABASE_URL:'postgres://private'}]) {
    await assert.rejects(launcher.validateLocalLaunchContext({...env,...override},dependencies));
  }
  await assert.rejects(launcher.validateLocalLaunchContext(env,{...dependencies,execute:async()=>({stdout:'overlay\n'})}),/ext4/);
});

test('startup acknowledgement is private, exclusive, source-bound, and records real process identity',async t=>{
  assert.equal(typeof launcher.recordLocalStartup,'function');
  const {env,evidenceDirectory,launchDirectory}=await contextFixture(t);
  const context=await launcher.validateLocalLaunchContext(env,dependencies);
  const directory=join(evidenceDirectory,'run-private');await mkdir(directory,{mode:0o700});
  await assert.rejects(launcher.recordLocalStartup(context,{pid:process.pid,sourceHead:'b'.repeat(40),directory}));
  await assert.rejects(launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory:launchDirectory}));
  await launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory});
  const ack=JSON.parse(await readFile(join(launchDirectory,'started.json'),'utf8'));
  assert.equal(ack.runId,runId);assert.equal(ack.sourceHead,sourceHead);assert.equal(ack.directory,directory);assert.equal(ack.pid,process.pid);
  assert.match(ack.startTicks,/^\d+$/);assert.match(ack.bootId,/^[a-f0-9-]{36}$/);
  assert.equal((await lstat(join(launchDirectory,'started.json'))).mode&0o777,0o600);
  await assert.rejects(launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory}),{code:'EEXIST'});
});

test('startup publication retains a pre-existing acknowledgement and refuses a second publisher',async t=>{
  const {env,evidenceDirectory,launchDirectory}=await contextFixture(t);
  const context=await launcher.validateLocalLaunchContext(env,dependencies);
  const directory=join(evidenceDirectory,'run-private');await mkdir(directory,{mode:0o700});
  const path=join(launchDirectory,'started.json');await writeFile(path,'retained incomplete evidence',{mode:0o600});
  const before=await lstat(path);
  await assert.rejects(launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory}),{code:'EEXIST'});
  assert.equal(await readFile(path,'utf8'),'retained incomplete evidence');
  assert.equal((await lstat(path)).ino,before.ino);
  await assert.rejects(lstat(join(launchDirectory,'.startup-ack','ready')),{code:'ENOENT'});
  await assert.rejects(launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory}),{code:'EEXIST'});
});

test('readers reject non-private or symlinked publication markers and committed malformed acknowledgement',async t=>{
  const {root,env,evidenceDirectory,launchDirectory}=await contextFixture(t);
  const context=await launcher.validateLocalLaunchContext(env,dependencies);
  const directory=join(evidenceDirectory,'run-private');await mkdir(directory,{mode:0o700});
  await launcher.recordLocalStartup(context,{pid:process.pid,sourceHead,directory});
  await writeFile(join(launchDirectory,'launcher.json'),JSON.stringify({schemaVersion:1,...context,supervisor:{pid:process.pid}}),{mode:0o600});
  const ready=join(launchDirectory,'.startup-ack','ready'),retained=join(root,'retained-ready');
  await rename(ready,retained);await symlink(retained,ready);
  await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),/real|symlink/i);
  await rm(ready);await writeFile(ready,'foreign file',{mode:0o600});
  await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),/real/i);
  assert.equal(await readFile(ready,'utf8'),'foreign file');
  await rm(ready);await rename(retained,ready);await chmod(ready,0o755);
  await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),/private/i);
  await chmod(ready,0o700);await writeFile(join(launchDirectory,'started.json'),'{');
  await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),SyntaxError);
  await rm(join(launchDirectory,'started.json'));
  await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),{code:'ENOENT'});
});

test('launcher and status wait for publication while the real acknowledgement writer is paused after a partial write',async t=>{
  const input=await fixture(t),runtimeDirectory=join(input.repositoryDirectory,'tools/document-poc-runtime');await mkdir(runtimeDirectory,{recursive:true,mode:0o700});
  await writeFile(join(runtimeDirectory,'run.mjs'),`import fs from 'node:fs/promises';import {join} from 'node:path';import {syncBuiltinESMExports} from 'node:module';import {setTimeout as delay} from 'node:timers/promises';
const root=process.env.KP_DOCUMENT_LOAD_LAUNCH_DIR,write=fs.writeFile;
const guard=setTimeout(()=>process.exit(1),7000);
async function waitFor(name) {while(true) {try {await fs.access(join(root,name));return;} catch(error) {if(error.code!=='ENOENT')throw error;}await delay(10);}}
fs.writeFile=async(path,data,options)=>{if(path!==join(root,'started.json'))return write(path,data,options);
await write(path,data.slice(0,5),options);await write(join(root,'partial.txt'),'partial',{mode:0o600});
await waitFor('release.txt');return write(path,data.slice(5),{flag:'a'});};syncBuiltinESMExports();
const {validateLocalLaunchContext,recordLocalStartup}=await import(${JSON.stringify(moduleUrl)});
const context=await validateLocalLaunchContext(process.env,{execute:async()=>({stdout:'ext4\\n'})});
const directory=join(context.evidenceDirectory,'run-fake');await fs.mkdir(directory,{mode:0o700});
await recordLocalStartup(context,{pid:process.pid,sourceHead:context.sourceHead,directory});
await waitFor('finish.txt');clearTimeout(guard);await write(join(root,'finished.txt'),'finished',{mode:0o600});`,{mode:0o600});
  const plan=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{PATH:process.env.PATH}},dependencies);
  let outcome;
  const launch=launcher.launchLocalRun(plan,{startupTimeoutMs:5000}).then(value=>outcome={value},error=>outcome={error});
  try {
    await waitForFile(join(plan.context.launchDirectory,'partial.txt'),'partial');
    assert.equal(await readFile(join(plan.context.launchDirectory,'started.json'),'utf8'),'{\n  "');
    assert.equal((await launcher.readLocalLaunchStatus(plan.context.launchDirectory)).status,'starting-or-stopping');
    assert.equal(outcome,undefined,'Partial acknowledgement must not settle the launch');
    await writeFile(join(plan.context.launchDirectory,'release.txt'),'release',{mode:0o600});
    await launch;if(outcome.error)throw outcome.error;
    assert.equal(outcome.value.status,'started');assert.equal(outcome.value.runId,plan.context.runId);assert.equal(outcome.value.sourceHead,sourceHead);
    assert.equal((await lstat(join(plan.context.launchDirectory,'started.json'))).nlink,1);
    assert.equal((await launcher.readLocalLaunchStatus(plan.context.launchDirectory)).status,'running');
  } finally {
    await writeFile(join(plan.context.launchDirectory,'release.txt'),'release',{mode:0o600});
    await writeFile(join(plan.context.launchDirectory,'finish.txt'),'finish',{mode:0o600});
    await launch;
    await waitForFile(join(plan.context.launchDirectory,'finished.txt'),'finished');
  }
});

test('launch plan fixes the entire 80-hour command and refuses dirty or mismatched source and foreign roots',async t=>{
  assert.equal(typeof launcher.prepareLocalLaunchPlan,'function');
  const input=await fixture(t);
  const plan=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{PATH:process.env.PATH,KP_DSI_PDFIUM_RUNTIME_DIR:'/approved/pdfium',
    FONTCONFIG_FILE:'/approved/fonts.conf',CARGO_BUILD_JOBS:'128',CARGO_PROFILE_DEV_DEBUG:'2',CARGO_INCREMENTAL:'1'}},dependencies);
  assert.equal(plan.command,'flock');
  assert.deepEqual(plan.args,['--nonblock','--no-fork','--conflict-exit-code','73',join(input.evidenceRoot,'.launcher.lock'),
    'timeout','--signal=TERM','--kill-after=120s','80h',process.execPath,join(input.repositoryDirectory,'tools/document-poc-runtime/run.mjs')]);
  assert.equal(plan.env.KP_DOCUMENT_LOAD_HUNDRED_THOUSAND,'true');assert.equal(plan.env.GITHUB_ACTIONS,undefined);
  assert.equal(plan.env.KP_DOCUMENT_LOAD_SOURCE_HEAD,sourceHead);assert.equal(plan.env.KP_POC_EVIDENCE_DIR,join(plan.context.launchDirectory,'evidence'));
  assert.equal(plan.env.KP_DSI_PDFIUM_RUNTIME_DIR,'/approved/pdfium');
  assert.equal(plan.env.FONTCONFIG_FILE,'/approved/fonts.conf');
  assert.equal(plan.env.CARGO_BUILD_JOBS,'2');assert.equal(plan.env.CARGO_PROFILE_DEV_DEBUG,'0');assert.equal(plan.env.CARGO_INCREMENTAL,'0');
  for(const execute of [async(command,args)=>({stdout:command==='findmnt'?'ext4\n':args.includes('rev-parse')?'b'.repeat(40)+'\n':''}),
    async(command,args)=>({stdout:command==='findmnt'?'ext4\n':args.includes('rev-parse')?sourceHead+'\n':' M source\n'})]) {
    await assert.rejects(launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{}},{...dependencies,execute}),/source|clean/i);
  }
  const foreign=join(input.root,'foreign');await mkdir(foreign,{mode:0o700});await writeFile(join(foreign,'unrelated'),'retain');
  await assert.rejects(launcher.prepareLocalLaunchPlan({...input,evidenceRoot:foreign,expectedHead:sourceHead,runtimeEnvironment:{}},dependencies),/dedicated|foreign/);
  await assert.rejects(launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{GITHUB_ACTIONS:'true'}},dependencies),/GitHub/);
});

test('launch context rejects a symlinked evidence tree without changing it',async t=>{
  assert.equal(typeof launcher.validateLocalLaunchContext,'function');
  const {root,env,evidenceDirectory}=await contextFixture(t),alias=join(root,'alias');await symlink(evidenceDirectory,alias);
  await assert.rejects(launcher.validateLocalLaunchContext({...env,KP_POC_EVIDENCE_DIR:alias},dependencies));
  assert.equal((await lstat(alias)).isSymbolicLink(),true);
});

test('replacement of the owned lock after preparation fails before spawning any process',async t=>{
  assert.equal(typeof launcher.launchLocalRun,'function');
  const input=await fixture(t),plan=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{}},dependencies);
  const lock=join(input.evidenceRoot,'.launcher.lock'),foreign=join(input.root,'foreign-lock');
  await writeFile(foreign,'foreign',{mode:0o600});await rename(lock,join(input.evidenceRoot,'retained-lock'));await symlink(foreign,lock);
  await assert.rejects(launcher.launchLocalRun(plan),/lock|symbolic/i);
  await assert.rejects(lstat(plan.stdoutPath),{code:'ENOENT'});
  assert.equal(await readFile(foreign,'utf8'),'foreign');
});

test('startup timeout leaves private evidence and never retries or kills the owned child',async t=>{
  assert.equal(typeof launcher.launchLocalRun,'function');
  const input=await fixture(t),runtimeDirectory=join(input.repositoryDirectory,'tools/document-poc-runtime');await mkdir(runtimeDirectory,{recursive:true,mode:0o700});
  await writeFile(join(runtimeDirectory,'run.mjs'),`import {writeFile} from 'node:fs/promises';import {join} from 'node:path';
setTimeout(()=>writeFile(join(process.env.KP_DOCUMENT_LOAD_LAUNCH_DIR,'not-killed.txt'),'alive',{mode:0o600}),200);setTimeout(()=>{},350);`,{mode:0o600});
  const plan=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{PATH:process.env.PATH}},dependencies);
  await assert.rejects(launcher.launchLocalRun(plan,{startupTimeoutMs:100}),/timed out/);
  await delay(300);
  assert.equal(await readFile(join(plan.context.launchDirectory,'not-killed.txt'),'utf8'),'alive');
  await assert.rejects(launcher.launchLocalRun(plan),{code:'EEXIST'});
  assert.equal((await lstat(plan.stdoutPath)).mode&0o777,0o600);assert.equal((await lstat(plan.stderrPath)).mode&0o777,0o600);
});

test('a detached fake runtime survives launcher exit, duplicate launch fails closed, and status is read-only',async t=>{
  assert.equal(typeof launcher.launchLocalRun,'function');
  const input=await fixture(t),runtimeDirectory=join(input.repositoryDirectory,'tools/document-poc-runtime');await mkdir(runtimeDirectory,{recursive:true,mode:0o700});
  const runtime=`import {mkdir,writeFile} from 'node:fs/promises';import {join} from 'node:path';import {validateLocalLaunchContext,recordLocalStartup} from ${JSON.stringify(moduleUrl)};
const context=await validateLocalLaunchContext(process.env,{execute:async()=>({stdout:'ext4\\n'})});
const directory=join(context.evidenceDirectory,'run-fake');await mkdir(directory,{mode:0o700});await recordLocalStartup(context,{pid:process.pid,sourceHead:context.sourceHead,directory});
setTimeout(()=>writeFile(join(context.launchDirectory,'survived.txt'),'survived',{mode:0o600}),500);setTimeout(()=>{},1700);`;
  await writeFile(join(runtimeDirectory,'run.mjs'),runtime,{mode:0o600});
  const launchScript=join(input.root,'launch.mjs');
  await writeFile(launchScript,`import {prepareLocalLaunchPlan,launchLocalRun} from ${JSON.stringify(moduleUrl)};
const plan=await prepareLocalLaunchPlan(${JSON.stringify({...input,expectedHead:sourceHead,runtimeEnvironment:{PATH:process.env.PATH}})},
{now:()=>Date.now()-2*3600*1000,execute:async(command,args)=>({stdout:command==='findmnt'?'ext4\\n':args.includes('rev-parse')?'${sourceHead}\\n':''})});
console.log(JSON.stringify(await launchLocalRun(plan)));`,{mode:0o600});
  const result=JSON.parse((await executeFile(process.execPath,[launchScript],{timeout:3000,encoding:'utf8'})).stdout);
  assert.equal(result.status,'started');
  const before=await readFile(join(result.launchDirectory,'launcher.json'),'utf8');
  const launchRecord=JSON.parse(before);assert.ok(launchRecord.watchdogSeconds<=78*3600-120);assert.ok(launchRecord.watchdogSeconds>77*3600);
  const status=await launcher.readLocalLaunchStatus(result.launchDirectory);
  assert.equal(status.status,'running');assert.equal(status.runId,result.runId);
  const duplicate=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{PATH:process.env.PATH}},dependencies);
  await assert.rejects(launcher.launchLocalRun(duplicate),/already running|lock/i);
  await delay(550);
  assert.equal(await readFile(join(result.launchDirectory,'survived.txt'),'utf8'),'survived');
  assert.equal(await readFile(join(result.launchDirectory,'launcher.json'),'utf8'),before);
  await delay(1400);
  assert.equal((await launcher.readLocalLaunchStatus(result.launchDirectory)).status,'stopped');
});

test('launcher retains an explicit approved byte budget in validated context and child environment',async t=>{
 const input=await fixture(t),budgetEnv={KP_DOCUMENT_LOAD_LOCAL_MAX_RSS_BYTES:'4294967296',KP_DOCUMENT_LOAD_LOCAL_MIN_AVAILABLE_MEMORY_BYTES:'4294967296'};
 const plan=await launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:budgetEnv},dependencies);
 assert.deepEqual(plan.context.resourceBudget,{maxRssBytes:4294967296,minAvailableMemoryBytes:4294967296});
 for(const [key,value] of Object.entries(budgetEnv))assert.equal(plan.env[key],value);
 const context=await launcher.validateLocalLaunchContext(plan.env,dependencies);assert.deepEqual(context.resourceBudget,plan.context.resourceBudget);
});
test('launcher rejects an incomplete explicit budget before claiming evidence root',async t=>{
 const input=await fixture(t);
 await assert.rejects(launcher.prepareLocalLaunchPlan({...input,expectedHead:sourceHead,runtimeEnvironment:{KP_DOCUMENT_LOAD_LOCAL_MAX_RSS_BYTES:'4294967296'}},dependencies),/resource budget/i);
 await assert.rejects(lstat(join(input.evidenceRoot,'.document-load-owned-root.json')));
});

test('status rejects startup acknowledgement that substituted the approved resource budget',async t=>{
 const {env,launchDirectory,evidenceDirectory}=await contextFixture(t);
 const resourceBudget={maxRssBytes:4294967296,minAvailableMemoryBytes:4294967296};
 const directory=join(evidenceDirectory,'owned-runtime');await mkdir(directory,{mode:0o700});
 const record={schemaVersion:1,runId,sourceHead,startedAt:Number(env.KP_DOCUMENT_LOAD_LOCAL_STARTED_AT),launchDirectory,evidenceDirectory,resourceBudget,supervisor:{pid:99999999}};
 await writeFile(join(launchDirectory,'launcher.json'),JSON.stringify(record),{mode:0o600});
 await mkdir(join(launchDirectory,'.startup-ack'),{mode:0o700});await mkdir(join(launchDirectory,'.startup-ack','ready'),{mode:0o700});
 await writeFile(join(launchDirectory,'started.json'),JSON.stringify({schemaVersion:1,runId,sourceHead,startedAt:record.startedAt,directory,pid:99999998,parentPid:99999999,processGroupId:99999999,sessionId:99999999,resourceBudget:{...resourceBudget,maxRssBytes:2147483648}}),{mode:0o600});
 await assert.rejects(launcher.readLocalLaunchStatus(launchDirectory),/acknowledgement mismatch/i);
});
