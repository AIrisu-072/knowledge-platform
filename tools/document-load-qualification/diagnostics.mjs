// Closed diagnostics only. Never retain Problem detail, trace IDs, author labels,
// locators, comment text, signature identities, parser messages, or file bytes.
const operations = new Set(['verifyHumanSession','verifyAgentSession','rootFolder','rootPolicy','createFolder','setFolderPolicy','create','detail','list','publish','updateMetadata','createNextVersion','denyAgent','agentRead','documentPolicy','revisions','revisionDetail','versions','versionDetail','versionFiles','downloadHash']);
const problemStatuses = Object.freeze({VALIDATION_FAILED:422,AUTHENTICATION_REQUIRED:401,FORBIDDEN:403,DOCUMENT_NOT_FOUND:404,DOCUMENT_VERSION_NOT_FOUND:404,REVISION_NOT_FOUND:404,FOLDER_NOT_FOUND:404,REVISION_CONFLICT:409,OPERATION_CONFLICT:409,CURSOR_STALE:409,STALE_VERSION:409,STALE_COMPARISON_INPUT:409,BUSINESS_RULE_REJECTED:422,RESERVED_DOCUMENT:409,FOLDER_CYCLE:409,ROOT_PROTECTED:409,IDENTITY_UNAVAILABLE:503,PUBLISH_QUALITY_REJECTED:422,UNSUPPORTED_MEDIA_TYPE:415,DEPENDENCY_UNAVAILABLE:503,TIMEOUT:504,COMMIT_OUTCOME_UNKNOWN:503,INTEGRITY_VIOLATION:500,INTERNAL:500});
export function sanitizeFailureDiagnostic(value) {
  if (!value || !operations.has(value.operation)) return null;
  const httpStatus = Number.isInteger(value.httpStatus) && value.httpStatus >= 400 && value.httpStatus <= 599 ? value.httpStatus : null;
  const problemCode = typeof value.problemCode === 'string' && Object.hasOwn(problemStatuses,value.problemCode) && problemStatuses[value.problemCode] === httpStatus ? value.problemCode : null;
  return {operation:value.operation,httpStatus,problemCode};
}
export function inspectionDiagnosticSql(fileId) {
  if (typeof fileId !== 'string' || !/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(fileId)) throw Error('Invalid owned file identity');
  return `SELECT json_build_object(
    'rowCount', count(*),
    'pdf', bool_and(s.detected_format = 'pdf'),
    'rawHashMatches', bool_and(s.observed_raw_content_hash = f.content_hash),
    'sizeMatches', bool_and(s.observed_size_bytes = f.size_bytes),
    'unresolvedTrackedChanges', COALESCE(sum((SELECT count(*) FROM jsonb_array_elements(s.editorial_provenance->'tracked_changes') change WHERE change->>'unresolved' = 'true')),0),
    'embeddedComments', COALESCE(sum(jsonb_array_length(s.editorial_provenance->'comments')),0),
    'invalidSignatures', COALESCE(sum((SELECT count(*) FROM jsonb_array_elements(s.digital_signature_evidence) signature WHERE signature->>'cryptographic_validity' = 'invalid')),0),
    'unverifiableSignatures', COALESCE(sum((SELECT count(*) FROM jsonb_array_elements(s.digital_signature_evidence) signature WHERE signature->>'cryptographic_validity' = 'unverifiable')),0),
    'diagnosticCount', COALESCE(sum(jsonb_array_length(s.diagnostics)),0)
  ) FROM document_semantic_inspections s JOIN file_objects f ON f.file_id = s.file_id
  WHERE s.file_id = '${fileId}'::uuid AND s.inspection_profile_version = 'dsi-v0'`;
}
export function sanitizeInspectionDiagnostic(value) {
  if (value?.rowCount === 0) return {status:'not-found'};
  const booleans=['pdf','rawHashMatches','sizeMatches'];
  const counts=['unresolvedTrackedChanges','embeddedComments','invalidSignatures','unverifiableSignatures','diagnosticCount'];
  if (value?.rowCount !== 1 || booleans.some(key=>typeof value[key]!=='boolean') || counts.some(key=>!Number.isSafeInteger(value[key]) || value[key]<0 || value[key]>1000000)) return {status:'unavailable'};
  return {status:'observed',rowCount:1,...Object.fromEntries([...booleans,...counts].map(key=>[key,value[key]]))};
}
