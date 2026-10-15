import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const module=await import('../worker-probe.mjs').catch(()=>({}));
test('worker diagnostic projection rejects raw text and preserves only closed sandbox outcomes',()=>{
 assert.equal(typeof module.sanitizeWorkerDiagnostic,'function');
 assert.deepEqual(module.sanitizeWorkerDiagnostic({status:'worker-failure',failureCode:'unsupported_semantic_construct',reason:'PRIVATE_SENTINEL'}),{status:'worker-failure',failureCode:'unsupported_semantic_construct',qualification:false});
 assert.deepEqual(module.sanitizeWorkerDiagnostic({status:'worker-failure',failureCode:'PRIVATE_SENTINEL'}),{status:'unavailable',qualification:false});
 assert.deepEqual(module.sanitizeWorkerDiagnostic({status:'inspected',rawHashMatches:true,sizeMatches:true,pdf:true,unresolvedTrackedChanges:0,embeddedComments:1,invalidSignatures:0,unverifiableSignatures:0,content:'PRIVATE_SENTINEL'}),{status:'inspected',qualification:false,rawHashMatches:true,sizeMatches:true,pdf:true,unresolvedTrackedChanges:0,embeddedComments:1,invalidSignatures:0,unverifiableSignatures:0});
});
test('diagnostic invocation pins original bytes/hash/size and the existing runner artifacts',()=>{
 assert.equal(typeof module.workerProbeArguments,'function');
 const asset={path:'notice.pdf',sha256:'a'.repeat(64),bytes:Buffer.alloc(20),mediaType:'application/pdf'};
 assert.deepEqual(module.workerProbeArguments({asset,assetDirectory:'/private/assets',worker:'/private/worker',pdfium:'/private/pdfium'}),['/private/assets/notice.pdf','/private/worker','/private/pdfium','a'.repeat(64),'20','application/pdf']);
 assert.throws(()=>module.workerProbeArguments({asset:{...asset,path:'../elsewhere'},assetDirectory:'/private/assets',worker:'/private/worker',pdfium:'/private/pdfium'}));
 assert.throws(()=>module.workerProbeArguments({asset:{...asset,mediaType:'text/plain'},assetDirectory:'/private/assets',worker:'/private/worker',pdfium:'/private/pdfium'}));
});
test('Cargo example uses qualified LinuxSandboxRunner with no direct worker or portable bypass',async()=>{
 const manifest=await readFile(new URL('../../../crates/document-semantic-inspection-runner/Cargo.toml',import.meta.url),'utf8');
 assert.match(manifest,/name = "document-load-inspection"/);
 const source=await readFile(new URL('../inspection-probe.rs',import.meta.url),'utf8');
 assert.match(source,/LinuxSandboxRunner::new/);assert.match(source,/runner\.inspect/);assert.match(source,/RunnerInput/);assert.match(source,/with_pdfium_runtime_dir/);
 assert.doesNotMatch(source,/DSI_SANDBOX_REQUIRED|run_worker_shell|Command::new|PdfAdapter|error\.to_string/);
});
