import {open} from 'node:fs/promises';
import {createHash} from 'node:crypto';

const resourceKeys=['rssBytes','diskFreeBytes','storageDiskFreeBytes','databaseDiskFreeBytes','databaseBytes','storageBytes'];
const finite=value=>typeof value==='number'&&Number.isFinite(value)&&value>=0;
const failure=()=>Object.assign(Error('Private qualification telemetry failed'),{code:'telemetry-write-failed'});

/** Private evidence only. Callers must await each append before taking another sample. */
export class BoundedTelemetry {
 #file;#path;#hash=createHash('sha256');#count=0;#bytes=0;#recent=[];
 #pending=Promise.resolve();#closePromise;#closed=false;#failure;
 #initial;#peak;#resourcesValid=true;#scansValid=true;
 #scans={status:'measured',observationCount:0,maxResourceObservationMs:0,maxStorageScanMs:0,maxStorageEntries:0};
 #timings=new Map();
 constructor(file,path){this.#file=file;this.#path=path;}
 static async open(path){
  // Never append onto another run, truncate existing evidence, or follow a symlink.
  try{return new BoundedTelemetry(await open(path,'ax',0o600),path);}catch{throw failure();}
 }
 get recentObservations(){return this.#recent.slice();}
 recordObservation(value){
  if(this.#closed)return Promise.reject(Error('Private qualification telemetry is closed'));
  let bytes,snapshot;
  try{
   if(!value||typeof value!=='object'||Array.isArray(value))throw failure();
   const line=JSON.stringify(value);snapshot=JSON.parse(line);bytes=Buffer.from(line+'\n');
  }catch{this.#failure??=failure();return Promise.reject(this.#failure);}
  const append=this.#pending.then(async()=>{
   if(this.#failure)throw this.#failure;
   try{await this.#file.writeFile(bytes);}catch{this.#failure??=failure();throw this.#failure;}
   this.#hash.update(bytes);this.#count++;this.#bytes+=bytes.length;
   this.#recent.push(snapshot);if(this.#recent.length>256)this.#recent.shift();
   this.#recordResources(snapshot);this.#recordScans(snapshot);
  });
  // Keep failures sticky while also ensuring no unhandled queue rejection.
  this.#pending=append.catch(error=>{this.#failure??=error;});
  return append;
 }
 #recordResources(value){
  if(!resourceKeys.every(key=>Number.isSafeInteger(value[key])&&value[key]>=0)){this.#resourcesValid=false;return;}
  if(!this.#initial){this.#initial=Object.fromEntries(resourceKeys.map(key=>[key,value[key]]));this.#peak={rss:0,storageFree:value.storageDiskFreeBytes,databaseFree:value.databaseDiskFreeBytes,db:value.databaseBytes,storage:value.storageBytes};}
  const peak=this.#peak;
  peak.rss=Math.max(peak.rss,value.rssBytes);peak.storageFree=Math.min(peak.storageFree,value.storageDiskFreeBytes);peak.databaseFree=Math.min(peak.databaseFree,value.databaseDiskFreeBytes);peak.db=Math.max(peak.db,value.databaseBytes);peak.storage=Math.max(peak.storage,value.storageBytes);
 }
 #recordScans(value){
  if(!finite(value.resourceObservationMs)||value.storageScan?.complete!==true||!finite(value.storageScan.elapsedMs)||!Number.isSafeInteger(value.storageScan.entries)||value.storageScan.entries<=0){this.#scansValid=false;return;}
  const result=this.#scans;result.observationCount++;result.maxResourceObservationMs=Math.max(result.maxResourceObservationMs,value.resourceObservationMs);result.maxStorageScanMs=Math.max(result.maxStorageScanMs,value.storageScan.elapsedMs);result.maxStorageEntries=Math.max(result.maxStorageEntries,value.storageScan.entries);
 }
 summary(totalElapsedMs){
  if(!this.#count||!this.#resourcesValid)return null;
  const initial=this.#initial,peak=this.#peak;
  const storageGrowthBytes=peak.storage-initial.storageBytes,databaseGrowthBytes=peak.db-initial.databaseBytes;
  const storageAllocatedGrowthBytes=Math.max(0,initial.storageDiskFreeBytes-peak.storageFree,storageGrowthBytes);
  const databaseAllocatedGrowthBytes=Math.max(0,initial.databaseDiskFreeBytes-peak.databaseFree,databaseGrowthBytes);
  return {totalElapsedMs,peakRssBytes:peak.rss,diskGrowthBytes:storageAllocatedGrowthBytes+databaseAllocatedGrowthBytes,storageAllocatedGrowthBytes,databaseAllocatedGrowthBytes,databaseGrowthBytes,storageGrowthBytes,rssMeasurement:'sampled process-tree RSS; peaks between samples may be missed',diskMeasurement:'sum of peak per-filesystem free-space decreases (at least logical growth); includes concurrent host allocation and conservatively double-counts a shared filesystem'};
 }
 scanSummary(){return this.#count&&this.#scansValid?{...this.#scans}:{status:'unavailable'};}
 recordTiming(value){
  if(this.#closed)throw Error('Private qualification telemetry is closed');
  if(this.#failure)throw this.#failure;
  if(typeof value?.operation!=='string'||!finite(value.elapsedMs)){this.#failure=failure();throw this.#failure;}
  let group=this.#timings.get(value.operation);
  if(!group){group={samples:new Float64Array(1024),count:0,statuses:new Map()};this.#timings.set(value.operation,group);}
  if(group.count===group.samples.length){const expanded=new Float64Array(group.samples.length*2);expanded.set(group.samples);group.samples=expanded;}
  group.samples[group.count++]=value.elapsedMs;const status=String(value.status);group.statuses.set(status,(group.statuses.get(status)??0)+1);
 }
 timingSummary(){
  if(this.#failure)throw this.#failure;
  return Object.fromEntries([...this.#timings].map(([operation,group])=>{
   // Native numeric sorting avoids comparator boxing for millions of samples.
   // Restore signed-zero input order: the old stable (a-b) sort considers both
   // zeros equal, whereas native typed-array sorting puts negative zero first.
   const count=group.count,sorted=group.samples.subarray(0,count),zeroSigns=new Uint8Array(count);let zeroCount=0;
   for(const value of sorted)if(value===0)zeroSigns[zeroCount++]=Object.is(value,-0)?1:0;
   sorted.sort();for(let i=0;i<zeroCount;i++)sorted[i]=zeroSigns[i]?-0:0;
   const percentile=p=>count?sorted[Math.ceil(p*count)-1]:null;
   return[operation,{count,minMs:count?sorted[0]:null,p50Ms:percentile(0.5),p95Ms:percentile(0.95),p99Ms:percentile(0.99),maxMs:count?sorted[count-1]:null,meanMs:count?sorted.reduce((sum,value)=>sum+value/count,0):null,statuses:Object.fromEntries(group.statuses)}];
  }));
 }
 close(){
  if(this.#closePromise)return this.#closePromise;
  this.#closed=true;
  this.#closePromise=(async()=>{
   await this.#pending;
   try{if(!this.#failure)await this.#file.sync();}catch{this.#failure??=failure();}
   finally{try{await this.#file.close();}catch{this.#failure??=failure();}}
   if(this.#failure)throw this.#failure;
   return{path:this.#path,count:this.#count,bytes:this.#bytes,sha256:this.#hash.digest('hex')};
  })();
  return this.#closePromise;
 }
}
