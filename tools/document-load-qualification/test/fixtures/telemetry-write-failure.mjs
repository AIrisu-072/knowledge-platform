import {BoundedTelemetry} from '../../telemetry.mjs';
import {runQualification} from '../../controller.mjs';
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';
process.on('SIGXFSZ',()=>{});
const directory=process.argv[2],telemetry=await BoundedTelemetry.open(join(directory,'partial.jsonl'));
await telemetry.recordObservation({sequence:1});
let appendFailed=false,closeFailed=false;
try{await telemetry.recordObservation({sequence:2,payload:'x'.repeat(65536)});}catch{appendFailed=true;}
try{await telemetry.close();}catch{closeFailed=true;}
// The failure report fits beneath the same 16 KiB per-file limit; the oversized
// observation must fail through the real filesystem rather than a mocked writer.
const report=await runQualification({directory:join(directory,'controller'),runId:'write-failure',plan:{stage:'small',documentCount:2,fingerprint:{code:'c',corpus:'c',runtime:'r'},safetyFactor:2,budgets:{diskReserveBytes:0,minAvailableMemoryBytes:0,maxRssBytes:100,maxWallTimeMs:30000},deadlineAt:new Date(Date.now()+30000).toISOString()},corpus:{hash:'c',assets:[]},runtime:{telemetryMode:'bounded-disk',observe:async()=>({payload:'x'.repeat(65536)})},probeFactory:()=>{throw Error('Must not mutate after observation write failure');}});
const durable=JSON.parse(await readFile(join(directory,'controller','report.json'),'utf8')).status;
console.log(JSON.stringify({appendFailed,closeFailed,controllerStatus:report.status,metricQualification:report.metricQualification,durableStatus:durable}));
