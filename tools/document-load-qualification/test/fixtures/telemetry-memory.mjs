import {BoundedTelemetry} from '../../telemetry.mjs';
import {join} from 'node:path';
import {createReadStream} from 'node:fs';
import {createHash} from 'node:crypto';
const telemetry=await BoundedTelemetry.open(join(process.argv[2],'observations.jsonl'));
for(let i=0;i<288000;i++)await telemetry.recordObservation({observedAt:new Date(i*1000).toISOString(),rssBytes:1000+i,diskFreeBytes:1000000-i,storageDiskFreeBytes:1000000-i,databaseDiskFreeBytes:2000000-i,databaseBytes:100+i,storageBytes:i,resourceObservationMs:i%11,storageScan:{complete:true,elapsedMs:i%7,entries:i+1},diagnostic:`${i}:`+'x'.repeat(256)});
for(let i=0;i<1200000;i++)telemetry.recordTiming({operation:['create','detail','publish'][i%3],elapsedMs:(i*197)%10007/17,status:i%5?200:422,discardedPayload:`${i}:`+'y'.repeat(1024)});
const summary=telemetry.timingSummary(),proof=await telemetry.close(),hash=createHash('sha256');let lines=0;
for await(const chunk of createReadStream(proof.path)){hash.update(chunk);for(const byte of chunk)if(byte===10)lines++;}
console.log(JSON.stringify({count:proof.count,lines,recentCount:telemetry.recentObservations.length,timingCount:Object.values(summary).reduce((sum,value)=>sum+value.count,0),maxRssBytes:process.resourceUsage().maxRSS*1024,digestMatches:hash.digest('hex')===proof.sha256}));
