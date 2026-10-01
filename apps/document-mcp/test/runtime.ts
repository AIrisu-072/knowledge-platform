/** Mandatory real-runtime acceptance. Only run inside the owned C1 synthetic process harness. */
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { Client as McpClient } from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';
import { createClient, createConfig, getDocument, listDocumentRevisions, listVersionFiles, getDocumentHistory, compareDocumentRevisions, compareDocumentVersions, patchDocumentMetadata, getDocumentAccessPolicy, setDocumentAccessPolicy, type CommandsSetAccessPolicy, type CommandsPolicyExplicit, type ModelsDocumentRevisionPage, type PublishedDocumentDetail, type ModelsFileList } from '@knowledge-platform/document-api-client';
import { guiOracle, assertSnapshotMatches, assertHistoryMatches, assertComparisons, assertMetadataUpdate } from './runtime-oracle';
function uuidV7(): string { const b=randomBytes(16); b.writeUIntBE(Date.now(),0,6);b[6]=(b[6]!&15)|112;b[8]=(b[8]!&63)|128;const x=b.toString('hex');return `${x.slice(0,8)}-${x.slice(8,12)}-${x.slice(12,16)}-${x.slice(16,20)}-${x.slice(20)}`; }
async function run(): Promise<void> {
  const contextPath=process.env.KP_POC_RUNTIME_CONTEXT;
  assert.ok(contextPath,'KP_POC_RUNTIME_CONTEXT must identify the owned real-runtime run');
  const context=JSON.parse(await readFile(contextPath,'utf8'));
  assert.ok(context.runId && context.human && context.agent && context.manifestPath);
  const manifest=JSON.parse(await readFile(context.manifestPath,'utf8'));
  assert.equal(manifest.schemaVersion,1);
  assert.equal(new URL(manifest.baseUrl).origin,new URL(context.human).origin);
  const human=createClient(createConfig({baseUrl:context.human,redirect:'error',credentials:'omit'}));
  const http={client:human,throwOnError:true as const,get signal(){return AbortSignal.timeout(50_000)}};
  const client=new McpClient({name:'document-real-runtime-acceptance',version:'0.0.0'});
  const transport=new StdioClientTransport({command:process.execPath,args:[join(__dirname,'main.cjs')],env:{KP_DOCUMENT_API_BASE_URL:context.agent},stderr:'pipe'});
  let stderr='';transport.stderr!.on('data',chunk=>stderr+=chunk);
  const sha256=(bytes:Uint8Array)=>createHash('sha256').update(bytes).digest('hex');
  const repository=join(__dirname,'../../..');
  const evidence: Record<string,unknown>={runId:context.runId,adapter:'actual SDK subprocess stdio',syntheticOnly:true,phase:'gui-oracle',
    sourceHead:execFileSync('git',['rev-parse','HEAD'],{cwd:repository,encoding:'utf8'}).trim(),nodeVersion:process.version,
    mainSha256:sha256(await readFile(join(__dirname,'main.cjs'))),runtimeSha256:sha256(await readFile(join(__dirname,'runtime.cjs'))),
    workspaceLockSha256:sha256(await readFile(join(repository,'pnpm-lock.yaml'))),runtimeContextSha256:sha256(await readFile(contextPath)),checks:[]};
  const checks=evidence.checks as string[];
  async function call<T>(name:string,args:Record<string,unknown>):Promise<T> { const result=await client.callTool({name,arguments:args},{timeout:55_000}); assert.notEqual(result.isError,true,`${name}: ${JSON.stringify(result)}`);assert.deepEqual(JSON.parse((result.content[0] as {text:string}).text),result.structuredContent);return result.structuredContent as T; }
  async function denied(name:string,args:Record<string,unknown>):Promise<void>{const result=await client.callTool({name,arguments:args},{timeout:55_000});assert.equal(result.isError,true,name);const problem=result.structuredContent as {status:number};assert.ok([403,404].includes(problem.status),JSON.stringify(result));}
  try {
    const oracle=guiOracle(JSON.parse(await readFile(context.statePath,'utf8')),manifest.documents.regulation.create.result.documentId);
    evidence.phase='stdio-initialize';
    await client.connect(transport);
    evidence.phase='discovery';
    const listed=await client.listTools();assert.deepEqual(listed.tools.map(tool=>tool.name).sort(),['document_get_root','document_list_folder','document_list','document_get','document_list_revisions','document_get_history','document_compare_versions','document_compare_revisions','document_list_files'].sort());checks.push('exact nine-tool discovery');
    evidence.phase='root-navigation';
    const root=await call<{folderId:string}>('document_get_root',{});assert.equal(root.folderId,manifest.rootFolderId);
    const children=await call<{items:Array<{folderId:string}>}>('document_list_folder',{folderId:root.folderId,pageSize:200});assert.ok(children.items.some(x=>x.folderId===manifest.folders.shared.folderId));assert.ok(!children.items.some(x=>x.folderId===manifest.folders.humanOnly.folderId));checks.push('root and authorized children');
    const regulation=manifest.documents.regulation.create.result.documentId;
    const restricted=manifest.documents.humanOnly.create.result.documentId;
    const restrictedVersion=manifest.documents.humanOnly.create.result.documentVersionId;
    evidence.phase='list-visibility';
    for(const view of ['published','authoring','history']){const docs=await call<{items:Array<{documentId:string}>}>('document_list',{view,pageSize:200});assert.ok(!docs.items.some(x=>x.documentId===restricted));if(view==='published')assert.ok(docs.items.some(x=>x.documentId===regulation));}checks.push('all document views exclude human-only document');
    evidence.phase='shared-state';
    const humanDetail=(await getDocument({...http,path:{documentId:regulation},query:{view:'published'}})).data as PublishedDocumentDetail;
    const detail=await call<PublishedDocumentDetail>('document_get',{documentId:regulation,view:'published'});
    for(const key of ['documentId','currentVersionId','revision','title','metadata','displayRevision'] as const)assert.deepEqual(detail[key],humanDetail[key],key);
    const revisions=await call<ModelsDocumentRevisionPage>('document_list_revisions',{documentId:regulation,pageSize:200});
    const humanRevisions=(await listDocumentRevisions({...http,path:{documentId:regulation},query:{pageSize:200}})).data;assert.deepEqual(revisions,humanRevisions);assertSnapshotMatches(oracle.regulation,detail,revisions);
    const pdfId=oracle.pdf.documentId as string;
    const pdfDetail=await call('document_get',{documentId:pdfId,view:'published'});
    const pdfRevisions=await call('document_list_revisions',{documentId:pdfId,pageSize:200});
    assertSnapshotMatches(oracle.pdf,pdfDetail,pdfRevisions);checks.push('GUI-created latest Version and human Major.Minor Revisions match shared state');
    evidence.phase='history-comparisons';
    const history=await call('document_get_history',{documentId:regulation,pageSize:200});
    assertHistoryMatches(oracle.regulation,history);
    assertHistoryMatches(oracle.regulation,(await getDocumentHistory({...http,path:{documentId:regulation},query:{pageSize:200}})).data);
    const first=revisions.items[0]!;const last=revisions.items.at(-1)!;
    const revisionBody={baseRevisionId:last.revisionId,targetRevisionId:first.revisionId,projection:'diff' as const};
    const versionBody={baseVersionId:last.documentVersionId,targetVersionId:first.documentVersionId,profile:'document-diff-v0' as const,projection:'display' as const,pageSize:100};
    const revisionComparison=await call('document_compare_revisions',{documentId:regulation,...revisionBody});
    const versionComparison=await call('document_compare_versions',{documentId:regulation,...versionBody});
    const humanRevisionComparison=(await compareDocumentRevisions({...http,path:{documentId:regulation},body:revisionBody})).data;
    const humanVersionComparison=(await compareDocumentVersions({...http,path:{documentId:regulation},body:versionBody})).data;
    assertComparisons(revisionComparison,versionComparison,humanRevisionComparison,humanVersionComparison,last.revisionId,first.revisionId);
    evidence.phase='files';
    const files=await call<ModelsFileList>('document_list_files',{documentId:regulation,versionId:detail.currentVersionId,purpose:'published'});assert.deepEqual(files,(await listVersionFiles({...http,path:{documentId:regulation,versionId:detail.currentVersionId},query:{purpose:'published'}})).data);checks.push('history, both comparisons and file metadata through actual API');
    evidence.phase='denied-ids';
    await denied('document_get',{documentId:restricted,view:'published'});await denied('document_get_history',{documentId:restricted});await denied('document_list_revisions',{documentId:restricted});await denied('document_list_files',{documentId:restricted,versionId:restrictedVersion,purpose:'history'});await denied('document_compare_versions',{documentId:restricted,baseVersionId:restrictedVersion,targetVersionId:restrictedVersion,profile:'document-diff-v0',projection:'diff'});const restrictedRevision=manifest.documents.humanOnly.snapshot.revisions[0].revisionId;
    await denied('document_compare_revisions',{documentId:restricted,baseRevisionId:restrictedRevision,targetRevisionId:restrictedRevision,projection:'diff'});checks.push('known unauthorized IDs denied');
    // Sandbox is deliberately separate from regulation's GUI/restart oracle. Mutations use only the human API.
    evidence.phase='metadata-update';
    const sandbox=manifest.documents.sandbox.create.result.documentId;
    const before=await call<PublishedDocumentDetail>('document_get',{documentId:sandbox,view:'published'});
    const metadataOperationId=uuidV7();
    await patchDocumentMetadata({...http,path:{documentId:sandbox},body:{operationId:metadataOperationId,expectedDocumentRevision:before.revision,set:{pocAgentObservation:context.runId},unset:[],reason:'Synthetic Human to Agent consistency acceptance'}});
    const after=await call<PublishedDocumentDetail>('document_get',{documentId:sandbox,view:'published'});assert.equal(after.metadata?.pocAgentObservation,context.runId);assert.ok(after.revision>before.revision);assert.notEqual(after.displayRevision?.revisionId,before.displayRevision?.revisionId);checks.push('human metadata update immediately visible through Agent');
    const sandboxRevisions=await call<ModelsDocumentRevisionPage>('document_list_revisions',{documentId:sandbox});
    const sandboxHistory=await call('document_get_history',{documentId:sandbox});
    assertMetadataUpdate(before,after,sandboxRevisions,sandboxHistory,context.runId,metadataOperationId);
    assert.deepEqual(sandboxRevisions,(await listDocumentRevisions({...http,path:{documentId:sandbox}})).data);
    assert.deepEqual(sandboxHistory,(await getDocumentHistory({...http,path:{documentId:sandbox}})).data);
    checks.push('exact published metadataRevision and Human operation history visible through MCP');
    evidence.phase='acl-revocation';
    const policy=(await getDocumentAccessPolicy({...http,path:{documentId:sandbox}})).data;
    const revoke: CommandsPolicyExplicit = {operationId:uuidV7(),expectedPolicyRevision:policy.policyRevision,reason:'Synthetic revoke after successful Agent read',mode:'explicit',grants:[{subjectKind:'group',identityProvider:'poc',subjectId:'poc-users',actions:['read','readHistory','write','publish','administer']}]};
    // Existing generated allOf union is unsatisfiable; same typed variant bridge as the approved seed.
    await setDocumentAccessPolicy({...http,path:{documentId:sandbox},body:revoke as unknown as CommandsSetAccessPolicy});
    await denied('document_get',{documentId:sandbox,view:'published'});await denied('document_get_history',{documentId:sandbox});await denied('document_list_revisions',{documentId:sandbox});await denied('document_list_files',{documentId:sandbox,versionId:after.currentVersionId,purpose:'history'});
    await denied('document_compare_versions',{documentId:sandbox,baseVersionId:after.currentVersionId,targetVersionId:after.currentVersionId,profile:'document-diff-v0',projection:'diff'});
    await denied('document_compare_revisions',{documentId:sandbox,baseRevisionId:sandboxRevisions.items[0]!.revisionId,targetRevisionId:sandboxRevisions.items.at(-1)!.revisionId,projection:'diff'});
    const filtered=await call<{items:Array<{documentId:string}>}>('document_list',{view:'published',pageSize:200});assert.ok(!filtered.items.some(x=>x.documentId===sandbox));checks.push('current authorization after revocation: detail/history/revisions/files/both comparisons/list');
    evidence.phase='stdio-cleanup';
    assert.equal(stderr,'');
    evidence.status='PASS';evidence.phase='complete';
  } catch(error) { evidence.status='FAIL';evidence.failureCategory=(error as {code?:string})?.code==='ERR_ASSERTION'?'assertion':'execution-failure';throw error;
  } finally { await client.close();await writeFile(join(dirname(contextPath),'agent-acceptance.json'),JSON.stringify(evidence,null,2)+'\n'); }
  console.log('PASS actual Document MCP real-runtime acceptance');
}
run().catch(()=>{console.error('Document MCP real-runtime acceptance failed; inspect the owned run evidence');process.exitCode=1;});
