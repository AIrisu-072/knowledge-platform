import test from 'node:test';import assert from 'node:assert/strict';import{readFile,mkdtemp,rm}from'node:fs/promises';import{join}from'node:path';import{tmpdir}from'node:os';
const hosted=await import('../hosted.mjs').catch(()=>({}));
test('small hosted plan is bounded and never defaults to a large workload',()=>{assert.equal(typeof hosted.smallPlan,'function');const p=hosted.smallPlan({corpus:'a',code:'b',runtime:'c'});assert.equal(p.stage,'small');assert.equal(p.documentCount,2);assert.ok(p.budgets.maxWallTimeMs<=300000);assert.ok(p.budgets.diskReserveBytes>0);});
test('optional existing-runner hook is disabled unless explicitly true and refuses prebuilt/external DB',()=>{assert.equal(typeof hosted.loadEnabled,'function');assert.equal(hosted.loadEnabled({},false),false);assert.equal(hosted.loadEnabled({KP_DOCUMENT_LOAD_SMALL:'true'},false),true);assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_SMALL:'false'},false));assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_SMALL:'true'},true));assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_SMALL:'true',TEST_DATABASE_URL:'postgres://localhost/a'},false));});
test('download fails closed on digest drift without retaining a corrupt PDF',async t=>{assert.equal(typeof hosted.downloadOfficialCorpus,'function');const dir=await mkdtemp(join(tmpdir(),'official-download-'));t.after(()=>rm(dir,{recursive:true,force:true}));await assert.rejects(hosted.downloadOfficialCorpus(dir,{fetcher:async()=>new Response('%PDF-corrupt',{status:200})}),/digest|size/);});
test('runner places optional small test after ordinary persistence and before final shutdown',async()=>{const s=await readFile(new URL('../../document-poc-runtime/run.mjs',import.meta.url),'utf8');assert.ok(s.includes('runDocumentLoad'));assert.ok(s.indexOf("report.stage('browser-persistence'")<s.indexOf('await runDocumentLoad'));assert.ok(s.indexOf('await runDocumentLoad')<s.indexOf("report.stage('final-shutdown'"));});
test('larger stages require an explicit local plan and cannot combine modes',()=>{assert.equal(hosted.loadEnabled({KP_DOCUMENT_LOAD_PLAN:'/private/plan.json'},false),true);assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_SMALL:'true',KP_DOCUMENT_LOAD_PLAN:'/private/plan.json'},false));assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_PLAN:''},false));});
test('CI enables only the two-PDF small mode for the dedicated pull-request label',async()=>{const s=await readFile(new URL('../../../.github/workflows/ci.yml',import.meta.url),'utf8');assert.match(s,/contains\(github\.event\.pull_request\.labels\.\*\.name, 'document-load-small'\)/);assert.match(s,/unset KP_DOCUMENT_LOAD_SMALL KP_DOCUMENT_LOAD_PLAN/);assert.match(s,/export KP_DOCUMENT_LOAD_SMALL=true/);assert.doesNotMatch(s,/export KP_DOCUMENT_LOAD_PLAN=/);});
test('actual fixed-code driver output is normalized before negative verification',()=>{assert.equal(typeof hosted.normalizeWorkerResult,'function');assert.deepEqual(hosted.normalizeWorkerResult({status:'worker-failure',failureCode:'unsupported_semantic_construct',message:'PRIVATE'}, {status:'observed'}),{status:'worker-failure',failureCode:'unsupported_semantic_construct',qualification:false,binding:{status:'observed'}});});
test('official manifest retains unsupported original and two distinct positive notices',async()=>{
 const manifest=JSON.parse(await readFile(new URL('../official-sources.json',import.meta.url),'utf8'));
 const positives=manifest.assets.filter(x=>x.expectedOutcome==='publish'),negatives=manifest.assets.filter(x=>x.expectedOutcome==='reject-unsupported');
 assert.equal(positives.length,2);assert.equal(new Set(positives.map(x=>x.sha256)).size,2);assert.equal(negatives.length,1);assert.equal(negatives[0].id,'mhlw-001472933');assert.equal(negatives[0].sha256,'e5a123087d108d066c775417049935ceb6ff9b807461e5439c36dda0987a3706');
 assert.equal(manifest.assets.reduce((n,x)=>n+x.bytes,0),404481);
});
test('CI exports only the fixed successful small receipt with the existing pinned artifact action',async()=>{
 const yaml=await readFile(new URL('../../../.github/workflows/ci.yml',import.meta.url),'utf8');
 const step=yaml.split('- name: Upload bounded Document small qualification receipt')[1]?.split('- name:')[0];
 assert.ok(step);assert.match(step,/success\(\)/);assert.match(step,/document-load-small/);assert.match(step,/actions\/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/);assert.match(step,/retention-days: 1/);assert.match(step,/overwrite: false/);assert.match(step,/if-no-files-found: error/);assert.match(step,/path: tools\/document-poc-runtime\/\.state\/document-load-export\/qualification\.json/);assert.doesNotMatch(step,/path:.*\*|report\.json/);
 const hostedSource=await readFile(new URL('../hosted.mjs',import.meta.url),'utf8');assert.match(hostedSource,/GITHUB_ACTIONS==='true'/);assert.match(hostedSource,/report\.status==='SUCCEEDED' && \(report\.stage==='small'\|\|thousandRequested\|\|tenThousandRequested\)/);assert.match(hostedSource,/writeSmallReceipt\(root,report,receiptSourceHead\(process\.env\)\)/);
});
test('receipt uses the explicit checked-out PR head even when GitHub event SHA is a merge commit',()=>{
 assert.equal(typeof hosted.receiptSourceHead,'function');const head='a'.repeat(40),merge='b'.repeat(40);
 assert.equal(hosted.receiptSourceHead({KP_DOCUMENT_LOAD_SOURCE_HEAD:head,GITHUB_SHA:merge}),head);
 assert.throws(()=>hosted.receiptSourceHead({GITHUB_SHA:merge}));assert.throws(()=>hosted.receiptSourceHead({KP_DOCUMENT_LOAD_SOURCE_HEAD:'PRIVATE_SENTINEL'}));
});
test('receipt export is opt-in only for the workflow-confirmed same-repository CI event',async()=>{
 assert.equal(typeof hosted.receiptExportEnabled,'function');assert.equal(hosted.receiptExportEnabled({GITHUB_ACTIONS:'true',KP_DOCUMENT_LOAD_RECEIPT_ALLOWED:'true'}),true);
 for(const env of [{GITHUB_ACTIONS:'true',KP_DOCUMENT_LOAD_RECEIPT_ALLOWED:'false'},{GITHUB_ACTIONS:'true'},{KP_DOCUMENT_LOAD_RECEIPT_ALLOWED:'true'},{}])assert.equal(hosted.receiptExportEnabled(env),false);
 const yaml=await readFile(new URL('../../../.github/workflows/ci.yml',import.meta.url),'utf8');const step=yaml.split('- name: Upload bounded Document small qualification receipt')[1]?.split('- name:')[0];assert.match(step,/github\.event\.pull_request\.head\.repo != null/);assert.match(step,/github\.event\.pull_request\.head\.repo\.full_name == github\.repository/);assert.match(yaml,/KP_DOCUMENT_LOAD_RECEIPT_ALLOWED:.*head\.repo\.full_name == github\.repository/);
});
test('1000 mode is explicit and mutually exclusive; higher volumes are never implicit',()=>{
 assert.equal(hosted.loadEnabled({KP_DOCUMENT_LOAD_THOUSAND:'true'},false),true);
 for(const env of [{KP_DOCUMENT_LOAD_THOUSAND:'false'},{KP_DOCUMENT_LOAD_THOUSAND:'10000'},{KP_DOCUMENT_LOAD_THOUSAND:'true',KP_DOCUMENT_LOAD_SMALL:'true'},{KP_DOCUMENT_LOAD_THOUSAND:'true',KP_DOCUMENT_LOAD_PLAN:'/private/plan'}])assert.throws(()=>hosted.loadEnabled(env,false));
 assert.throws(()=>hosted.loadEnabled({KP_DOCUMENT_LOAD_THOUSAND:'true'},true));
});
test('a blocked chain does not relabel successful small measurements as completed1000 results',()=>{
 assert.equal(typeof hosted.selectChainReport,'function');const small={status:'SUCCEEDED',metrics:{totalElapsedMs:100},counts:{confirmedCreatedDocuments:2}};
 const report=hosted.selectChainReport({status:'NOT_ADMITTED',failureCode:'interstage-runtime-identity-mismatch',small},{code:'x'});
 assert.equal(report.status,'NOT_ADMITTED');assert.equal(report.stage,1000);assert.equal(report.documentCount,1000);assert.equal(report.metrics,null);assert.equal(report.counts.confirmedCreatedDocuments,0);assert.equal(report.previousReport,small);
 const thousand={status:'FAILED',stage:1000,metrics:{totalElapsedMs:20}};assert.equal(hosted.selectChainReport({status:'FAILED',small,thousand},{}),thousand);
});
