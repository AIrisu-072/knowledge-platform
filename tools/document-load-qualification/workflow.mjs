import {randomBytes} from 'node:crypto';
import assert from 'node:assert/strict';
export function uuid7(){const b=randomBytes(16);b.writeUIntBE(Date.now(),0,6);b[6]=(b[6]&15)|112;b[8]=(b[8]&63)|128;const h=b.toString('hex');return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`;}
const HUMAN={subjectKind:'group',identityProvider:'poc',subjectId:'poc-users',actions:['read','readHistory','write','publish','administer']};
const AGENT={subjectKind:'group',identityProvider:'poc',subjectId:'poc-agents',actions:['read','readHistory']};
function equal(actual,expected,label){assert.deepEqual(actual,expected,label);}
async function listCheck(probe,folderId,expected,who='human'){equal((await probe.list(folderId,who)).sort(),[...expected].sort(),`${who} list membership mismatch`);}
export async function exerciseStage({probe,journal,count,assets,checkpoint,runId}){
 if(!Number.isSafeInteger(count)||count<2||count>100000||assets.length<2||new Set(assets.map(asset=>asset.sha256)).size!==assets.length)throw Error('Stage requires at least two documents and two distinct PDFs');
 const session=await probe.verifySessions();await checkpoint();
 const mutate=async(key,makeRequest,action,options)=>{await checkpoint();const request=journal.get(key)?.request??await makeRequest();return journal.perform(key,request,action,options);};
 const folderRequest=()=>({operationId:uuid7(),folderId:uuid7(),parentFolderId:session.rootFolderId,expectedParentRevision:session.root.revision,name:`Document load ${runId}`,reason:'Isolated synthetic-repetition qualification'});
 await mutate('folder',folderRequest,request=>probe.createFolder(request));
 const folderId=journal.get('folder').request.folderId;
 await mutate('folder-policy',()=>({operationId:uuid7(),expectedPolicyRevision:0,reason:'Fixed PoC qualification profiles',mode:'explicit',grants:[HUMAN,AGENT]}),request=>probe.setFolderPolicy(folderId,request));
 const ids=[],created=[];
 for(let i=0;i<count;i++){
  const asset=assets[i%assets.length];
  const tuple=await mutate(`create:${i}`,()=>({folderId,title:`Load document ${i}`,assetId:asset.id,sha256:asset.sha256}),request=>probe.create({folderId:request.folderId,title:request.title,bytes:asset.bytes,filename:`document-${i}.pdf`,mediaType:'application/pdf'}),{recoverable:false});
  ids.push(tuple.documentId);created.push(tuple);
  await mutate(`publish:${i}`,async()=>({operationId:uuid7(),expectedRevision:(await probe.detail(tuple.documentId)).revision}),request=>probe.publish(tuple.documentId,tuple.documentVersionId,request.expectedRevision,request.operationId));
 }
 await checkpoint();await listCheck(probe,folderId,ids);await listCheck(probe,folderId,ids,'agent');
 const first=ids[0];equal((await probe.detail(first)).currentVersionId,created[0].documentVersionId,'initial published pointer');
 await mutate('metadata',async()=>({operationId:uuid7(),expectedDocumentRevision:(await probe.detail(first)).revision}),request=>probe.updateMetadata(first,request.expectedDocumentRevision,request.operationId));
 const beforeNext=await probe.detail(first);
 equal(beforeNext.metadata.extensions?.loadQualificationOperation,journal.get('metadata').request.operationId,'metadata update readback');
 await mutate('stale-metadata',()=>({operationId:uuid7(),expectedDocumentRevision:journal.get('metadata').request.expectedDocumentRevision}),async request=>{
  try{await probe.updateMetadata(first,request.expectedDocumentRevision,request.operationId);}catch(error){if(error.status===409)return{status:409};throw error;}
  throw Error('stale metadata OCC request unexpectedly succeeded');
 });
 const nextAsset=assets[1];
 await mutate('next-version',()=>({operationId:uuid7(),expectedRevision:beforeNext.revision,targetVersionId:uuid7(),fileId:uuid7(),assetId:nextAsset.id,sha256:nextAsset.sha256}),request=>probe.createNextVersion(first,request.expectedRevision,request.operationId,request.targetVersionId,request.fileId,nextAsset));
 const nextId=journal.get('next-version').request.targetVersionId;
 equal((await probe.detail(first)).currentVersionId,created[0].documentVersionId,'working version must not change current pointer');
 await mutate('publish-next',async()=>({operationId:uuid7(),expectedRevision:(await probe.detail(first)).revision}),request=>probe.publish(first,nextId,request.expectedRevision,request.operationId));
 equal((await probe.detail(first)).currentVersionId,nextId,'new published current pointer');
 const privateDocumentId=ids.at(-1);
 equal(await probe.agentReadStatus(privateDocumentId),{status:200,allowed:true},'agent positive control');
 await mutate('deny-agent',async()=>({operationId:uuid7(),expectedPolicyRevision:(await probe.snapshot(privateDocumentId)).policy.policyRevision}),request=>probe.denyAgent(privateDocumentId,request.expectedPolicyRevision,request.operationId));
 equal(await probe.agentReadStatus(privateDocumentId),{status:404,allowed:false},'agent known-ID masked denial');
 await listCheck(probe,folderId,ids.filter(id=>id!==privateDocumentId),'agent');
 const sampledIndices=count<=20?[...Array(count).keys()]:[0,Math.floor(count/2),count-1];
 const snapshots={};
 for(const i of sampledIndices){await checkpoint();const id=ids[i];const snapshot=await probe.snapshot(id);
  const expectedHashes=i===0?[assets[0].sha256,assets[1].sha256]:[assets[i%assets.length].sha256];
  equal(snapshot.versions.flatMap(version=>version.files.map(file=>file.sha256)).sort(),expectedHashes.sort(),'downloaded original hash mismatch');
  snapshots[id]=snapshot;
 }
 return {status:'AWAITING_RESTART',folderId,documentIds:ids,privateDocumentId,sampledDocumentIds:sampledIndices.map(i=>ids[i]),snapshots,uniqueOriginalCount:assets.length,syntheticRepetitionCount:count,versionCount:count+1,contentQualityClaim:false};
}
export async function verifyRetained({probe,evidence,checkpoint}){
 await probe.verifySessions();await checkpoint();await listCheck(probe,evidence.folderId,evidence.documentIds);
 await listCheck(probe,evidence.folderId,evidence.documentIds.filter(id=>id!==evidence.privateDocumentId),'agent');
 equal(await probe.agentReadStatus(evidence.privateDocumentId),{status:404,allowed:false},'retained agent denial');
 for(const id of evidence.sampledDocumentIds){await checkpoint();equal(await probe.snapshot(id),evidence.snapshots[id],'retained state changed across restart');}
 return {status:'SUCCEEDED',verifiedDocuments:evidence.documentIds.length,snapshotCount:evidence.sampledDocumentIds.length};
}
