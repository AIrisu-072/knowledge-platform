import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const module = await import('../diagnostics.mjs').catch(()=>({}));
const fileId='0198eada-1234-7000-8000-000000000001';
test('failure projection keeps only known operation/status/problem combinations',()=>{
 assert.equal(typeof module.sanitizeFailureDiagnostic,'function');
 const safe=module.sanitizeFailureDiagnostic({operation:'publish',httpStatus:422,problemCode:'PUBLISH_QUALITY_REJECTED',detail:'PRIVATE_SENTINEL',traceId:'secret'});
 assert.deepEqual(safe,{operation:'publish',httpStatus:422,problemCode:'PUBLISH_QUALITY_REJECTED'});
 assert.equal(module.sanitizeFailureDiagnostic({operation:'PRIVATE_SENTINEL',httpStatus:422,problemCode:'PUBLISH_QUALITY_REJECTED'}),null);
 assert.deepEqual(module.sanitizeFailureDiagnostic({operation:'publish',httpStatus:422,problemCode:'PRIVATE_SENTINEL'}),{operation:'publish',httpStatus:422,problemCode:null});
});
test('DSI SQL is read-only, one strict owned UUID, and projects counts/bindings without document content',()=>{
 assert.equal(typeof module.inspectionDiagnosticSql,'function');
 const query=module.inspectionDiagnosticSql(fileId);
 assert.match(query,/document_semantic_inspections/);assert.match(query,/file_id = '0198eada-1234-7000-8000-000000000001'::uuid/);
 assert.match(query,/jsonb_array_length/);assert.match(query,/cryptographic_validity/);
 assert.doesNotMatch(query,/SELECT\s+\*|author_label|source_locator|certificate_subject|signer_claim|->>'content'|UPDATE|DELETE|INSERT|DROP/i);
 for(const id of ["'; DROP TABLE foo;--",'',null,'not-a-uuid'])assert.throws(()=>module.inspectionDiagnosticSql(id));
});
test('DSI output has a closed bounded scalar schema; missing or malformed evidence stays unavailable',()=>{
 assert.equal(typeof module.sanitizeInspectionDiagnostic,'function');
 const row={rowCount:1,pdf:true,rawHashMatches:true,sizeMatches:true,unresolvedTrackedChanges:0,embeddedComments:2,invalidSignatures:0,unverifiableSignatures:0,diagnosticCount:0,content:'PRIVATE_SENTINEL'};
 const result=module.sanitizeInspectionDiagnostic(row);
 assert.deepEqual(result,{status:'observed',rowCount:1,pdf:true,rawHashMatches:true,sizeMatches:true,unresolvedTrackedChanges:0,embeddedComments:2,invalidSignatures:0,unverifiableSignatures:0,diagnosticCount:0});
 assert.deepEqual(module.sanitizeInspectionDiagnostic({rowCount:0}),{status:'not-found'});
 for(const bad of [{...row,embeddedComments:null},{...row,invalidSignatures:-1},{...row,pdf:'PRIVATE_SENTINEL'},{...row,rowCount:2}])assert.deepEqual(module.sanitizeInspectionDiagnostic(bad),{status:'unavailable'});
});
test('diagnostic SQL follows exact persisted DSI field names and rejected-quality predicates',async()=>{
 assert.equal(typeof module.inspectionDiagnosticSql,'function');
 const sql=module.inspectionDiagnosticSql(fileId);
 const source=await readFile(new URL('../../../crates/document-application/src/publish_quality.rs',import.meta.url),'utf8');
 for(const field of ['tracked_changes','comments','cryptographic_validity']){assert.ok(sql.includes(field));assert.ok(source.includes(field));}
 assert.match(sql,/observed_raw_content_hash = f.content_hash/);assert.match(sql,/observed_size_bytes = f.size_bytes/);
});
test('publication prerequisites are bound to one owned raw hash and contain only scalar state',()=>{
 assert.equal(typeof module.publicationPrerequisiteSql,'function');
 const sql=module.publicationPrerequisiteSql(fileId,'a'.repeat(64),200);
 assert.match(sql,/requires_content_classification/);assert.match(sql,/lifecycle_state = 'WORKING'/);assert.match(sql,/f\.media_type = 'application\/pdf'/);assert.match(sql,/encode\(f\.content_hash, 'hex'\)/);
 assert.match(sql,/f\.size_bytes = 200/);
 assert.doesNotMatch(sql,/SELECT\s+\*|storage_locator|original_filename|title|metadata/i);
 assert.throws(()=>module.publicationPrerequisiteSql(fileId,"';bad"));
 const input={fileCount:1,versionCount:1,authoritativeItemCount:1,isWorking:true,requiresContentClassification:false,mediaTypeMatches:true,rawHashMatches:true,sizeMatches:true,secret:'PRIVATE_SENTINEL'};
 assert.deepEqual(module.sanitizePublicationPrerequisites(input),{status:'observed',fileCount:1,versionCount:1,authoritativeItemCount:1,isWorking:true,requiresContentClassification:false,mediaTypeMatches:true,rawHashMatches:true,sizeMatches:true});
});
