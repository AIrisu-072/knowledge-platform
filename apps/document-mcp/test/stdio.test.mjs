import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {Client} from '@modelcontextprotocol/client';
import {StdioClientTransport} from '@modelcontextprotocol/client/stdio';
const main=new URL('../dist/main.cjs',import.meta.url).pathname;
const session={principal:{identityProvider:'poc',principalId:'poc-agent'},invocationKind:'agent'};
async function backend(t,handle){const s=createServer(handle);await new Promise(r=>s.listen(0,'127.0.0.1',r));t.after(()=>{s.closeAllConnections();s.close();});return `http://127.0.0.1:${s.address().port}/`;}
function json(res,data){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(data));}
async function connect(t,url){const transport=new StdioClientTransport({command:process.execPath,args:[main],env:{KP_DOCUMENT_API_BASE_URL:url},stderr:'pipe'});let stderr='';transport.stderr.on('data',x=>stderr+=x);const client=new Client({name:'stdio-tests',version:'0.0.0'});await client.connect(transport);t.after(()=>client.close());return {client,transport,stderr:()=>stderr};}
test('real subprocess stdio initialize/list/call/close, protocol-only stdout',async t=>{const url=await backend(t,(req,res)=>json(res,req.url==='/v1/session'?session:{folderId:'synthetic-root'}));const {client,stderr}=await connect(t,url);assert.equal((await client.listTools()).tools.length,9);const result=await client.callTool({name:'document_get_root',arguments:{}});assert.equal(result.structuredContent.folderId,'synthetic-root');assert.equal(JSON.parse(result.content[0].text).folderId,'synthetic-root');await client.close();assert.equal(stderr(),'');});
test('actual protocol cancellation aborts accepted upstream request',async t=>{let accepted;const requestAccepted=new Promise(r=>accepted=r);let closed;const requestClosed=new Promise(r=>closed=r);const url=await backend(t,(req,res)=>{if(req.url==='/v1/session')return json(res,session);res.on('close',closed);accepted();});const {client}=await connect(t,url);const controller=new AbortController();const call=client.callTool({name:'document_get_root',arguments:{}},{signal:controller.signal});await requestAccepted;controller.abort();await assert.rejects(call);await Promise.race([requestClosed,new Promise((_,reject)=>setTimeout(()=>reject(new Error('HTTP request was not aborted')),3000).unref())]);});
test('tool deadline aborts upstream that accepts but never responds', {timeout:40000},async t=>{const url=await backend(t,(req,res)=>{if(req.url==='/v1/session')json(res,session);});const {client}=await connect(t,url);const start=Date.now();const result=await client.callTool({name:'document_get_root',arguments:{}},{timeout:39000});assert.equal(result.isError,true);assert.ok(Date.now()-start>=34000);assert.ok(Date.now()-start<39000);});
test('preflight deadline bounds silent upstream and emits no raw endpoint', {timeout:40000},async t=>{const url=await backend(t,()=>{});const child=spawn(process.execPath,[main],{env:{KP_DOCUMENT_API_BASE_URL:url},stdio:['pipe','pipe','pipe']});let out='',err='';child.stdout.on('data',x=>out+=x);child.stderr.on('data',x=>err+=x);t.after(()=>child.kill());const start=Date.now();const code=await new Promise(r=>child.on('exit',r));assert.equal(code,1);assert.equal(out,'');assert.match(err,/startup failed/);assert.ok(!err.includes(url));assert.ok(Date.now()-start<39000);});
test('EOF terminates initialized stdio cleanly',async t=>{const url=await backend(t,(req,res)=>json(res,session));const child=spawn(process.execPath,[main],{env:{KP_DOCUMENT_API_BASE_URL:url},stdio:['pipe','pipe','pipe']});t.after(()=>child.kill());child.stdin.end();assert.equal(await new Promise(r=>child.on('exit',r)),0);});
test('EOF during hung preflight promptly aborts startup HTTP', {timeout:6000},async t=>{let accepted;const requestAccepted=new Promise(r=>accepted=r);const url=await backend(t,()=>accepted());const child=spawn(process.execPath,[main],{env:{KP_DOCUMENT_API_BASE_URL:url},stdio:['pipe','pipe','pipe']});t.after(()=>child.kill());await requestAccepted;const exited=new Promise(r=>child.on('exit',r));child.stdin.end();await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(new Error('preflight ignored EOF')),2000).unref())]);});
test('human profile startup exits before exposing tools and redacts endpoint',async t=>{const url=await backend(t,(req,res)=>json(res,{...session,principal:{identityProvider:'poc',principalId:'poc-human'},invocationKind:'human_interactive'}));const child=spawn(process.execPath,[main],{env:{KP_DOCUMENT_API_BASE_URL:url},stdio:['pipe','pipe','pipe']});t.after(()=>child.kill());let out='',err='';child.stdout.on('data',x=>out+=x);child.stderr.on('data',x=>err+=x);assert.equal(await new Promise(r=>child.on('exit',r)),1);assert.equal(out,'');assert.match(err,/startup failed/);assert.ok(!err.includes(url));});

test('actual stdio preserves source-bound full comparisons and the runtime oracle still rejects ambiguous move plus edit', async t => {
  const { createRequire } = await import('node:module');
  const { assertComparisons } = createRequire(import.meta.url)('../dist/oracle.cjs');
  const id = '018f1234-1234-7234-8234-123456789abc';
  const baseId = '018f1234-1234-7234-8234-123456789abd';
  const targetId = '018f1234-1234-7234-8234-123456789abe';
  const source = { documentId:id,versionId:baseId,contentItemId:id,representationId:id,fileId:id,
    rawSha256:'a'.repeat(64),inspectionProfile:'dsi-v0',locator:{kind:'textSpan',line:1,byteStart:0,byteEnd:1},
    granularity:'exact',parserProvenance:'document-diff-v0' };
  const revision = { projection:'diff',baseRevision:{revisionId:baseId,documentVersionId:baseId},
    targetRevision:{revisionId:targetId,documentVersionId:targetId},contentComparisonStatus:'differentAuthoritativeVersions',
    verdict:'different',coverage:'full',resultDigest:'b'.repeat(64),
    changes:[{operation:'modified',relocation:null,facet:'text',base:source,target:{...source,versionId:targetId,rawSha256:'c'.repeat(64)},reasonCode:'text_changed'}],
    rows:[],unverifiedRegions:[],ancillaryChanges:[],metadataComparisonStatus:'same',metadataChanges:[],
    baseMetadataSnapshotDigest:'d'.repeat(64),targetMetadataSnapshotDigest:'d'.repeat(64),
    displayItems:[],auditEventId:'agent-revision',contentAuditEventId:'agent-content' };
  const version = { projection:'display',verdict:'different',coverage:'full',resultDigest:revision.resultDigest,
    items:[{changeIndex:0,operation:'modified',relocation:null,facet:'text',baseLocator:source.locator,targetLocator:source.locator,
      base:{kind:'text',text:'a',truncated:false,locator:source.locator},target:{kind:'text',text:'c',truncated:false,locator:source.locator}}],
    unverifiedRegions:[],pageSize:100,nextCursor:null,auditEventId:'agent-display',resultAuditEventId:'agent-result' };
  let responseRevision=revision, responseVersion=version;
  const requests=[];
  const url=await backend(t,(req,res)=>{
    if(req.url==='/v1/session')return json(res,session);
    let body='';req.on('data',x=>body+=x);req.on('end',()=>{
      requests.push({path:req.url,body:JSON.parse(body)});
      json(res,req.url.endsWith('/revision-comparisons')?responseRevision:responseVersion);
    });
  });
  const {client,stderr}=await connect(t,url);
  const revisionArgs={documentId:id,baseRevisionId:baseId,targetRevisionId:targetId,projection:'diff'};
  const versionArgs={documentId:id,baseVersionId:baseId,targetVersionId:targetId,profile:'document-diff-v0',projection:'display',pageSize:100};
  async function pair(){
    const r=await client.callTool({name:'document_compare_revisions',arguments:revisionArgs});
    const v=await client.callTool({name:'document_compare_versions',arguments:versionArgs});
    for(const result of [r,v]){assert.notEqual(result.isError,true);assert.deepEqual(JSON.parse(result.content[0].text),result.structuredContent);}
    return [r.structuredContent,v.structuredContent];
  }
  const [r,v]=await pair();
  assert.deepEqual(r,revision);assert.deepEqual(v,version);
  assertComparisons(r,v,{...revision,auditEventId:'human-revision',contentAuditEventId:'human-content'},
    {...version,auditEventId:'human-display',resultAuditEventId:'human-result'},baseId,targetId);
  assert.equal(requests[1].body.profile,'document-diff-v0');
  assert.equal(requests[0].body.baseRevisionId,baseId);assert.equal(requests[1].body.targetVersionId,targetId);
  const altered=structuredClone(revision);altered.changes[0].base.rawSha256='e'.repeat(64);
  assert.throws(()=>assertComparisons(r,v,altered,version,baseId,targetId));
  const region={reason:'ambiguousAlignment',base:source,target:null,navigationHint:null};
  responseRevision={...revision,verdict:'unknown',coverage:'none',changes:[],unverifiedRegions:[region]};
  responseVersion={...version,verdict:'unknown',coverage:'none',items:[],unverifiedRegions:[region]};
  const [unknownRevision,unknownVersion]=await pair();
  assert.deepEqual(unknownRevision,responseRevision);assert.deepEqual(unknownVersion,responseVersion);
  assert.throws(()=>assertComparisons(unknownRevision,unknownVersion,responseRevision,responseVersion,baseId,targetId));
  await client.close();assert.equal(stderr(),'');
});
