import {isDeepStrictEqual} from 'node:util';
import {uuid7,qualificationStageSuffix} from './workflow.mjs';
import {sanitizeFailureDiagnostic, sanitizeInspectionDiagnostic, sanitizePublicationPrerequisites} from './diagnostics.mjs';
import {sanitizeWorkerDiagnostic} from './worker-probe.mjs';

const HUMAN = {subjectKind:'group', identityProvider:'poc', subjectId:'poc-users', actions:['read','readHistory','write','publish','administer']};
const EXPECTED_FAILURE = {operation:'publish', httpStatus:422, problemCode:'BUSINESS_RULE_REJECTED'};
const EXPECTED_BINDING = {status:'observed', fileCount:1, versionCount:1, authoritativeItemCount:1, isWorking:true, requiresContentClassification:false, mediaTypeMatches:true, rawHashMatches:true, sizeMatches:true};
const EXPECTED_WORKER = {status:'worker-failure', failureCode:'unsupported_semantic_construct', qualification:false};
const BUDGET_CODES = new Set(['wall-budget-exhausted','resource-budget-exhausted']);
const FAILURE_CODES = new Set([
  'negative-assets-invalid', 'negative-diagnostics-required',
  'negative-create-outcome-unknown', 'negative-created-identity-invalid',
  'negative-original-binding-drift', 'negative-snapshot-invalid',
  'negative-publication-unexpected-success', 'negative-publication-rejection-mismatch',
  'negative-recorded-rejection-invalid', 'negative-rejection-state-changed',
  'negative-inspection-unavailable', 'negative-inspection-not-absent',
  'negative-worker-unavailable', 'negative-worker-rejection-mismatch',
  'negative-publication-binding-mismatch', 'negative-document-membership-mismatch',
  'negative-retention-evidence-invalid', 'negative-retained-state-changed',
  'negative-prerequisite-failed',
]);

class NegativeQualificationError extends Error {
  constructor(code, diagnostic) {
    super(code); this.name = 'NegativeQualificationError'; this.code = code;
    if (diagnostic) this.diagnostic = diagnostic;
  }
}
/** Only errors originating here can supply a public fixed failure category. */
export function sanitizeNegativeFailureCode(error) {
  return error instanceof NegativeQualificationError && FAILURE_CODES.has(error.code) ? error.code : undefined;
}
const fail = (code, diagnostic) => {throw new NegativeQualificationError(code, diagnostic);};
function sanitizedError(error) {
  if (error instanceof NegativeQualificationError) return error;
  if (BUDGET_CODES.has(error?.code)) return new NegativeQualificationError(error.code);
  if (error?.message === 'Initial create outcome unknown; never repeat POST') return new NegativeQualificationError('negative-create-outcome-unknown');
  return new NegativeQualificationError('negative-prerequisite-failed', sanitizeFailureDiagnostic(error?.diagnostic));
}
function counts(number) {
  return {targetDocuments:number, confirmedCreatedDocuments:number, confirmedRejectedDocuments:number, confirmedPublishedDocuments:0, confirmedHttp422Responses:number};
}
const notRun = () => ({status:'NOT_RUN', documentIds:[], documents:[], counts:counts(0), contentQualityClaim:false});
async function guarded(checkpoint, action) {await checkpoint(); return action();}
function humanOnly(policy, folderId) {
  if (policy?.effectiveSource?.kind !== 'folder' || policy.effectiveSource.id !== folderId || !Array.isArray(policy.effectiveGrants)) return false;
  const grants = policy.effectiveGrants.map(grant => ({subjectKind:grant.subjectKind, identityProvider:grant.identityProvider, subjectId:grant.subjectId, actions:Array.isArray(grant.actions) ? [...grant.actions].sort() : null}));
  return isDeepStrictEqual(grants,[{...HUMAN,actions:[...HUMAN.actions].sort()}]);
}
function validateSnapshot(snapshot, {documentId, documentVersionId, sha256, sizeBytes}, folderId) {
  const detail = snapshot?.detail;
  if (!detail || detail.documentId !== documentId || detail.documentVersionId !== documentVersionId || detail.folderId !== folderId
    || detail.currentVersionId !== null || detail.lifecycleState !== 'working' || !Number.isSafeInteger(detail.revision) || detail.revision < 0
    || !humanOnly(snapshot.policy,folderId) || !Array.isArray(snapshot.revisions) || snapshot.revisions.length !== 0
    || !Array.isArray(snapshot.versions) || snapshot.versions.length !== 1) fail('negative-snapshot-invalid');
  const version = snapshot.versions[0];
  for (const part of [version.summary,version.detail]) {
    if (part?.versionId !== documentVersionId || part.lifecycleState !== 'working' || part.isCurrent !== false || part.publishedAt !== null) fail('negative-snapshot-invalid');
  }
  // The API file projection has no FileId. The owned FileId is independently
  // bound by the read-only worker prerequisites; here prove the exact original.
  if (!Array.isArray(version.files) || version.files.length !== 1) fail('negative-snapshot-invalid');
  const file = version.files[0];
  if (file.sha256 !== sha256 || file.mediaType !== 'application/pdf' || file.sizeBytes !== sizeBytes || file.downloadedBytes !== sizeBytes
    || typeof file.contentItemId !== 'string' || !file.contentItemId || typeof file.representationId !== 'string' || !file.representationId) fail('negative-snapshot-invalid');
}
async function missingInspection({diagnosePublication, checkpoint}, fileId) {
  await checkpoint();
  let value;
  try {value = await diagnosePublication(fileId);} catch {fail('negative-inspection-unavailable');}
  const diagnostic = sanitizeInspectionDiagnostic(value);
  if (diagnostic.status !== 'not-found') fail('negative-inspection-not-absent');
  return diagnostic;
}
async function workerRejection({diagnoseWorker, checkpoint}, item) {
  await checkpoint();
  let value;
  try {value = await diagnoseWorker(item.fileId,{assetId:item.assetId,sha256:item.sha256});} catch {fail('negative-worker-unavailable');}
  const workerDiagnostic = sanitizeWorkerDiagnostic(value);
  if (value?.qualification !== false || !isDeepStrictEqual(workerDiagnostic,EXPECTED_WORKER)) fail('negative-worker-rejection-mismatch');
  const publicationPrerequisites = sanitizePublicationPrerequisites(value?.binding);
  if (value?.binding?.status !== 'observed' || !isDeepStrictEqual(publicationPrerequisites,EXPECTED_BINDING)) fail('negative-publication-binding-mismatch');
  return {workerDiagnostic,publicationPrerequisites};
}
async function verifyMembership(probe, checkpoint, folderId, ids) {
  const actual = await guarded(checkpoint,() => probe.list(folderId));
  if (!Array.isArray(actual) || !isDeepStrictEqual([...actual].sort(),[...ids].sort())) fail('negative-document-membership-mismatch');
}

/** Expected failure is an explicit API test, never a successful publication. */
export async function exerciseNegative({probe, journal, assets, checkpoint, runId, stageLabel, diagnosePublication, diagnoseWorker}) {
  try {
    if (!Array.isArray(assets) || assets.length > 20
      || assets.some(asset => !asset || asset.expectedOutcome !== 'reject-unsupported')
      || new Set(assets.map(asset => asset.id)).size !== assets.length
      || new Set(assets.map(asset => asset.sha256)).size !== assets.length) fail('negative-assets-invalid');
    if (assets.length === 0) return notRun();
    if (typeof diagnosePublication !== 'function' || typeof diagnoseWorker !== 'function') fail('negative-diagnostics-required');
    const suffix = qualificationStageSuffix(stageLabel);
    const session = await guarded(checkpoint,() => probe.verifySessions());
    const mutate = async (key, makeRequest, action, options) => {
      await checkpoint();
      const request = journal.get(key)?.request ?? await makeRequest();
      return journal.perform(key,request,action,options);
    };
    await mutate('negative-folder',() => ({operationId:uuid7(),folderId:uuid7(),parentFolderId:session.rootFolderId,expectedParentRevision:session.root.revision,name:`Document negative ${runId}${suffix}`,reason:'Isolated unsupported official PDF qualification'}),request => probe.createFolder(request));
    const folderId = journal.get('negative-folder').request.folderId;
    await mutate('negative-folder-policy',() => ({operationId:uuid7(),expectedPolicyRevision:0,reason:'Private fixed PoC negative qualification',mode:'explicit',grants:[HUMAN]}),request => probe.setFolderPolicy(folderId,request));
    const documents = [];
    for (const [index, asset] of assets.entries()) {
      const declaration = {folderId,title:`Negative official PDF ${index}`,assetId:asset.id,sha256:asset.sha256,sizeBytes:asset.bytes.length,expectedOutcome:'reject-unsupported'};
      const prior = journal.get(`negative-create:${index}`);
      if (prior && !isDeepStrictEqual(prior.request,declaration)) fail('negative-original-binding-drift');
      const tuple = await mutate(`negative-create:${index}`,() => declaration,request => probe.create({folderId:request.folderId,title:request.title,bytes:asset.bytes,filename:`negative-${index}.pdf`,mediaType:'application/pdf'}),{recoverable:false});
      if (!tuple || !['documentId','documentVersionId','fileId'].every(key => typeof tuple[key] === 'string' && tuple[key])) fail('negative-created-identity-invalid');
      const item = {assetId:asset.id,sha256:asset.sha256,sizeBytes:asset.bytes.length,documentId:tuple.documentId,documentVersionId:tuple.documentVersionId,fileId:tuple.fileId};
      const snapshot = await mutate(`negative-baseline:${index}`,() => item,async () => {
        const value = await probe.snapshot(item.documentId,{purpose:'authoring'});
        validateSnapshot(value,item,folderId);
        return value;
      });
      validateSnapshot(snapshot,item,folderId);
      const rejection = await mutate(`negative-publish:${index}`,() => ({operationId:uuid7(),documentId:item.documentId,documentVersionId:item.documentVersionId,expectedRevision:snapshot.detail.revision}),async request => {
        try {
          await probe.publish(request.documentId,request.documentVersionId,request.expectedRevision,request.operationId);
        } catch (error) {
          const diagnostic = sanitizeFailureDiagnostic(error?.diagnostic);
          if (error?.status !== 422 || !isDeepStrictEqual(diagnostic,EXPECTED_FAILURE)) fail('negative-publication-rejection-mismatch',diagnostic);
          // Persist this known response before running supplemental diagnostics.
          // Throwing it out of perform would leave an unknown pending mutation.
          return {outcome:'rejected-unsupported',failureDiagnostic:diagnostic};
        }
        // A returned success is known, even though it violates this test.
        // Keep it durable so a later run cannot repeat the publication.
        return {outcome:'unexpected-published'};
      });
      if (isDeepStrictEqual(rejection,{outcome:'unexpected-published'})) fail('negative-publication-unexpected-success');
      if (!isDeepStrictEqual(rejection,{outcome:'rejected-unsupported',failureDiagnostic:EXPECTED_FAILURE})) fail('negative-recorded-rejection-invalid');
      const retained = await guarded(checkpoint,() => probe.snapshot(item.documentId,{purpose:'authoring'}));
      if (!isDeepStrictEqual(retained,snapshot)) fail('negative-rejection-state-changed');
      validateSnapshot(retained,item,folderId);
      const inspectionDiagnostic = await missingInspection({diagnosePublication,checkpoint},item.fileId);
      const worker = await workerRejection({diagnoseWorker,checkpoint},item);
      documents.push({...item,snapshot,failureDiagnostic:rejection.failureDiagnostic,inspectionDiagnostic,...worker});
    }
    const documentIds = documents.map(item => item.documentId);
    if (new Set(documentIds).size !== documents.length || new Set(documents.map(item => item.documentVersionId)).size !== documents.length || new Set(documents.map(item => item.fileId)).size !== documents.length) fail('negative-created-identity-invalid');
    await verifyMembership(probe,checkpoint,folderId,documentIds);
    return {status:'AWAITING_RESTART',folderId,documentIds,documents,counts:counts(documents.length),contentQualityClaim:false};
  } catch (error) {throw sanitizedError(error);}
}

/** The caller proves same dataset and replaced HTTP processes before this read. */
export async function verifyNegativeRetained({probe, evidence, checkpoint, diagnosePublication}) {
  try {
    if (evidence?.status === 'NOT_RUN' && evidence.documents?.length === 0 && evidence.documentIds?.length === 0) return {status:'NOT_RUN',verifiedDocuments:0,snapshotCount:0,confirmedPublishedDocuments:0};
    if (evidence?.status !== 'AWAITING_RESTART' || !Array.isArray(evidence.documents) || evidence.documents.length < 1 || evidence.documents.length > 20
      || !isDeepStrictEqual(evidence.documentIds,evidence.documents.map(item => item.documentId)) || !isDeepStrictEqual(evidence.counts,counts(evidence.documents.length))) fail('negative-retention-evidence-invalid');
    if (typeof diagnosePublication !== 'function') fail('negative-diagnostics-required');
    await guarded(checkpoint,() => probe.verifySessions());
    await verifyMembership(probe,checkpoint,evidence.folderId,evidence.documentIds);
    for (const item of evidence.documents) {
      const snapshot = await guarded(checkpoint,() => probe.snapshot(item.documentId,{purpose:'authoring'}));
      if (!isDeepStrictEqual(snapshot,item.snapshot)) fail('negative-retained-state-changed');
      validateSnapshot(snapshot,item,evidence.folderId);
      await missingInspection({diagnosePublication,checkpoint},item.fileId);
    }
    return {status:'SUCCEEDED',verifiedDocuments:evidence.documents.length,snapshotCount:evidence.documents.length,confirmedPublishedDocuments:0};
  } catch (error) {throw sanitizedError(error);}
}
