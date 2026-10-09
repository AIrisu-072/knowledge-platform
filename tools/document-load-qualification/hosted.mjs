import {mkdir,readFile,writeFile}from'node:fs/promises';
import {join,resolve}from'node:path';
import {loadCorpus,sha256}from'./corpus.mjs';
import {observeResources}from'./safety.mjs';
import {runQualification}from'./controller.mjs';
import {summarizeAdmission}from'./admission-summary.mjs';
import {smallPlan,runThousandChain}from'./chain.mjs';
export {smallPlan}from'./chain.mjs';
import {writeSmallReceipt,writeThousandReceipt}from'./receipt-export.mjs';
import {sanitizeWorkerDiagnostic,workerProbeArguments} from './worker-probe.mjs';
import {inspectionDiagnosticSql,publicationPrerequisiteSql,sanitizePublicationPrerequisites} from './diagnostics.mjs';
import {postgresVersionArgs}from'../document-poc-runtime/postgres-readiness.mjs';
export function receiptExportEnabled(env){return env.GITHUB_ACTIONS==='true' && env.KP_DOCUMENT_LOAD_RECEIPT_ALLOWED==='true';}
export function receiptSourceHead(env){const head=env.KP_DOCUMENT_LOAD_SOURCE_HEAD;if(typeof head!=='string'||!/^[a-f0-9]{40}$/.test(head))throw Error('Receipt checkout head unavailable');return head;}
export function normalizeWorkerResult(result,binding){return{...sanitizeWorkerDiagnostic(result),binding};}
export function loadEnabled(env,prebuilt){
 const modes=['KP_DOCUMENT_LOAD_SMALL','KP_DOCUMENT_LOAD_PLAN','KP_DOCUMENT_LOAD_THOUSAND'].filter(key=>env[key]!==undefined);
 if(!modes.length)return false;
 if(modes.length!==1)throw Error('Use one explicit load mode');
 if(env.KP_DOCUMENT_LOAD_PLAN!==undefined && (typeof env.KP_DOCUMENT_LOAD_PLAN!=='string'||!env.KP_DOCUMENT_LOAD_PLAN))throw Error('Invalid load plan path');
 for(const key of ['KP_DOCUMENT_LOAD_SMALL','KP_DOCUMENT_LOAD_THOUSAND'])if(env[key]!==undefined&&env[key]!=='true')throw Error('Load mode must be absent or true');
 if(prebuilt || env.TEST_DATABASE_URL)throw Error('Document load qualification requires built-in-this-run and harness-owned database');
 return true;
}
export function selectChainReport(chain,fingerprint){
 if(chain.thousand)return chain.thousand;
 return {schemaVersion:1,status:chain.status==='SUCCEEDED'?'FAILED':chain.status,stage:1000,documentCount:1000,fingerprint,
 failureCode:chain.failureCode??'fresh-small-not-qualified',previousReport:chain.small,
 counts:{targetDocuments:1000,confirmedCreatedDocuments:0,confirmedPublishedDocuments:0},
 metricQualification:'not-started',metrics:null,productionSloClaim:false,qualityClaim:false};
}
export async function downloadOfficialCorpus(directory,{fetcher=globalThis.fetch}={}){
 await mkdir(directory,{recursive:true,mode:0o700});
 const manifest=JSON.parse(await readFile(new URL('./official-sources.json',import.meta.url),'utf8'));
 for(const item of manifest.assets){
  const response=await fetcher(item.url,{redirect:'error',signal:AbortSignal.timeout(30000)});
  if(!response.ok || !response.body)throw Error('Official PDF download unavailable');
  const chunks=[],reader=response.body.getReader();let size=0;
  try{for(;;){const part=await reader.read();if(part.done)break;size+=part.value.byteLength;if(size>item.bytes)throw Error('Official PDF size drift');chunks.push(part.value);}}catch(error){await reader.cancel().catch(()=>{});throw error;}finally{reader.releaseLock();}
  const bytes=Buffer.concat(chunks);
  if(size!==item.bytes || sha256(bytes)!==item.sha256)throw Error('Official PDF digest or size drift');
  await writeFile(join(directory,item.path),bytes,{mode:0o600,flag:'wx'});item.retrievedAt=new Date().toISOString();
 }
 await writeFile(join(directory,'manifest.json'),JSON.stringify(manifest,null,2)+'\n',{mode:0o600,flag:'wx'});
 return loadCorpus(join(directory,'manifest.json'));
}
function integer(value){if(!/^\d+$/.test(value)||!Number.isSafeInteger(Number(value)))throw Error('Measured numeric resource unavailable');return Number(value);}
/** Called only by the existing owned runtime after its ordinary persistence stage. */
export async function runDocumentLoad({root,directory,runId,sourceHead,artifacts,storage,cid,password,human,agent,getPids,identity,restart,run,worker,pdfium}){
 const target=join(directory,'document-load-qualification');await mkdir(target,{mode:0o700});
 let report,chain;
 const thousandRequested=process.env.KP_DOCUMENT_LOAD_THOUSAND==='true';
 try{
  await run('document-load-build',process.execPath,[join(root,'tools/document-load-qualification/build.mjs')]);
  const corpus=await downloadOfficialCorpus(join(target,'assets'));
  const {DocumentProbe}=await import('./.build/tools/document-load-qualification/src/api.js');
  const fingerprint={corpus:corpus.hash,code:sourceHead,runtime:sha256(JSON.stringify({platform:process.platform,arch:process.arch,artifacts}))};
  let plan=smallPlan(fingerprint),previousReport;
  if(process.env.KP_DOCUMENT_LOAD_PLAN){
   const input=JSON.parse(await readFile(process.env.KP_DOCUMENT_LOAD_PLAN,'utf8'));
   plan={stage:input.stage,documentCount:input.documentCount,safetyFactor:input.safetyFactor,budgets:input.budgets,deadlineAt:input.deadlineAt,fingerprint};
   if(input.previousReport)previousReport=JSON.parse(await readFile(input.previousReport,'utf8'));
  }
  const observe=async()=>{
   const query=postgresVersionArgs(cid);query[query.length-1]='SELECT pg_database_size(current_database())';
   const databaseBytes=integer(await run('document-load-db-size','docker',query,{...process.env,PGPASSWORD:password},10000));
   const databasePid=integer(await run('document-load-db-pid','docker',['inspect','--format','{{.State.Pid}}',cid],process.env,10000));
   const observation=await observeResources({storageRoot:storage,pids:[process.pid,...getPids(),databasePid],databaseBytes});
   const df=await run('document-load-db-free','docker',['exec',cid,'df','-Pk','/var/lib/postgresql'],process.env,10000);
   const fields=df.trim().split('\n').at(-1).trim().split(/\s+/);if(fields.length<6)throw Error('Database filesystem measurement unavailable');
   const databaseFreeBytes=integer(fields[3])*1024;
   if(Number.isSafeInteger(observation.diskFreeBytes)) observation.diskFreeBytes=Math.min(observation.diskFreeBytes,databaseFreeBytes);
   observation.databaseFilesystemVerified=true;
   observation.databaseDiskFreeBytes=databaseFreeBytes;
   observation.limitations=observation.limitations.filter(item=>!item.includes('Database filesystem capacity'));
   observation.limitations.push('Database filesystem free capacity measured inside the owned PostgreSQL container; tmpfs consumes host memory.');
   return observation;
  };
  const diagnosePublication=async fileId=>{
   const query=postgresVersionArgs(cid);query[query.length-1]=inspectionDiagnosticSql(fileId);
   return JSON.parse(await run('document-load-inspection-diagnostic','docker',query,{...process.env,PGPASSWORD:password},10000));
  };
  const diagnoseWorker=async(fileId,binding)=>{
   const asset=[...corpus.assets,...corpus.negativeAssets].find(item=>item.id===binding?.assetId && item.sha256===binding?.sha256);
   if(!asset)throw Error('No bound diagnostic original');
   const query=postgresVersionArgs(cid);query[query.length-1]=publicationPrerequisiteSql(fileId,asset.sha256,asset.bytes.length);
   const prerequisites=sanitizePublicationPrerequisites(JSON.parse(await run('document-load-publication-prerequisites','docker',query,{...process.env,PGPASSWORD:password},10000)));
   if(prerequisites.status!=='observed' || !prerequisites.isWorking || prerequisites.requiresContentClassification || !prerequisites.mediaTypeMatches || !prerequisites.rawHashMatches || !prerequisites.sizeMatches)return{status:'unavailable',binding:prerequisites};
   const executable=join(resolve(root,process.env.CARGO_TARGET_DIR??'target'),'debug','examples','document-load-inspection');
   const result=JSON.parse(await run('document-load-worker-diagnostic',executable,workerProbeArguments({asset,assetDirectory:join(target,'assets'),worker,pdfium}),process.env,15000));
   return normalizeWorkerResult(result,prerequisites);
  };
  const runtime={observe,identity,restart,diagnosePublication,diagnoseWorker,evidenceClass:'owned-real-process'};
  const probeFactory=options=>new DocumentProbe({...options,humanUrl:human,agentUrl:agent});
  if(thousandRequested){
   chain=await runThousandChain({directory:target,runId,fingerprint,corpus,runtime,probeFactory});
   report=selectChainReport(chain,fingerprint);
   await writeFile(join(target,'chain.json'),JSON.stringify(chain)+'\n',{mode:0o600,flag:'wx'});
  }else report=await runQualification({directory:target,runId,plan,previousReport,corpus,runtime,probeFactory});
 }catch{report={schemaVersion:1,status:'FAILED',stage:thousandRequested?1000:'small',documentCount:thousandRequested?1000:2,failureCode:thousandRequested?'official-thousand-prerequisite-failed':'official-small-prerequisite-failed',productionSloClaim:false};await writeFile(join(target,'report.json'),JSON.stringify(report)+'\n',{mode:0o600});}
 let receiptExport;
 if(receiptExportEnabled(process.env) && report.status==='SUCCEEDED' && (report.stage==='small'||thousandRequested)){
  try{receiptExport={status:'EXPORTED',...await (thousandRequested?writeThousandReceipt(root,chain,receiptSourceHead(process.env)):writeSmallReceipt(root,report,receiptSourceHead(process.env)))};}catch{receiptExport={status:'FAILED'};}
 }
 // Only fixed categories, numeric aggregates and the receipt digest are emitted.
 console.log(JSON.stringify({documentLoadQualification:{freshSmall:chain?.small?{status:chain.small.status,counts:chain.small.counts,metrics:chain.small.metrics,failureCode:chain.small.failureCode??null,failureDiagnostic:chain.small.failureDiagnostic??null,negativeFailureCode:chain.small.negativeFailureCode??null,admission:summarizeAdmission(chain.small)}:null,status:report.status,admission:summarizeAdmission(report),receiptExport:receiptExport??null,stage:report.stage??'small',documentCount:report.documentCount??2,failureCode:report.failureCode??null,failureDiagnostic:report.failureDiagnostic??null,inspectionDiagnostic:report.inspectionDiagnostic??null,workerDiagnostic:report.workerDiagnostic??null,publicationPrerequisites:report.publicationPrerequisites??null,counts:report.counts??null,negativeFailureCode:report.negativeFailureCode??null,negativeCorpus:report.negativeCorpus?{status:report.negativeCorpus.status,counts:report.negativeCorpus.counts,contentQualityClaim:false}:null,metricQualification:report.metricQualification??'measurement-unavailable',metrics:report.metrics??null,timings:report.timings??{},productionSloClaim:false}}));
 if(report.status!=='SUCCEEDED'||receiptExport?.status==='FAILED')throw Error('Document load qualification did not succeed; preserve its separate report');
 return {status:report.status,stage:report.stage,documentCount:report.documentCount,fingerprint:report.fingerprint,metrics:report.metrics};
}
