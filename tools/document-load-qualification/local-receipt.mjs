import { createHash } from 'node:crypto';
import { isDeepStrictEqual, types } from 'node:util';

export const MAX_LOCAL_SCALE_RECEIPT_BYTES = 1024 * 1024;
const STAGES = [['small', 2, 'small'], [1000, 1000, 'thousand'], [10000, 10000, 'tenThousand'], [100000, 100000, 'hundredThousand']];
const FINGERPRINT = ['code', 'corpus', 'runtime'];
const METRICS = ['totalElapsedMs', 'peakRssBytes', 'diskGrowthBytes', 'storageAllocatedGrowthBytes', 'databaseAllocatedGrowthBytes', 'databaseGrowthBytes', 'storageGrowthBytes'];
const COUNTS = ['targetDocuments', 'confirmedCreatedDocuments', 'confirmedPublishedDocuments'];
const NEGATIVE_COUNTS = ['targetDocuments', 'confirmedCreatedDocuments', 'confirmedHttp422Responses', 'confirmedRejectedDocuments', 'confirmedPublishedDocuments'];
const IDENTITY = ['sourceHead', 'databaseIdentitySha256', 'storageIdentitySha256', 'databaseStorageMode', 'databaseStorageIdentitySha256'];
const HEADER = ['schemaVersion', 'evidenceClass', 'status', 'runId'];
const STAGE_KEYS = [...HEADER, 'stage', 'documentCount', 'fingerprint', 'startedAt', 'finishedAt', 'metricQualification', 'metrics', 'counts', 'evidence', 'restart', 'negativeCorpus', 'productionSloClaim', 'qualityClaim'];
const invalid = () => { throw Error('Invalid local scale receipt'); };
const requireValue = condition => { if (!condition) invalid(); };
const sha = (value, length = 64) => typeof value === 'string' && value.length === length && /^[a-f0-9]+$/.test(value);
const uuid = value => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[47][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i.test(value);
const canonicalUuid = value => uuid(value) && value === value.toLowerCase();
const integer = value => Number.isSafeInteger(value) && value >= 0;
const elapsed = value => typeof value === 'number' && Number.isFinite(value) && value > 0 && value <= Number.MAX_SAFE_INTEGER;
const iso = value => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value)
  && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;

function object(value) {
  requireValue(value !== null && typeof value === 'object' && !types.isProxy(value) && !Array.isArray(value)
    && [Object.prototype, null].includes(Object.getPrototypeOf(value)));
}
function read(value, key) {
  object(value);
  const descriptor = Object.getOwnPropertyDescriptor(value, key);
  requireValue(descriptor && Object.hasOwn(descriptor, 'value') && descriptor.enumerable);
  return descriptor.value;
}
function exact(value, keys) {
  object(value);
  const actual = Reflect.ownKeys(value);
  requireValue(actual.length === keys.length && actual.every(key => keys.includes(key)));
  for (const key of keys) read(value, key);
}
const select = (value, keys) => Object.fromEntries(keys.map(key => [key, read(value, key)]));
function array(value, length, predicate) {
  requireValue(Array.isArray(value) && !types.isProxy(value) && Object.getPrototypeOf(value) === Array.prototype && value.length === length);
  const keys = Reflect.ownKeys(value);
  requireValue(keys.length === length + 1 && keys.every(key => key === 'length' || typeof key === 'string' && /^(0|[1-9][0-9]*)$/.test(key)));
  for (let index = 0; index < length; index++) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    requireValue(descriptor && Object.hasOwn(descriptor, 'value') && descriptor.enumerable && predicate(descriptor.value));
  }
}
function pids(value) {
  array(value, 2, item => integer(item) && item > 0);
  requireValue(value[0] !== value[1]);
}
function fingerprint(value) {
  exact(value, FINGERPRINT);
  requireValue(sha(value.code, 40) && sha(value.corpus) && sha(value.runtime));
}
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}
const hash = value => createHash('sha256').update(canonical(value)).digest('hex');

function validateIdentity(value, code) {
  exact(value, IDENTITY);
  requireValue(value.sourceHead === code && sha(value.databaseIdentitySha256) && sha(value.storageIdentitySha256)
    && value.databaseStorageMode === 'owned-ext4' && sha(value.databaseStorageIdentitySha256));
}
function validateStage(value, stage, count) {
  exact(value, STAGE_KEYS);
  requireValue(value.schemaVersion === 1 && value.evidenceClass === 'owned-real-process' && value.status === 'SUCCEEDED'
    && canonicalUuid(value.runId) && value.stage === stage && value.documentCount === count
    && value.metricQualification === 'complete-stage' && value.productionSloClaim === false && value.qualityClaim === false);
  fingerprint(value.fingerprint);
  requireValue(iso(value.startedAt) && iso(value.finishedAt) && Date.parse(value.finishedAt) > Date.parse(value.startedAt));
  exact(value.metrics, METRICS);
  requireValue(elapsed(value.metrics.totalElapsedMs) && value.metrics.peakRssBytes > 0);
  for (const key of METRICS.slice(1)) requireValue(integer(value.metrics[key]));
  requireValue(value.metrics.diskGrowthBytes === value.metrics.storageAllocatedGrowthBytes + value.metrics.databaseAllocatedGrowthBytes
    && value.metrics.storageAllocatedGrowthBytes >= value.metrics.storageGrowthBytes
    && value.metrics.databaseAllocatedGrowthBytes >= value.metrics.databaseGrowthBytes);
  exact(value.counts, COUNTS);
  requireValue(COUNTS.every(key => value.counts[key] === count));
  exact(value.evidence, ['positiveDocumentCount', 'positiveDocumentIdsSha256', 'detailedSampleCount', 'allOriginalHashesVerified']);
  requireValue(value.evidence.positiveDocumentCount === count && sha(value.evidence.positiveDocumentIdsSha256)
    && value.evidence.detailedSampleCount === (count === 2 ? 2 : 3) && value.evidence.allOriginalHashesVerified === false);
  exact(value.restart, ['identityRetained', 'processesReplaced', 'before', 'after', 'processes']);
  requireValue(value.restart.identityRetained === true && value.restart.processesReplaced === true);
  validateIdentity(value.restart.before, value.fingerprint.code);
  validateIdentity(value.restart.after, value.fingerprint.code);
  requireValue(isDeepStrictEqual(value.restart.before, value.restart.after));
  exact(value.restart.processes, ['before', 'after']);
  pids(value.restart.processes.before); pids(value.restart.processes.after);
  requireValue(value.restart.processes.before.every(pid => !value.restart.processes.after.includes(pid)));
  exact(value.negativeCorpus, ['status', 'documentIds', 'counts', 'diagnostic']);
  requireValue(value.negativeCorpus.status === 'SUCCEEDED');
  array(value.negativeCorpus.documentIds, 1, canonicalUuid);
  exact(value.negativeCorpus.counts, NEGATIVE_COUNTS);
  requireValue(NEGATIVE_COUNTS.every(key => value.negativeCorpus.counts[key] === (key === 'confirmedPublishedDocuments' ? 0 : 1)));
  exact(value.negativeCorpus.diagnostic, ['operation', 'httpStatus', 'problemCode', 'workerFailureCode']);
  const diagnostic = value.negativeCorpus.diagnostic;
  requireValue(diagnostic.operation === 'publish' && diagnostic.httpStatus === 422
    && diagnostic.problemCode === 'BUSINESS_RULE_REJECTED' && diagnostic.workerFailureCode === 'unsupported_semantic_construct');
}
function validateCleanup(value, finalPids) {
  exact(value, ['ownedCleanupSucceeded', 'records']);
  requireValue(value.ownedCleanupSucceeded === true);
  array(value.records, 4, () => true);
  for (let index = 0; index < 2; index++) {
    exact(value.records[index], ['pid', 'result']);
    requireValue(value.records[index].pid === finalPids[index] && value.records[index].result === 'gracefully-stopped');
  }
  for (let index = 2; index < 4; index++) exact(value.records[index], ['resource', 'result']);
  requireValue(value.records[2].resource === 'owned-database-proxy' && value.records[2].result === 'closed'
    && value.records[3].resource === 'owned-postgres' && value.records[3].result === 'removed');
}
function validateReceipt(value) {
  exact(value, [...HEADER, 'receiptKind', 'fingerprint', 'stages', 'cleanup', 'productionSloClaim', 'qualityClaim']);
  requireValue(value.schemaVersion === 1 && value.receiptKind === 'local-scale-compact'
    && value.evidenceClass === 'owned-real-process' && value.status === 'SUCCEEDED' && canonicalUuid(value.runId)
    && value.productionSloClaim === false && value.qualityClaim === false);
  fingerprint(value.fingerprint);
  array(value.stages, 4, () => true);
  const negativeIds = new Set();
  for (const [index, [stage, count]] of STAGES.entries()) {
    const current = value.stages[index];
    validateStage(current, stage, count);
    requireValue(current.runId === value.runId && isDeepStrictEqual(current.fingerprint, value.fingerprint));
    const negativeId = current.negativeCorpus.documentIds[0];
    requireValue(!negativeIds.has(negativeId)); negativeIds.add(negativeId);
    if (index > 0) {
      const previous = value.stages[index - 1];
      requireValue(Date.parse(previous.finishedAt) <= Date.parse(current.startedAt)
        && isDeepStrictEqual(previous.restart.after, current.restart.before)
        && isDeepStrictEqual(previous.restart.processes.after, current.restart.processes.before));
    }
  }
  validateCleanup(value.cleanup, value.stages[3].restart.processes.after);
}

// Validate the whole fixed private dataset before dropping ports and fixture
// identity from the public receipt. No getters or arbitrary source extras run.
function privateIdentity(value, runId, code) {
  exact(value, ['runId', ...IDENTITY, 'fixtureHash', 'ports']);
  requireValue(read(value, 'runId') === runId && sha(read(value, 'fixtureHash')));
  const ports = read(value, 'ports');
  const portNames = ['human', 'agent', 'postgres', 'proxy'];
  exact(ports, portNames);
  requireValue(portNames.every(name => integer(ports[name]) && ports[name] > 0 && ports[name] <= 65535)
    && new Set(portNames.map(name => ports[name])).size === 4);
  validateIdentity(select(value, IDENTITY), code);
  return value;
}

function projectStage(report, stage, count, allIds) {
  const header = select(report, HEADER), sourceFingerprint = select(read(report, 'fingerprint'), FINGERPRINT);
  const restart = read(report, 'restart');
  const before = privateIdentity(read(restart, 'before'), header.runId, sourceFingerprint.code);
  const after = privateIdentity(read(restart, 'after'), header.runId, sourceFingerprint.code);
  requireValue(isDeepStrictEqual(before, after));
  const processes = read(restart, 'processes');
  const oldPids = read(processes, 'before'), newPids = read(processes, 'after'); pids(oldPids); pids(newPids);
  const evidence = read(report, 'evidence'), documentIds = read(evidence, 'documentIds');
  array(documentIds, count, uuid);
  const normalizedIds = documentIds.map(id => id.toLowerCase());
  for (const id of normalizedIds) { requireValue(!allIds.has(id)); allIds.add(id); }
  const sampledIds = read(evidence, 'sampledDocumentIds');
  const sampleCount = count === 2 ? 2 : 3;
  array(sampledIds, sampleCount, uuid);
  const expectedSamples = count === 2 ? documentIds : [documentIds[0], documentIds[Math.floor(count / 2)], documentIds.at(-1)];
  requireValue(sampledIds.every((id, index) => id === expectedSamples[index])
    && read(evidence, 'status') === 'AWAITING_RESTART' && read(evidence, 'contentQualityClaim') === false);
  const snapshots = read(evidence, 'snapshots'); exact(snapshots, sampledIds);
  for (const id of sampledIds) object(read(snapshots, id));
  const negative = read(report, 'negativeCorpus'), negativeIds = read(negative, 'documentIds');
  array(negativeIds, 1, uuid);
  const negativeId = negativeIds[0].toLowerCase();
  requireValue(!allIds.has(negativeId)); allIds.add(negativeId);
  const documents = read(negative, 'documents'); array(documents, 1, item => item !== null && typeof item === 'object');
  requireValue(read(documents[0], 'documentId') === negativeIds[0] && read(negative, 'contentQualityClaim') === false);
  const failure = read(documents[0], 'failureDiagnostic'), worker = read(documents[0], 'workerDiagnostic');
  requireValue(read(worker, 'status') === 'worker-failure' && read(worker, 'qualification') === false);
  const result = {
    ...header, ...select(report, ['stage', 'documentCount']), fingerprint: sourceFingerprint,
    ...select(report, ['startedAt', 'finishedAt', 'metricQualification']),
    metrics: select(read(report, 'metrics'), METRICS), counts: select(read(report, 'counts'), COUNTS),
    evidence: { positiveDocumentCount: count,
      positiveDocumentIdsSha256: createHash('sha256').update(normalizedIds.sort().join('\n') + '\n').digest('hex'),
      detailedSampleCount: sampleCount, allOriginalHashesVerified: false },
    restart: { ...select(restart, ['identityRetained', 'processesReplaced']), before: select(before, IDENTITY), after: select(after, IDENTITY),
      processes: { before: [...oldPids], after: [...newPids] } },
    negativeCorpus: { status: read(negative, 'status'), documentIds: [negativeId], counts: select(read(negative, 'counts'), NEGATIVE_COUNTS),
      diagnostic: { ...select(failure, ['operation', 'httpStatus', 'problemCode']), workerFailureCode: read(worker, 'failureCode') } },
    ...select(report, ['productionSloClaim', 'qualityClaim']),
  };
  validateStage(result, stage, count);
  return result;
}

/**
 * Project the full fresh small -> 1000 -> 10000 -> 100000 in-memory chain only
 * AFTER the caller has confirmed final acceptance and owned cleanup. It must
 * supply the actual four completed cleanup records, not a success flag alone.
 * Every positive/negative UUID is validated and checked for global uniqueness
 * before IDs are compacted. The digest is SHA-256 of sorted lowercase UUIDs,
 * separated by LF, including a final LF. Three large-stage detail snapshots
 * remain private; all-original hash verification is explicitly not claimed.
 * This pure projection performs no filesystem, network or admission operations.
 */
export function projectLocalScaleReceipt(chain) {
  try {
    exact(chain, [...STAGES.map(([, , key]) => key), 'cleanup']);
    const allIds = new Set(), stages = [];
    let previousReport;
    for (const [stage, count, key] of STAGES) {
      const report = read(chain, key);
      if (previousReport) {
        requireValue(read(report, 'previousReport') === previousReport);
        // Both complete datasets have been validated before this comparison.
        privateIdentity(read(read(report, 'restart'), 'before'), read(report, 'runId'), read(read(report, 'fingerprint'), 'code'));
        requireValue(isDeepStrictEqual(read(read(previousReport, 'restart'), 'after'), read(read(report, 'restart'), 'before')));
      } else {
        object(report);
        requireValue(!Object.hasOwn(report, 'previousReport') || read(report, 'previousReport') === undefined);
      }
      stages.push(projectStage(report, stage, count, allIds));
      previousReport = report;
    }
    const cleanup = read(chain, 'cleanup');
    validateCleanup(cleanup, stages[3].restart.processes.after);
    const receipt = { ...select(stages[0], HEADER), receiptKind: 'local-scale-compact',
      fingerprint: select(stages[0].fingerprint, FINGERPRINT), stages,
      cleanup: { ownedCleanupSucceeded: true, records: cleanup.records.map((record, index) => select(record, index < 2 ? ['pid', 'result'] : ['resource', 'result'])) },
      productionSloClaim: false, qualityClaim: false };
    validateReceipt(receipt);
    const envelope = { schemaVersion: 1, sha256: hash(receipt), receipt };
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_LOCAL_SCALE_RECEIPT_BYTES);
    return envelope;
  } catch { invalid(); }
}

/**
 * Check the closed compact schema, continuity and canonical JSON checksum only.
 * SHA-256 is an unkeyed integrity checksum, never execution authentication.
 * The five expected bindings MUST come from independently trusted provenance.
 * Compact positive-ID digests cannot re-establish source-list uniqueness, actual
 * execution or downloaded original content. This verifier does not reconstruct
 * a full report and MUST NOT be used as input to admission or resume a stage.
 */
export function verifyLocalScaleReceipt(input, expected) {
  try {
    let envelope = input;
    if (Buffer.isBuffer(input)) {
      requireValue(input.length <= MAX_LOCAL_SCALE_RECEIPT_BYTES);
      envelope = input.toString('utf8');
    }
    if (typeof envelope === 'string') {
      requireValue(Buffer.byteLength(envelope) <= MAX_LOCAL_SCALE_RECEIPT_BYTES);
      envelope = JSON.parse(envelope);
    }
    exact(expected, [...FINGERPRINT, 'runId', 'sha256']);
    requireValue(sha(expected.code, 40) && sha(expected.corpus) && sha(expected.runtime) && canonicalUuid(expected.runId) && sha(expected.sha256));
    exact(envelope, ['schemaVersion', 'sha256', 'receipt']);
    requireValue(envelope.schemaVersion === 1 && sha(envelope.sha256));
    validateReceipt(envelope.receipt);
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_LOCAL_SCALE_RECEIPT_BYTES
      && envelope.sha256 === hash(envelope.receipt) && envelope.sha256 === expected.sha256
      && envelope.receipt.runId === expected.runId && FINGERPRINT.every(key => envelope.receipt.fingerprint[key] === expected[key]));
    return { receipt: JSON.parse(JSON.stringify(envelope.receipt)), integrityVerified: true, authenticityVerified: false };
  } catch { invalid(); }
}
