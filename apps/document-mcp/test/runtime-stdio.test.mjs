import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';

// The real acceptance executable and actual SDK subprocess run here against
// controlled HTTP contract responses. This is harness verification, not a real
// Document/DB/worker acceptance substitute.
for (const mutateDeniedState of [false, true]) test(mutateDeniedState
  ? 'actual acceptance rejects a denied comparison that changes authoritative state'
  : 'actual acceptance stdio proves distinct pairs before authorization denial and preserves business state', { timeout: 15_000 }, async t => {
  let sequence=0;
  const id=()=>`018f0000-0000-7000-8000-${(++sequence).toString(16).padStart(12,'0')}`;
  function document(key,count,mediaType='text/plain') {
    const documentId=id();
    const revisions=Array.from({length:count},(_,i)=>({revisionId:id(),documentVersionId:id(),major:count-i,minor:0,sourceKind:'contentPublication'}));
    const detail={documentId,title:`Synthetic ${key}`,metadata:{extensions:{retained:true}},currentVersionId:revisions[0].documentVersionId,revision:count+2,displayRevision:revisions[0]};
    const history={items:revisions.map(revision=>({sourceKey:`publish:${revision.revisionId}`,actionCode:'document.version.published',actor:{principalId:'poc-human'},provenanceQuality:'operationLedger'})),nextCursor:null};
    const versions=revisions.map(revision=>({versionId:revision.documentVersionId,versionNo:revision.major,files:[{mediaType}]}));
    const publications=history.items.map(item=>({...item,actor:item.actor.principalId}));
    return {key,detail,revisions,history,files:{items:[{logicalPath:'primary',ordinal:0,mediaType}]},snapshot:{...detail,revisions,versions,publications}};
  }
  const regulation=document('regulation',3), pdf=document('pdf',2,'application/pdf'), restricted=document('humanOnly',2), sandbox=document('sandbox',2);
  const docs=[regulation,pdf,restricted,sandbox],root=id(),shared=id(),privateFolder=id();
  let revoked=false,metadataWrites=0,policyWrites=0;
  const comparisons=[];
  async function serve(profile) {
    const server=createServer(async(req,res)=>{
      const url=new URL(req.url,'http://local');let raw='';for await(const chunk of req)raw+=chunk;
      const body=raw?JSON.parse(raw):undefined;
      const send=(value,status=200)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(value));};
      if(url.pathname==='/v1/session')return send({principal:{identityProvider:'poc',principalId:profile},invocationKind:profile==='poc-agent'?'agent':'human_interactive'});
      if(url.pathname==='/v1/folders/root')return send({folderId:root});
      if(url.pathname===`/v1/folders/${root}/children`)return send({items:[{folderId:shared}],nextCursor:null});
      if(url.pathname==='/v1/documents')return send({items:docs.filter(doc=>profile==='poc-human'||doc!==restricted&&(!revoked||doc!==sandbox)).map(doc=>({documentId:doc.detail.documentId})),nextCursor:null});
      const match=url.pathname.match(/^\/v1\/documents\/([^/]+)(.*)$/);
      const doc=docs.find(doc=>doc.detail.documentId===match?.[1]);
      assert.ok(doc,'the harness must use an existing document');
      const suffix=match[2];
      if(suffix==='/comparisons'||suffix==='/revision-comparisons') {
        const isRevision=suffix==='/revision-comparisons';
        const base=isRevision?body.baseRevisionId:body.baseVersionId,target=isRevision?body.targetRevisionId:body.targetVersionId;
        comparisons.push({profile,key:doc.key,isRevision,base,target,revoked});
        // Existing Application validates pair inequality before authorization.
        if(base===target)return send({code:'VALIDATION_ERROR',status:400},400);
        const existing=doc.revisions.map(revision=>isRevision?revision.revisionId:revision.documentVersionId);
        if(!existing.includes(base)||!existing.includes(target))return send({code:'DOCUMENT_VERSION_NOT_FOUND',status:404},404);
      }
      if(profile==='poc-agent'&&(doc===restricted||doc===sandbox&&revoked)) {
        if(mutateDeniedState&&suffix==='/comparisons')doc.detail={...doc.detail,revision:doc.detail.revision+1};
        return send({code:'DOCUMENT_NOT_FOUND',status:404,traceId:'synthetic'},404);
      }
      if(!suffix&&req.method==='GET')return send(doc.detail);
      if(suffix==='/revisions')return send({items:doc.revisions,nextCursor:null});
      if(suffix==='/history')return send(doc.history);
      if(suffix.match(/^\/versions\/[^/]+\/files$/))return send(doc.files);
      if(suffix==='/comparisons'||suffix==='/revision-comparisons') {
        const diff={projection:body.projection,verdict:'different',coverage:'full',resultDigest:'a'.repeat(64),unverifiedRegions:[],auditEventId:id()};
        if(suffix==='/revision-comparisons')return send({...diff,baseRevision:doc.revisions.find(r=>r.revisionId===body.baseRevisionId),targetRevision:doc.revisions.find(r=>r.revisionId===body.targetRevisionId),changes:[{facet:'text'}]});
        return send({...diff,items:[{facet:'text'}],nextCursor:null});
      }
      if(suffix==='/metadata'&&req.method==='PATCH') {
        assert.equal(profile,'poc-human');assert.equal(doc,sandbox);assert.equal(body.expectedDocumentRevision,doc.detail.revision);
        metadataWrites++;doc.detail={...doc.detail,metadata:{...doc.detail.metadata,...body.set},revision:doc.detail.revision+1,
          displayRevision:{...doc.detail.displayRevision,revisionId:id(),minor:doc.detail.displayRevision.minor+1,sourceKind:'metadataRevision'}};
        doc.revisions=[doc.detail.displayRevision,...doc.revisions];
        doc.history={...doc.history,items:[{sourceKey:`management:${body.operationId}`,actionCode:'document.metadata.changed',actor:{principalId:'poc-human'},provenanceQuality:'operationLedger',details:{changed:true,resulting_revision:doc.detail.revision}},...doc.history.items]};
        return send({operationId:body.operationId,resultingRevision:doc.detail.revision});
      }
      if(suffix==='/access-policy') {
        if(req.method==='GET')return send({policyRevision:0});
        assert.equal(profile,'poc-human');assert.equal(doc,sandbox);policyWrites++;revoked=true;return send({resultingRevision:1});
      }
      assert.fail(`unexpected synthetic route ${req.method} ${suffix}`);
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    t.after(()=>{server.closeAllConnections();server.close();});
    return `http://127.0.0.1:${server.address().port}`;
  }
  const human=await serve('poc-human'),agent=await serve('poc-agent');
  const dir=await mkdtemp(join(tmpdir(),'mcp-real-executable-'));t.after(()=>rm(dir,{recursive:true,force:true}));
  const manifestPath=join(dir,'manifest.json'),statePath=join(dir,'state.json'),contextPath=join(dir,'context.json');
  await writeFile(manifestPath,JSON.stringify({schemaVersion:1,baseUrl:human,rootFolderId:root,folders:{shared:{folderId:shared},humanOnly:{folderId:privateFolder}},documents:Object.fromEntries(docs.map(doc=>[doc.key,{create:{result:{documentId:doc.detail.documentId,documentVersionId:doc.revisions.at(-1).documentVersionId}},snapshot:{revisions:doc.revisions}}]))}));
  await writeFile(statePath,JSON.stringify({documents:[regulation,pdf].map(doc=>({key:doc.key,snapshot:doc.snapshot}))}));
  await writeFile(contextPath,JSON.stringify({runId:'synthetic-contract',human,agent,manifestPath,statePath}));
  const child=spawn(process.execPath,[new URL('../dist/runtime.cjs',import.meta.url).pathname],{env:{...process.env,KP_POC_RUNTIME_CONTEXT:contextPath},stdio:['ignore','pipe','pipe']});
  t.after(()=>child.kill());let output='';child.stdout.on('data',x=>output+=x);child.stderr.on('data',x=>output+=x);
  const exit=await new Promise(resolve=>child.on('exit',resolve));
  const evidence=JSON.parse(await readFile(join(dir,'agent-acceptance.json'),'utf8'));
  if(mutateDeniedState) {
    assert.equal(exit,1);assert.equal(evidence.phase,'denied-ids');assert.equal(evidence.failureCategory,'assertion');
    const denied=comparisons.find(call=>call.key==='humanOnly'&&call.profile==='poc-agent'&&!call.isRevision);
    assert.ok(denied);assert.notEqual(denied.base,denied.target);
    assert.equal(restricted.detail.revision,restricted.snapshot.revision+1);
    assert.equal(metadataWrites,0);assert.equal(policyWrites,0);
    return;
  }
  assert.equal(exit,0,`contract executable failed in ${evidence.phase}`);
  assert.equal(evidence.status,'PASS');assert.equal(evidence.phase,'complete');
  assert.equal(metadataWrites,1);assert.equal(policyWrites,1);
  for(const key of ['humanOnly','sandbox'])for(const isRevision of [false,true]) {
    const denialIndex=comparisons.findIndex(call=>call.key===key&&call.isRevision===isRevision&&call.profile==='poc-agent'&&(key==='humanOnly'||call.revoked));
    assert.ok(denialIndex>=0);const denial=comparisons[denialIndex];assert.notEqual(denial.base,denial.target);
    assert.ok(comparisons.slice(0,denialIndex).some(call=>call.key===key&&call.isRevision===isRevision&&call.profile==='poc-human'&&call.base===denial.base&&call.target===denial.target));
  }
  assert.doesNotMatch(output,/127\.0\.0\.1|Synthetic|retained/);
});
