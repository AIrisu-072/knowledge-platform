import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import * as hosted from '../hosted.mjs';
test('local100k mode is explicit, cannot spoof hosted execution, and excludes every other mode',()=>{
 assert.equal(hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true'},false),true);
 for(const value of ['false','100000',''])assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:value},false));
 for(const key of ['KP_DOCUMENT_LOAD_SMALL','KP_DOCUMENT_LOAD_THOUSAND','KP_DOCUMENT_LOAD_TEN_THOUSAND','KP_DOCUMENT_LOAD_PLAN'])assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true',[key]:key.endsWith('PLAN')?'/plan':'true'},false));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true',GITHUB_ACTIONS:'true'},false));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true'},true));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true',TEST_DATABASE_URL:'postgres://external'},false));
});
test('unstarted100k never copies lower-stage metrics and counts',()=>{
 assert.equal(typeof hosted.selectLocalScaleReport,'function');const tenThousand={status:'SUCCEEDED',stage:10000,metrics:{totalElapsedMs:1}};
 const output=hosted.selectLocalScaleReport({status:'NOT_ADMITTED',tenThousand,failureCode:'chain-deadline-exhausted'},{code:'a'});
 assert.equal(output.stage,100000);assert.equal(output.documentCount,100000);assert.equal(output.status,'NOT_ADMITTED');assert.equal(output.metrics,null);assert.equal(output.counts.confirmedCreatedDocuments,0);assert.equal(output.previousReport,tenThousand);
 const target={status:'FAILED',stage:100000};assert.equal(hosted.selectLocalScaleReport({status:'FAILED',hundredThousand:target},{}),target);
});
test('local runtime validates clean source before ack, owns ext4 DB from startup and exports only after cleanup',async()=>{
 const s=await readFile(new URL('../../document-poc-runtime/run.mjs',import.meta.url),'utf8');
 assert.match(s,/validateLocalLaunchContext/);assert.match(s,/prepareOwnedDatabaseStorage/);assert.match(s,/ownedPostgresArguments/);assert.match(s,/verifyOwnedDatabaseMounts/);
 assert.ok(s.indexOf('recordLocalStartup(')<s.indexOf("report.stage('build'"));assert.match(s,/maxStages:.*4.*3.*2/);assert.match(s,/captureMode: 'bounded-tail'/);
 assert.ok(s.indexOf('await writeLocalScaleReceipt(')>s.indexOf('await report.finish()'));assert.match(s,/localFinalShutdown/);
});
