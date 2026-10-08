import {isAbsolute,join} from 'node:path';
const codes=new Set(['unsupported_document_format','requires_ocr','encrypted_content_unsupported','format_mismatch','raw_binding_mismatch','semantic_extraction_failed','inspection_timeout','inspection_resource_limit_exceeded','extractor_unavailable','invalid_worker_result','parser_disagreement','unsupported_semantic_construct','malformed_request','worker_panicked']);
export function sanitizeWorkerDiagnostic(value){
 if(value?.status==='worker-failure' && codes.has(value.failureCode))return{status:'worker-failure',failureCode:value.failureCode,qualification:false};
 const bools=['rawHashMatches','sizeMatches','pdf'],counts=['unresolvedTrackedChanges','embeddedComments','invalidSignatures','unverifiableSignatures'];
 if(value?.status==='inspected' && bools.every(key=>typeof value[key]==='boolean') && counts.every(key=>Number.isSafeInteger(value[key])&&value[key]>=0&&value[key]<=1000000))return{status:'inspected',qualification:false,...Object.fromEntries([...bools,...counts].map(key=>[key,value[key]]))};
 return{status:'unavailable',qualification:false};
}
export function workerProbeArguments({asset,assetDirectory,worker,pdfium}){
 if(!asset || typeof asset.path!=='string' || !/^[a-zA-Z0-9_-]+\.pdf$/.test(asset.path) || !/^[a-f0-9]{64}$/.test(asset.sha256) || asset.mediaType!=='application/pdf' || !Buffer.isBuffer(asset.bytes) || asset.bytes.length<1 || asset.bytes.length>32*1024*1024 || ![assetDirectory,worker,pdfium].every(value=>typeof value==='string' && isAbsolute(value)))throw Error('Invalid bound worker diagnostic input');
 return[join(assetDirectory,asset.path),worker,pdfium,asset.sha256,String(asset.bytes.length),'application/pdf'];
}
