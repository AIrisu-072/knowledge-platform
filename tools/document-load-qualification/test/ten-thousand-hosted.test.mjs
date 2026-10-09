import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import * as hosted from '../hosted.mjs';

test('ten-thousand mode is explicit and cannot combine modes or use external runtime',()=>{
 assert.equal(hosted.loadEnabled({KP_DOCUMENT_LOAD_TEN_THOUSAND:'true'},false),true);
 for(const value of ['false','10000',''])assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_TEN_THOUSAND:value},false));
 for(const key of ['KP_DOCUMENT_LOAD_SMALL','KP_DOCUMENT_LOAD_THOUSAND','KP_DOCUMENT_LOAD_PLAN'])assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_TEN_THOUSAND:'true',[key]:key.endsWith('PLAN')?'/plan':'true'},false));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_TEN_THOUSAND:'true'},true));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_TEN_THOUSAND:'true',TEST_DATABASE_URL:'postgres://external'},false));
});
test('unstarted ten-thousand stage cannot borrow successful lower-stage counts or measurements',()=>{
 const small={stage:'small',status:'SUCCEEDED'},thousand={stage:1000,status:'SUCCEEDED',metrics:{totalElapsedMs:942290},counts:{confirmedCreatedDocuments:1000}};
 assert.equal(typeof hosted.selectTenThousandChainReport,'function');
 const report=hosted.selectTenThousandChainReport({status:'NOT_ADMITTED',small,thousand,failureCode:'chain-deadline-exhausted'},{code:'a'});
 assert.equal(report.stage,10000);assert.equal(report.documentCount,10000);assert.equal(report.status,'NOT_ADMITTED');assert.equal(report.metrics,null);assert.equal(report.counts.confirmedCreatedDocuments,0);assert.equal(report.previousReport,thousand);
 const target={stage:10000,status:'ABORTED',metrics:{totalElapsedMs:10}};
 assert.equal(hosted.selectTenThousandChainReport({status:'ABORTED',small,thousand,tenThousand:target},{}),target);
});
test('ten-thousand workflow uses one public main-only manual target with shared concurrency and bounded postwork',async()=>{
 const file=new URL('../../../.github/workflows/document-load-ten-thousand.yml',import.meta.url);
 const y=await readFile(file,'utf8').catch(()=>null);assert.ok(y,'dedicated ten-thousand workflow exists');
 const events=y.split('permissions:')[0];assert.match(events,/workflow_dispatch:/);assert.match(events,/options: \['10000'\]/);assert.doesNotMatch(events,/(?:push|pull_request|schedule)\s*:/);
 for(const pattern of [/timeout-minutes: 360/,/runs-on: ubuntu-24.04/,/group: document-load-thousand/,/cancel-in-progress: false/,/github\.event\.repository\.private == false/,/github\.ref == 'refs\/heads\/main'/,/inputs\.stage == '10000'/,/contents: read/,/export KP_DOCUMENT_LOAD_TEN_THOUSAND=true/,/unset KP_DOCUMENT_LOAD_SMALL KP_DOCUMENT_LOAD_PLAN KP_DOCUMENT_LOAD_THOUSAND/])assert.match(y,pattern);
 assert.doesNotMatch(y,/write-all|contents: write|secrets\.|100000|--prebuilt|TEST_DATABASE_URL/);
 const summary=y.split('- name: Emit bounded owned runtime result')[1].split('- name:')[0];assert.match(summary,/timeout-minutes: 1/);
 const upload=y.split('- name: Upload only the successful bounded chain receipt')[1];assert.match(upload,/timeout-minutes: 2/);assert.match(upload,/if: \$\{\{ success\(\) \}\}/);assert.match(upload,/retention-days: 1/);assert.match(upload,/path: tools\/document-poc-runtime\/\.state\/document-load-export\/qualification\.json/);assert.doesNotMatch(upload,/path:.*\*/);
});
test('hosted runtime captures the provider clock before build and uses three owned restarts only for ten-thousand',async()=>{
 const s=await readFile(new URL('../../document-poc-runtime/run.mjs',import.meta.url),'utf8');
 assert.match(s,/fetchCurrentRunBudget/);assert.ok(s.indexOf('await fetchCurrentRunBudget')<s.indexOf("report.stage('build'"));assert.match(s,/maxStages:.*3.*2/);assert.match(s,/loadBudget/);
});
