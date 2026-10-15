/** Diagnostic measurements only; never substitute for fresh admission observations. */
export function summarizeScans(observations){
 const finite=value=>typeof value==='number'&&Number.isFinite(value)&&value>=0;
 if(!Array.isArray(observations)||observations.length===0||!observations.every(value=>
  finite(value?.resourceObservationMs)&&value?.storageScan?.complete===true
  &&finite(value.storageScan.elapsedMs)&&Number.isSafeInteger(value.storageScan.entries)&&value.storageScan.entries>0))return{status:'unavailable'};
 return observations.reduce((result,value)=>({status:'measured',observationCount:result.observationCount+1,
  maxResourceObservationMs:Math.max(result.maxResourceObservationMs,value.resourceObservationMs),
  maxStorageScanMs:Math.max(result.maxStorageScanMs,value.storageScan.elapsedMs),
  maxStorageEntries:Math.max(result.maxStorageEntries,value.storageScan.entries)}),
 {status:'measured',observationCount:0,maxResourceObservationMs:0,maxStorageScanMs:0,maxStorageEntries:0});
}
