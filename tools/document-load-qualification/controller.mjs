import assert from 'node:assert/strict';
import {mkdir,open,rename}from'node:fs/promises';
import {join}from'node:path';
import {admitStage,summarizeTimings,validatePlan}from'./safety.mjs';
import {Journal}from'./journal.mjs';
import {exerciseStage,verifyRetained}from'./workflow.mjs';
import {exerciseNegative,verifyNegativeRetained,sanitizeNegativeFailureCode}from'./negative.mjs';
import {sanitizeWorkerDiagnostic} from './worker-probe.mjs';
import {sanitizeFailureDiagnostic,sanitizeInspectionDiagnostic,sanitizePublicationPrerequisites} from './diagnostics.mjs';
export async function runQualification({directory,runId,plan,previousReport,corpus,runtime,probeFactory,execute=exerciseStage,verify=verifyRetained,executeNegative=exerciseNegative,verifyNegative=verifyNegativeRetained}){
 await mkdir(directory,{recursive:true,mode:0o700});
 const started=performance.now(),timings=[],observations=[];
 const report={schemaVersion:1,runId,evidenceClass:runtime.evidenceClass==='owned-real-process'?'owned-real-process':'test-double',status:'NOT_RUN',stage:plan?.stage,documentCount:plan?.documentCount,fingerprint:plan?.fingerprint,plan,previousReport,productionSloClaim:false,qualityClaim:false,startedAt:new Date().toISOString(),sampleIntervalMs:1000};
 async function save(){const file=await open(join(directory,'report.tmp'),'w',0o600);try{await file.writeFile(JSON.stringify(report,null,2)+'\n');await file.sync();}finally{await file.close();}await rename(join(directory,'report.tmp'),join(directory,'report.json'));}
 let journal,timer,monitor,busy,stopError;
 const abort=new AbortController();
 const fail=code=>{const error=Error(code);error.code=code;return error;};
 let lastObservationAt=-Infinity;
 async function observe(force=false){if(busy)return busy;if(!force && performance.now()-lastObservationAt<1000)return observations.at(-1);busy=(async()=>{const value=await runtime.observe();observations.push(value);lastObservationAt=performance.now();return value;})();try{return await busy;}finally{busy=undefined;}}
 async function checkpoint(force=false){
  if(stopError)throw stopError;
  if(performance.now()-started>plan.budgets.maxWallTimeMs || Date.now()>Date.parse(plan.deadlineAt))throw fail('wall-budget-exhausted');
  const observation=await observe(force);
  if(stopError)throw stopError;
  if(performance.now()-started>plan.budgets.maxWallTimeMs || Date.now()>Date.parse(plan.deadlineAt))throw fail('wall-budget-exhausted');
  // Recheck current reserves, not a second full-stage capacity projection.
  const guard=admitStage({...plan,stage:'small',documentCount:2},undefined,observation);
  if(guard.status!=='ADMITTED')throw fail('resource-budget-exhausted');
 }
 try{
  const valid=validatePlan(plan);
  if(!valid.valid || corpus.hash!==plan.fingerprint.corpus){report.status='NOT_ADMITTED';report.failureCode='invalid-plan-or-corpus';await save();return report;}
  const initial=await observe();report.admission=admitStage(plan,previousReport,initial);
  if(report.admission.status!=='ADMITTED'){report.status='NOT_ADMITTED';await save();return report;}
  report.status='RUNNING';await save();
  const budgetMs=Math.min(plan.budgets.maxWallTimeMs,Date.parse(plan.deadlineAt)-Date.now());
  timer=setTimeout(()=>{stopError=fail('wall-budget-exhausted');abort.abort(stopError);},budgetMs);
  monitor=setInterval(()=>{checkpoint().catch(error=>{stopError=error;abort.abort(error);});},1000);
  journal=await Journal.open(join(directory,'operations.jsonl'),{runId,fingerprint:plan.fingerprint,stage:plan.stage,documentCount:plan.documentCount});
  const probe=await probeFactory({signal:abort.signal,onTiming:value=>timings.push(value)});
  const identityBefore=await runtime.identity();
  if(corpus.negativeAssets?.length){
   report.negativeCorpus=await executeNegative({probe,journal,assets:corpus.negativeAssets,checkpoint,runId,stageLabel:plan.stage,diagnosePublication:runtime.diagnosePublication,diagnoseWorker:runtime.diagnoseWorker});
   await save();
  }
  report.evidence=await execute({probe,journal,count:plan.documentCount,assets:corpus.assets,checkpoint,runId,stageLabel:plan.stage});
  report.status='AWAITING_RESTART';await save();
  // Avoid observing expected process absence while the owned restart is in progress.
  clearInterval(monitor);monitor=undefined;await busy;
  await runtime.restart();lastObservationAt=-Infinity;
  const identityAfter=await runtime.identity();
  const {humanPid:oldHuman,agentPid:oldAgent,...beforeDataset}=identityBefore;
  const {humanPid:newHuman,agentPid:newAgent,...afterDataset}=identityAfter;
  try{assert.deepEqual(beforeDataset,afterDataset);assert.ok(Number.isInteger(oldHuman)&&Number.isInteger(oldAgent)&&Number.isInteger(newHuman)&&Number.isInteger(newAgent));assert.notEqual(oldHuman,newHuman);assert.notEqual(oldAgent,newAgent);}catch{throw fail('restart-proof-failed');}
  report.restart={identityRetained:true,processesReplaced:true,before:beforeDataset,after:afterDataset,processes:{before:[oldHuman,oldAgent],after:[newHuman,newAgent]}};
  monitor=setInterval(()=>{checkpoint().catch(error=>{stopError=error;abort.abort(error);});},1000);
  await verify({probe,evidence:report.evidence,checkpoint});
  if(report.negativeCorpus){await verifyNegative({probe,evidence:report.negativeCorpus,checkpoint,diagnosePublication:runtime.diagnosePublication});report.negativeCorpus.status='SUCCEEDED';}
  await checkpoint(true);
  report.status='SUCCEEDED';
 }catch(error){
  error=stopError??error;
  report.status=error?.code==='wall-budget-exhausted'||error?.code==='resource-budget-exhausted'?'ABORTED':'FAILED';
  report.failureCode=['wall-budget-exhausted','resource-budget-exhausted','restart-proof-failed'].includes(error?.code)?error.code:'qualification-assertion-or-prerequisite-failed';
  report.failureDiagnostic=sanitizeFailureDiagnostic(error?.diagnostic);
  const negativeFailureCode=sanitizeNegativeFailureCode(error);
  if(negativeFailureCode)report.negativeFailureCode=negativeFailureCode;
  if(report.failureDiagnostic?.operation==='publish' && journal && runtime.diagnosePublication){
   // Only an unacknowledged publication from this run's durable operation map.
   const pending=[...journal.states].find(([key,state])=>/^publish:\d+$/.test(key) && !Object.hasOwn(state,'result'));
   const fileId=pending?journal.get(pending[0].replace('publish:','create:'))?.result?.fileId
    :journal.get('publish-next')&&!Object.hasOwn(journal.get('publish-next'),'result')?journal.get('next-version')?.request?.fileId:undefined;
   if(fileId){try{report.inspectionDiagnostic=sanitizeInspectionDiagnostic(await runtime.diagnosePublication(fileId));}catch{report.inspectionDiagnostic={status:'unavailable'};}}
   if(fileId && report.inspectionDiagnostic?.status==='not-found' && report.failureDiagnostic.problemCode==='BUSINESS_RULE_REJECTED' && runtime.diagnoseWorker){
    const declaration=pending?journal.get(pending[0].replace('publish:','create:'))?.request:journal.get('next-version')?.request;
    try{const result=await runtime.diagnoseWorker(fileId,{assetId:declaration?.assetId,sha256:declaration?.sha256});report.workerDiagnostic=sanitizeWorkerDiagnostic(result);report.publicationPrerequisites=sanitizePublicationPrerequisites(result?.binding);}catch{report.workerDiagnostic={status:'unavailable',qualification:false};}
   }

  }
 }
 finally{
  clearInterval(monitor);clearTimeout(timer);await busy?.catch(()=>{});
  const entries=journal?[...journal.states]:[];
  report.counts={targetDocuments:plan?.documentCount??0,confirmedCreatedDocuments:entries.filter(([key,state])=>/^create:\d+$/.test(key)&&Object.hasOwn(state,'result')).length,confirmedPublishedDocuments:entries.filter(([key,state])=>/^publish:\d+$/.test(key)&&Object.hasOwn(state,'result')).length};
  if(corpus.negativeAssets?.length){
   const previous=report.negativeCorpus;
   report.negativeCorpus={...previous,status:report.status==='SUCCEEDED'?'SUCCEEDED':report.status,contentQualityClaim:false,
    counts:{targetDocuments:corpus.negativeAssets.length,
     confirmedCreatedDocuments:entries.filter(([key,state])=>/^negative-create:\d+$/.test(key)&&Object.hasOwn(state,'result')).length,
     confirmedHttp422Responses:entries.filter(([key,state])=>/^negative-publish:\d+$/.test(key)&&state.result?.outcome==='rejected-unsupported'&&state.result?.failureDiagnostic?.httpStatus===422).length,
     confirmedRejectedDocuments:previous?.documents?.length??0,confirmedPublishedDocuments:entries.filter(([key,state])=>/^negative-publish:\d+$/.test(key)&&state.result?.outcome==='unexpected-published').length}};
  }
  report.metricQualification=report.status==='SUCCEEDED'?'complete-stage':'partial-failed-stage';
  if(journal && report.status!=='SUCCEEDED')await observe(true).catch(()=>{report.metricQualification='measurement-unavailable';});
  await journal?.close();
  report.finishedAt=new Date().toISOString();
  report.observations=observations;
  report.metrics=summarizeResources(observations,performance.now()-started);
  report.timings=Object.fromEntries([...new Set(timings.map(t=>t.operation))].map(operation=>{const samples=timings.filter(t=>t.operation===operation);return[operation,{...summarizeTimings(samples.map(t=>t.elapsedMs)),statuses:Object.fromEntries([...new Set(samples.map(t=>String(t.status)))].map(status=>[status,samples.filter(t=>String(t.status)===status).length]))}];}));
  report.throughputDocumentsPerSecond=report.status==='SUCCEEDED'?plan.documentCount/(report.metrics.totalElapsedMs/1000):null;
  await save();
 }
 return report;
}

export function summarizeResources(observations,totalElapsedMs){
 if(!observations.length || !observations.every(value=>['rssBytes','diskFreeBytes','storageDiskFreeBytes','databaseDiskFreeBytes','databaseBytes','storageBytes'].every(key=>Number.isSafeInteger(value[key])&&value[key]>=0)))return null;
 const initial=observations[0];
 const peak=observations.reduce((out,value)=>({rss:Math.max(out.rss,value.rssBytes),storageFree:Math.min(out.storageFree,value.storageDiskFreeBytes),databaseFree:Math.min(out.databaseFree,value.databaseDiskFreeBytes),db:Math.max(out.db,value.databaseBytes),storage:Math.max(out.storage,value.storageBytes)}),{rss:0,storageFree:initial.storageDiskFreeBytes,databaseFree:initial.databaseDiskFreeBytes,db:initial.databaseBytes,storage:initial.storageBytes});
 const storageGrowthBytes=peak.storage-initial.storageBytes,databaseGrowthBytes=peak.db-initial.databaseBytes;
 const storageAllocatedGrowthBytes=Math.max(0,initial.storageDiskFreeBytes-peak.storageFree,storageGrowthBytes);
 const databaseAllocatedGrowthBytes=Math.max(0,initial.databaseDiskFreeBytes-peak.databaseFree,databaseGrowthBytes);
 return {totalElapsedMs,peakRssBytes:peak.rss,diskGrowthBytes:storageAllocatedGrowthBytes+databaseAllocatedGrowthBytes,storageAllocatedGrowthBytes,databaseAllocatedGrowthBytes,databaseGrowthBytes,storageGrowthBytes,rssMeasurement:'sampled process-tree RSS; peaks between samples may be missed',diskMeasurement:'sum of peak per-filesystem free-space decreases (at least logical growth); includes concurrent host allocation and conservatively double-counts a shared filesystem'};
}
