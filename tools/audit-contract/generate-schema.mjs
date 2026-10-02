import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
const root=resolve(import.meta.dirname,'../..');
export function renderSchema(catalog) {
if(catalog.version!==1) throw Error('unsupported catalog version');
const types=new Set();
for(const event of catalog.events){ if(types.has(event.type)) throw Error('duplicate event type'); types.add(event.type); }
const uuid={type:'string',pattern:'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',not:{const:'00000000-0000-0000-0000-000000000000'}};
const text={type:'string',minLength:1,maxLength:512,auditMaxUtf8Bytes:512,pattern:'^[^\\u0000-\\u001f\\u007f]+$'};
const object=(properties,required=Object.keys(properties))=>({type:'object',properties,required,additionalProperties:false});
const principal=object({identityProvider:text,principalId:text});
const counter={type:'integer',minimum:0,maximum:JSON.rawJSON("9223372036854775807")};
function field(f){switch(f.kind){
case'uuid':return uuid;case'nullable_uuid':return {anyOf:[uuid,{type:'null'}]};case'counter':return counter;case'positive_counter':return{...counter,minimum:1};case'boolean':return{type:'boolean'};
case'enum':return{enum:f.values};case'principal':return principal;
case'digest':return{type:'array',minItems:32,maxItems:32,items:{type:'integer',minimum:0,maximum:255}};
case'legacy_time':return {type:'array',minItems:9,maxItems:9,prefixItems:[{type:'integer',minimum:-9999,maximum:9999},{type:'integer',minimum:1,maximum:366},...Array.from({length:3},()=>({type:'integer',minimum:0,maximum:59})),{type:'integer',minimum:0,maximum:999999999},...Array.from({length:3},()=>({type:'integer',minimum:-59,maximum:59}))],items:false,auditLegacyTime:true};
default:throw Error('unknown catalog field');}}
const correlation=object({operation_id:uuid,publish_operation_id:uuid,trace_id:{type:'string',pattern:'^[0-9a-f]{32}$',not:{const:'0'.repeat(32)}},legacy_correlation_id:{...text,pattern:'^[A-Za-z0-9._:/-]+$'}},[]);
const actor=object({identity_provider:text,principal_id:text,kind:{const:'unknown'}});
const executor=object({identity_provider:text,principal_id:text});
const cases=catalog.events.filter(e=>!e.deferred).map(e=>{
 const data=object({schema_version:{const:1},category:{const:e.category},actor,action:{const:e.type},resource:object({type:{enum:e.resources},id:e.type==='authorization.denied'?{const:'00000000-0000-0000-0000-000000000000'}:uuid,version_id:uuid},e.requires_version?['type','id','version_id']:['type','id']),result:{const:e.result},correlation,metadata:object(Object.fromEntries(Object.entries(e.fields).map(([k,v])=>[k,field(v)])),e.required),provenance:object({source_format:{const:'document-audit-outbox-v0'},adapter_version:{const:1}}),service_executor:executor,reason_code:{type:'string',minLength:1,maxLength:64}},['schema_version','category','actor','action','resource','result','correlation','metadata','provenance']);
 if(!e.requires_version) delete data.properties.resource.properties.version_id;
 return object({specversion:{const:'1.0'},id:uuid,source:{const:e.source},type:{const:e.type},subject:text,time:{type:'string',format:'date-time',pattern:'Z$'},datacontenttype:{const:'application/json'},dataschema:{const:'urn:knowledge-platform:audit:event:1'},data});
});
const schema={$schema:'https://json-schema.org/draft/2020-12/schema',$id:'urn:knowledge-platform:audit:event:1',$comment:'Audit contract requires registered auditMaxUtf8Bytes, auditLegacyTime and auditEnvelopeBytes validators, plus audit-core semantic bindings and pre-parse wire byte limits. Unknown custom keywords MUST NOT be ignored.',oneOf:cases,auditEnvelopeBytes:32768};
return JSON.stringify(schema,null,2)+'\n';
}
if(process.argv[1] && resolve(process.argv[1])===resolve(import.meta.filename)){
 const catalog=JSON.parse(readFileSync(resolve(root,'spec/telemetry/audit-event-catalog.json'),'utf8'));
 const output=renderSchema(catalog); const file=resolve(root,'spec/telemetry/audit-event.schema.json');
 if(process.argv.includes('--check')) {
  if(readFileSync(file,'utf8')!==output) throw Error('audit schema is stale; regenerate from catalog');
 } else { writeFileSync(file,output); }
}
