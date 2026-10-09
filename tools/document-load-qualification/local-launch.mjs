import {execFile,spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {constants} from 'node:fs';
import {lstat,mkdir,open,readFile,readdir,realpath,writeFile} from 'node:fs/promises';
import {dirname,isAbsolute,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import {createLocalRunBudget} from './local-budget.mjs';
import {assertLocalLinux,ownedPrivateDirectory,verifyExt4Directory} from './local-storage.mjs';

const executeFile=promisify(execFile);
const contexts=new WeakMap(),plans=new WeakMap();
const UUID=/^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/;
const SHA=/^[a-f0-9]{40}$/;
const OTHER_MODES=['KP_DOCUMENT_LOAD_SMALL','KP_DOCUMENT_LOAD_THOUSAND','KP_DOCUMENT_LOAD_TEN_THOUSAND','KP_DOCUMENT_LOAD_PLAN'];
const ENVIRONMENT_KEYS=['PATH','HOME','USER','LOGNAME','LANG','LC_ALL','TMPDIR','XDG_CACHE_HOME','XDG_CONFIG_HOME',
  'CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','PNPM_HOME','PLAYWRIGHT_BROWSERS_PATH','FONTCONFIG_FILE','KP_DSI_PDFIUM_RUNTIME_DIR'];
const probeOptions={timeout:5000,killSignal:'SIGKILL',maxBuffer:65536,encoding:'utf8'};

export function remainingLocalWatchdogSeconds(startedAt,now=Date.now()) {
  const budget=createLocalRunBudget({startedAt,now});
  const seconds=Math.floor((Date.parse(budget.hardDeadlineAt)-now-120000)/1000);
  if(!Number.isSafeInteger(seconds)||seconds<1)throw Error('Local watchdog budget exhausted');
  return seconds;
}

function identityMatches(left,right) {return left.dev===right.dev && left.ino===right.ino;}
function assertEnvironment(env) {
  if(env.GITHUB_ACTIONS==='true')throw Error('Local qualification cannot run as GitHub Actions');
  if(env.TEST_DATABASE_URL!==undefined || env.KP_POC_BINARY_DIR!==undefined || OTHER_MODES.some(key=>env[key]!==undefined))throw Error('Local launch requires only its explicit mode and owned database/source build');
}
function verifiedContext(context) {
  const record=contexts.get(context);if(!record)throw Error('Validated local launch context is required');
  if(!identityMatches(ownedPrivateDirectory(context.launchDirectory),record.launchStat)
    ||!identityMatches(ownedPrivateDirectory(context.evidenceDirectory),record.evidenceStat))throw Error('Local launch directory identity changed');
  return record;
}
async function privateJson(path) {
  const file=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW);
  try {
    const stat=await file.stat();
    if(!stat.isFile() || stat.uid!==process.getuid() || (stat.mode&0o777)!==0o600 || stat.nlink!==1 || stat.size>65536)throw Error('Invalid private launcher evidence');
    return JSON.parse(await file.readFile('utf8'));
  } finally {await file.close();}
}
async function writePrivateJson(path,value) {await writeFile(path,JSON.stringify(value,null,2)+'\n',{mode:0o600,flag:'wx'});}
async function readStartupAcknowledgement(launchDirectory) {
  const publicationDirectory=join(launchDirectory,'.startup-ack');
  try {
    ownedPrivateDirectory(publicationDirectory);
    ownedPrivateDirectory(join(publicationDirectory,'ready'));
  } catch(error) {if(error.code==='ENOENT')return undefined;throw error;}
  // Once published, missing or malformed evidence is an error, never pending.
  return privateJson(join(launchDirectory,'started.json'));
}
async function ownedLockIdentity(lockPath) {
  let lock;
  try {
    lock=await open(lockPath,constants.O_RDONLY|constants.O_NOFOLLOW);
    const stat=await lock.stat();
    if(!stat.isFile()||stat.uid!==process.getuid()||(stat.mode&0o777)!==0o600||stat.nlink!==1)throw Error('Invalid owned launcher lock');
    return stat;
  } catch(error) {throw Error('Owned launcher lock identity is unavailable',{cause:error});}
  finally {await lock?.close();}
}

/** All deadlines refer to the original start. Validation never restarts a clock. */
export async function validateLocalLaunchContext(env,dependencies={}) {
  assertLocalLinux(dependencies);assertEnvironment(env);
  if(env.KP_DOCUMENT_LOAD_HUNDRED_THOUSAND!=='true')throw Error('Explicit local hundred-thousand mode is required');
  const sourceHead=env.KP_DOCUMENT_LOAD_SOURCE_HEAD,runId=env.KP_DOCUMENT_LOAD_LOCAL_RUN_ID;
  if(typeof sourceHead!=='string'||!SHA.test(sourceHead)||typeof runId!=='string'||!UUID.test(runId))throw Error('Invalid local source or run identity');
  const rawStart=env.KP_DOCUMENT_LOAD_LOCAL_STARTED_AT;
  if(typeof rawStart!=='string'||!/^[1-9][0-9]{0,15}$/.test(rawStart))throw Error('Invalid local launch start');
  const startedAt=Number(rawStart);createLocalRunBudget({startedAt,now:dependencies.now?.()??Date.now()});
  const launchDirectory=env.KP_DOCUMENT_LOAD_LAUNCH_DIR,evidenceDirectory=env.KP_POC_EVIDENCE_DIR;
  const launchStat=ownedPrivateDirectory(launchDirectory);
  if(evidenceDirectory!==join(launchDirectory,'evidence'))throw Error('Evidence must belong to the private launch directory');
  const evidenceStat=ownedPrivateDirectory(evidenceDirectory);
  await verifyExt4Directory(launchDirectory,dependencies);await verifyExt4Directory(evidenceDirectory,dependencies);
  const context=Object.freeze({startedAt,runId,launchDirectory,evidenceDirectory,sourceHead});
  contexts.set(context,{launchStat,evidenceStat});verifiedContext(context);return context;
}

async function observeProcess(pid) {
  if(!Number.isSafeInteger(pid)||pid<1)throw Error('Invalid owned process ID');
  const [raw,boot]=await Promise.all([readFile(`/proc/${pid}/stat`,'utf8'),readFile('/proc/sys/kernel/random/boot_id','utf8')]);
  const boundary=raw.lastIndexOf(') '),fields=raw.slice(boundary+2).trim().split(/\s+/),bootId=boot.trim();
  if(boundary<0||!/^\d+$/.test(fields[19]??'')||!UUID.test(bootId)||fields.length<20)throw Error('Owned process identity unavailable');
  return {pid,startTicks:fields[19],bootId,state:fields[0],parentPid:Number(fields[1]),processGroupId:Number(fields[2]),sessionId:Number(fields[3])};
}
function sameProcess(observed,expected) {
  return observed.pid===expected.pid&&observed.startTicks===expected.startTicks&&observed.bootId===expected.bootId
    &&!['Z','X','x'].includes(observed.state);
}

/** Called by the runtime only after validating its clean source and before build. */
export async function recordLocalStartup(context,{pid,sourceHead,directory}) {
  verifiedContext(context);
  if(pid!==process.pid || sourceHead!==context.sourceHead || dirname(directory)!==context.evidenceDirectory)throw Error('Startup acknowledgement identity mismatch');
  ownedPrivateDirectory(directory);
  const observed=await observeProcess(pid);
  verifiedContext(context);
  const publicationDirectory=join(context.launchDirectory,'.startup-ack');
  await mkdir(publicationDirectory,{mode:0o700});
  const publicationStat=ownedPrivateDirectory(publicationDirectory);
  await writePrivateJson(join(context.launchDirectory,'started.json'),{schemaVersion:1,runId:context.runId,
    sourceHead,startedAt:context.startedAt,directory,...observed});
  verifiedContext(context);
  if(!identityMatches(ownedPrivateDirectory(publicationDirectory),publicationStat))throw Error('Startup publication directory identity changed');
  // mkdir publishes completion atomically without replacing any existing path.
  // Failed/partial writes remain private and cannot be retried by another writer.
  await mkdir(join(publicationDirectory,'ready'),{mode:0o700});
  ownedPrivateDirectory(join(publicationDirectory,'ready'));
}

async function verifySource(repositoryDirectory,expectedHead,execute) {
  const {stdout:head}=await execute('git',['rev-parse','HEAD'],{...probeOptions,cwd:repositoryDirectory});
  const {stdout:dirty}=await execute('git',['status','--porcelain','--untracked-files=normal'],{...probeOptions,cwd:repositoryDirectory});
  if(head.trim()!==expectedHead || dirty.trim()!=='')throw Error('Reviewed source must match expected HEAD and be clean');
}

async function claimEvidenceRoot(root,dependencies) {
  const stat=ownedPrivateDirectory(root);await verifyExt4Directory(root,dependencies);
  const markerPath=join(root,'.document-load-owned-root.json');
  const marker={schemaVersion:1,purpose:'document-local-load-evidence',uid:process.getuid(),device:String(stat.dev),inode:String(stat.ino)};
  try {
    const existing=await privateJson(markerPath);
    if(JSON.stringify(existing)!==JSON.stringify(marker))throw Error('Evidence root ownership marker mismatch');
  } catch(error) {
    if(error.code!=='ENOENT')throw error;
    if((await readdir(root)).length!==0)throw Error('A dedicated empty evidence root is required; foreign files are retained');
    await writePrivateJson(markerPath,marker);
  }
  if(!identityMatches(ownedPrivateDirectory(root),stat))throw Error('Evidence root identity changed');
  const lockPath=join(root,'.launcher.lock');
  try {const lock=await open(lockPath,constants.O_CREAT|constants.O_EXCL|constants.O_WRONLY,0o600);await lock.close();}
  catch(error) {if(error.code!=='EEXIST')throw error;}
  const lockStat=await ownedLockIdentity(lockPath);
  return {stat,lockPath,lockStat};
}

/** Preparation is reviewable and does not launch a runtime or alter services. */
export async function prepareLocalLaunchPlan({repositoryDirectory,evidenceRoot,expectedHead,runtimeEnvironment=process.env},dependencies={}) {
  assertLocalLinux(dependencies);assertEnvironment(runtimeEnvironment);
  if(typeof expectedHead!=='string'||!SHA.test(expectedHead))throw Error('Explicit reviewed source HEAD is required');
  if(typeof repositoryDirectory!=='string'||!isAbsolute(repositoryDirectory)||resolve(repositoryDirectory)!==repositoryDirectory
    ||/[\x00-\x1f\x7f]/.test(repositoryDirectory)||(await realpath(repositoryDirectory))!==repositoryDirectory
    ||!(await lstat(repositoryDirectory)).isDirectory())throw Error('Canonical real source checkout is required');
  const execute=dependencies.execute??executeFile;
  await verifySource(repositoryDirectory,expectedHead,execute);
  const root=await claimEvidenceRoot(evidenceRoot,dependencies);
  const runId=randomUUID(),startedAt=dependencies.now?.()??Date.now(),monotonicStartedAt=performance.now(),launchDirectory=join(evidenceRoot,`run-${runId}`);
  await mkdir(launchDirectory,{mode:0o700});const evidenceDirectory=join(launchDirectory,'evidence');await mkdir(evidenceDirectory,{mode:0o700});
  const env=Object.fromEntries(ENVIRONMENT_KEYS.filter(key=>runtimeEnvironment[key]!==undefined).map(key=>[key,runtimeEnvironment[key]]));
  Object.assign(env,{KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true',KP_DOCUMENT_LOAD_SOURCE_HEAD:expectedHead,
    KP_DOCUMENT_LOAD_LOCAL_STARTED_AT:String(startedAt),KP_DOCUMENT_LOAD_LOCAL_RUN_ID:runId,
    KP_DOCUMENT_LOAD_LAUNCH_DIR:launchDirectory,KP_POC_EVIDENCE_DIR:evidenceDirectory,
    CARGO_BUILD_JOBS:'2',CARGO_PROFILE_DEV_DEBUG:'0',CARGO_INCREMENTAL:'0'});
  const context=await validateLocalLaunchContext(env,dependencies);
  const args=Object.freeze(['--nonblock','--no-fork','--conflict-exit-code','73',root.lockPath,
    'timeout','--signal=TERM','--kill-after=120s','80h',process.execPath,join(repositoryDirectory,'tools/document-poc-runtime/run.mjs')]);
  const plan=Object.freeze({command:'flock',args,cwd:repositoryDirectory,env:Object.freeze(env),context,
    stdoutPath:join(launchDirectory,'stdout.log'),stderrPath:join(launchDirectory,'stderr.log')});
  plans.set(plan,{evidenceRoot,rootStat:root.stat,lockPath:root.lockPath,lockStat:root.lockStat,execute,monotonicStartedAt});return plan;
}

function assertAck(ack,context,supervisor) {
  if(ack?.schemaVersion!==1||ack.runId!==context.runId||ack.sourceHead!==context.sourceHead||ack.startedAt!==context.startedAt
    ||typeof ack.directory!=='string'||dirname(ack.directory)!==context.evidenceDirectory
    ||ack.processGroupId!==supervisor.pid||ack.sessionId!==supervisor.pid||ack.parentPid!==supervisor.pid)throw Error('Runtime startup acknowledgement mismatch');
  ownedPrivateDirectory(ack.directory);
}

/** Returns only after a live, owned runtime acknowledges the exact launch identity. */
export async function launchLocalRun(plan,{startupTimeoutMs=30000}={}) {
  const prepared=plans.get(plan);if(!prepared)throw Error('Prepared local launch plan is required');
  if(!Number.isSafeInteger(startupTimeoutMs)||startupTimeoutMs<1||startupTimeoutMs>30000)throw Error('Startup observation must be bounded by 30 seconds');
  verifiedContext(plan.context);createLocalRunBudget({startedAt:plan.context.startedAt});
  await verifySource(plan.cwd,plan.context.sourceHead,prepared.execute);
  if(!identityMatches(ownedPrivateDirectory(prepared.evidenceRoot),prepared.rootStat))throw Error('Evidence root identity changed');
  if(!identityMatches(await ownedLockIdentity(prepared.lockPath),prepared.lockStat))throw Error('Owned launcher lock identity changed');
  let child,out,err,spawnError,watchdogSeconds;
  try {
    out=await open(plan.stdoutPath,'wx',0o600);err=await open(plan.stderrPath,'wx',0o600);
    const effectiveNow=Math.max(Date.now(),plan.context.startedAt+Math.ceil(performance.now()-prepared.monotonicStartedAt));
    watchdogSeconds=remainingLocalWatchdogSeconds(plan.context.startedAt,effectiveNow);
    const args=[...plan.args];args[args.indexOf('80h')]=`${watchdogSeconds}s`;
    child=spawn(plan.command,args,{cwd:plan.cwd,env:plan.env,detached:true,stdio:['ignore',out.fd,err.fd]});
    child.once('error',error=>{spawnError=error;});
  } finally {await out?.close();await err?.close();}
  // No pipe/socket keeps the launching SSH session attached to the runtime.
  const deadline=Date.now()+startupTimeoutMs;
  try {
    if(!child.pid)throw Error('Owned launcher process could not start');
    let supervisor;
    try {supervisor=await observeProcess(child.pid);}
    catch(error) {
      await delay(0);
      if(child.exitCode===73)throw Error('An owned local run is already running; lock refused');
      throw Error('Owned launcher exited before process identity could be recorded',{cause:error});
    }
    await writePrivateJson(join(plan.context.launchDirectory,'launcher.json'),{schemaVersion:1,...plan.context,watchdogSeconds,supervisor});
    while(Date.now()<deadline) {
      if(spawnError)throw Error('Owned launcher executable unavailable',{cause:spawnError});
      if(child.exitCode!==null||child.signalCode)throw Error(child.exitCode===73?'An owned local run is already running; lock refused':'Owned runtime exited before startup acknowledgement');
      const ack=await readStartupAcknowledgement(plan.context.launchDirectory);
      if(ack) {
        assertAck(ack,plan.context,supervisor);
        const [currentSupervisor,currentRuntime]=await Promise.all([observeProcess(supervisor.pid),observeProcess(ack.pid)]);
        if(!sameProcess(currentSupervisor,supervisor)||!sameProcess(currentRuntime,ack))throw Error('Owned startup process identity changed');
        return {status:'started',runId:plan.context.runId,sourceHead:plan.context.sourceHead,launchDirectory:plan.context.launchDirectory,
          supervisorPid:supervisor.pid,runtimePid:ack.pid};
      }
      await delay(50);
    }
    throw Error('Owned startup acknowledgement timed out; inspect private status before any new attempt');
  } finally {child.unref();}
}

/** Observation only: never signals processes, rewrites evidence, or starts retries. */
export async function readLocalLaunchStatus(launchDirectory) {
  ownedPrivateDirectory(launchDirectory);
  const record=await privateJson(join(launchDirectory,'launcher.json'));
  if(record.schemaVersion!==1||record.launchDirectory!==launchDirectory||!UUID.test(record.runId)||!SHA.test(record.sourceHead))throw Error('Invalid private launcher record');
  const ack=await readStartupAcknowledgement(launchDirectory);
  let supervisor,runtime;
  try {supervisor=await observeProcess(record.supervisor.pid);}catch(error){if(error.code!=='ENOENT'&&error.code!=='ESRCH')throw error;}
  if(ack) {assertAck(ack,record,record.supervisor);try {runtime=await observeProcess(ack.pid);}catch(error){if(error.code!=='ENOENT'&&error.code!=='ESRCH')throw error;}}
  const supervisorAlive=Boolean(supervisor&&sameProcess(supervisor,record.supervisor)),runtimeAlive=Boolean(runtime&&sameProcess(runtime,ack));
  return {status:supervisorAlive?(runtimeAlive?'running':'starting-or-stopping'):(runtimeAlive?'orphaned-owned-runtime':'stopped'),
    runId:record.runId,sourceHead:record.sourceHead,launchDirectory,supervisorAlive,runtimeAlive,
    qualification:'Inspect the runtime report; process exit alone does not prove qualification.'};
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  try {
    const [action,...args]=process.argv.slice(2);
    if(action==='start'&&args.length===3)console.log(JSON.stringify(await launchLocalRun(await prepareLocalLaunchPlan({repositoryDirectory:resolve(args[0]),evidenceRoot:resolve(args[1]),expectedHead:args[2]}))));
    else if(action==='status'&&args.length===1)console.log(JSON.stringify(await readLocalLaunchStatus(resolve(args[0]))));
    else throw Error('Usage: local-launch.mjs start <checkout> <dedicated-private-ext4-root> <reviewed-head> | status <launch-directory>');
  } catch(error) {console.error(error.message);process.exitCode=1;}
}
