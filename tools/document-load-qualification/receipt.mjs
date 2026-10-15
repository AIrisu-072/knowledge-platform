import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';

export const MAX_RECEIPT_BYTES = 1024 * 1024;
const METRICS = ['totalElapsedMs', 'peakRssBytes', 'diskGrowthBytes', 'storageAllocatedGrowthBytes', 'databaseAllocatedGrowthBytes', 'databaseGrowthBytes', 'storageGrowthBytes'];
const COUNTS = ['targetDocuments', 'confirmedCreatedDocuments', 'confirmedPublishedDocuments'];
const NEGATIVE_COUNTS = ['targetDocuments', 'confirmedCreatedDocuments', 'confirmedHttp422Responses', 'confirmedRejectedDocuments', 'confirmedPublishedDocuments'];
const IDENTITY = ['sourceHead', 'databaseIdentitySha256', 'storageIdentitySha256'];
const RECEIPT_KEYS = ['schemaVersion', 'evidenceClass', 'status', 'runId', 'stage', 'documentCount', 'fingerprint', 'startedAt', 'finishedAt', 'metricQualification', 'metrics', 'counts', 'evidence', 'restart', 'negativeCorpus', 'productionSloClaim', 'qualityClaim'];
const invalid = () => { throw Error('Invalid qualification receipt'); };
const requireValue = condition => { if (!condition) invalid(); };
const sha = (value, length = 64) => typeof value === 'string' && value.length === length && /^[a-f0-9]+$/.test(value);
const uuid = value => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[47][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(value);
const integer = value => Number.isSafeInteger(value) && value >= 0;
const elapsed = value => typeof value === 'number' && Number.isFinite(value) && value > 0 && value <= Number.MAX_SAFE_INTEGER;
const iso = value => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value)
  && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;

function object(value) {
  requireValue(value !== null && typeof value === 'object' && !Array.isArray(value)
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
function select(value, keys) {
  return Object.fromEntries(keys.map(key => [key, read(value, key)]));
}
function array(value, length, predicate) {
  requireValue(Array.isArray(value) && Object.getPrototypeOf(value) === Array.prototype && value.length === length);
  // Custom properties, accessors, sparse entries and toJSON are never exported.
  const keys = Reflect.ownKeys(value);
  requireValue(keys.length === length + 1 && keys.every(key => key === 'length' || /^(0|[1-9][0-9]*)$/.test(String(key))));
  for (let i = 0; i < length; i++) {
    const item = Object.getOwnPropertyDescriptor(value, String(i));
    requireValue(item && Object.hasOwn(item, 'value') && item.enumerable && predicate(item.value));
  }
}
function ids(value, length) {
  array(value, length, uuid);
  requireValue(new Set(value).size === length);
}
function pids(value) {
  array(value, 2, item => integer(item) && item > 0);
  requireValue(value[0] !== value[1]);
}
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}
const hash = value => createHash('sha256').update(canonical(value)).digest('hex');

function validateReceipt(receipt, stage = 'small', documentCount = 2) {
  exact(receipt, RECEIPT_KEYS);
  requireValue(receipt.schemaVersion === 1 && receipt.evidenceClass === 'owned-real-process' && receipt.status === 'SUCCEEDED'
    && receipt.stage === stage && receipt.documentCount === documentCount && uuid(receipt.runId)
    && receipt.metricQualification === 'complete-stage' && receipt.productionSloClaim === false && receipt.qualityClaim === false);
  exact(receipt.fingerprint, ['code', 'corpus', 'runtime']);
  requireValue(sha(receipt.fingerprint.code, 40) && sha(receipt.fingerprint.corpus) && sha(receipt.fingerprint.runtime));
  requireValue(iso(receipt.startedAt) && iso(receipt.finishedAt) && Date.parse(receipt.finishedAt) > Date.parse(receipt.startedAt));
  exact(receipt.metrics, METRICS);
  requireValue(elapsed(receipt.metrics.totalElapsedMs) && receipt.metrics.peakRssBytes > 0);
  for (const key of METRICS.slice(1)) requireValue(integer(receipt.metrics[key]));
  requireValue(receipt.metrics.diskGrowthBytes === receipt.metrics.storageAllocatedGrowthBytes + receipt.metrics.databaseAllocatedGrowthBytes
    && receipt.metrics.storageAllocatedGrowthBytes >= receipt.metrics.storageGrowthBytes
    && receipt.metrics.databaseAllocatedGrowthBytes >= receipt.metrics.databaseGrowthBytes);
  exact(receipt.counts, COUNTS);
  requireValue(COUNTS.every(key => receipt.counts[key] === documentCount));
  exact(receipt.evidence, ['documentIds']);
  ids(receipt.evidence.documentIds, documentCount);
  exact(receipt.restart, ['identityRetained', 'processesReplaced', 'before', 'after', 'processes']);
  requireValue(receipt.restart.identityRetained === true && receipt.restart.processesReplaced === true);
  for (const identity of [receipt.restart.before, receipt.restart.after]) {
    exact(identity, IDENTITY);
    requireValue(identity.sourceHead === receipt.fingerprint.code && sha(identity.databaseIdentitySha256) && sha(identity.storageIdentitySha256));
  }
  requireValue(isDeepStrictEqual(receipt.restart.before, receipt.restart.after));
  exact(receipt.restart.processes, ['before', 'after']);
  pids(receipt.restart.processes.before); pids(receipt.restart.processes.after);
  requireValue(receipt.restart.processes.before.every((pid, i) => pid !== receipt.restart.processes.after[i]));
  exact(receipt.negativeCorpus, ['status', 'documentIds', 'counts', 'diagnostic']);
  requireValue(receipt.negativeCorpus.status === 'SUCCEEDED');
  ids(receipt.negativeCorpus.documentIds, 1);
  requireValue(!receipt.evidence.documentIds.includes(receipt.negativeCorpus.documentIds[0]));
  exact(receipt.negativeCorpus.counts, NEGATIVE_COUNTS);
  requireValue(NEGATIVE_COUNTS.every(key => receipt.negativeCorpus.counts[key] === (key === 'confirmedPublishedDocuments' ? 0 : 1)));
  exact(receipt.negativeCorpus.diagnostic, ['operation', 'httpStatus', 'problemCode', 'workerFailureCode']);
  const diagnostic = receipt.negativeCorpus.diagnostic;
  requireValue(diagnostic.operation === 'publish' && diagnostic.httpStatus === 422 && diagnostic.problemCode === 'BUSINESS_RULE_REJECTED'
    && diagnostic.workerFailureCode === 'unsupported_semantic_construct');
}

/**
 * Pure, explicit projection of the approved TWO-positive/ONE-negative small run.
 * Never serialize or spread an internal report: it contains private snapshots,
 * observations, paths, ports and arbitrary diagnostics. No filesystem or network
 * operations occur here. Unknown source fields are ignored, not carried through.
 * Successful projection establishes a schema, not that a real run occurred.
 */
export function projectQualificationReceipt(report) {
  return projectStageReceipt(report, 'small', 2);
}

function projectStageReceipt(report, stage, documentCount) {
  try {
    const restart = read(report, 'restart');
    const before = read(restart, 'before'), after = read(restart, 'after');
    requireValue(read(before, 'runId') === read(report, 'runId') && read(after, 'runId') === read(report, 'runId') && isDeepStrictEqual(before, after));
    const evidence = read(report, 'evidence');
    const documentIds = read(evidence, 'documentIds'); ids(documentIds, documentCount);
    const processes = read(restart, 'processes');
    const oldPids = read(processes, 'before'), newPids = read(processes, 'after'); pids(oldPids); pids(newPids);
    const negative = read(report, 'negativeCorpus');
    const negativeIds = read(negative, 'documentIds'); ids(negativeIds, 1);
    const documents = read(negative, 'documents'); array(documents, 1, item => item !== null && typeof item === 'object');
    requireValue(read(documents[0], 'documentId') === negativeIds[0] && read(negative, 'contentQualityClaim') === false);
    const failure = read(documents[0], 'failureDiagnostic'), worker = read(documents[0], 'workerDiagnostic');
    requireValue(read(worker, 'status') === 'worker-failure' && read(worker, 'qualification') === false);
    const receipt = {
      ...select(report, ['schemaVersion', 'evidenceClass', 'status', 'runId', 'stage', 'documentCount']),
      fingerprint: select(read(report, 'fingerprint'), ['code', 'corpus', 'runtime']),
      ...select(report, ['startedAt', 'finishedAt', 'metricQualification']),
      metrics: select(read(report, 'metrics'), METRICS),
      counts: select(read(report, 'counts'), COUNTS),
      evidence: { documentIds: [...documentIds] },
      restart: {
        ...select(restart, ['identityRetained', 'processesReplaced']),
        before: select(before, IDENTITY), after: select(after, IDENTITY),
        processes: { before: [...oldPids], after: [...newPids] },
      },
      negativeCorpus: {
        status: read(negative, 'status'), documentIds: [...negativeIds], counts: select(read(negative, 'counts'), NEGATIVE_COUNTS),
        diagnostic: { ...select(failure, ['operation', 'httpStatus', 'problemCode']), workerFailureCode: read(worker, 'failureCode') },
      },
      ...select(report, ['productionSloClaim', 'qualityClaim']),
    };
    validateReceipt(receipt, stage, documentCount);
    const envelope = { schemaVersion: 1, sha256: hash(receipt), receipt };
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    return envelope;
  } catch { invalid(); }
}

/**
 * Verify schema, exact identifiers and integrity ONLY. sha256 is SHA-256 of the
 * UTF-8 receipt JSON with recursively sorted object keys and no whitespace.
 * It is an unkeyed checksum, NOT a signature or trusted proof of execution.
 * expected MUST come from independently authenticated, authorized artifact/run
 * provenance, never from this envelope alone. All five bindings are required.
 * This module cannot authenticate GitHub provenance, authorize a later stage,
 * or restore a report. A future consumer must independently verify the owned
 * repository/workflow/run/attempt/source/artifact origin before admitStage.
 */
export function verifyQualificationReceipt(input, expected) {
  try {
    let envelope = input;
    if (Buffer.isBuffer(input)) {
      requireValue(input.length <= MAX_RECEIPT_BYTES);
      envelope = input.toString('utf8');
    }
    if (typeof envelope === 'string') {
      requireValue(Buffer.byteLength(envelope) <= MAX_RECEIPT_BYTES);
      envelope = JSON.parse(envelope);
    }
    exact(expected, ['code', 'corpus', 'runtime', 'runId', 'sha256']);
    requireValue(sha(expected.code, 40) && sha(expected.corpus) && sha(expected.runtime) && uuid(expected.runId) && sha(expected.sha256));
    exact(envelope, ['schemaVersion', 'sha256', 'receipt']);
    requireValue(envelope.schemaVersion === 1 && sha(envelope.sha256));
    validateReceipt(envelope.receipt);
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    requireValue(envelope.sha256 === hash(envelope.receipt) && envelope.sha256 === expected.sha256
      && envelope.receipt.runId === expected.runId
      && ['code', 'corpus', 'runtime'].every(key => envelope.receipt.fingerprint[key] === expected[key]));
    return { receipt: JSON.parse(JSON.stringify(envelope.receipt)), integrityVerified: true, authenticityVerified: false };
  } catch { invalid(); }
}

function validateThousandReceipt(receipt) {
  exact(receipt, ['schemaVersion', 'evidenceClass', 'status', 'runId', 'fingerprint', 'stages', 'productionSloClaim', 'qualityClaim']);
  requireValue(receipt.schemaVersion === 1 && receipt.evidenceClass === 'owned-real-process' && receipt.status === 'SUCCEEDED'
    && uuid(receipt.runId) && receipt.productionSloClaim === false && receipt.qualityClaim === false);
  exact(receipt.fingerprint, ['code', 'corpus', 'runtime']);
  requireValue(sha(receipt.fingerprint.code, 40) && sha(receipt.fingerprint.corpus) && sha(receipt.fingerprint.runtime));
  array(receipt.stages, 2, () => true);
  const [small, thousand] = receipt.stages;
  validateReceipt(small);
  validateReceipt(thousand, 1000, 1000);
  for (const stage of receipt.stages) {
    requireValue(stage.runId === receipt.runId && isDeepStrictEqual(stage.fingerprint, receipt.fingerprint));
  }
  requireValue(Date.parse(small.finishedAt) <= Date.parse(thousand.startedAt)
    && isDeepStrictEqual(small.restart.after, thousand.restart.before)
    && isDeepStrictEqual(small.restart.processes.after, thousand.restart.processes.before));
  const documentIds = receipt.stages.flatMap(stage => [...stage.evidence.documentIds, ...stage.negativeCorpus.documentIds]);
  requireValue(new Set(documentIds).size === documentIds.length);
}

/**
 * Export only the fresh in-memory small -> 1000 full-report chain. The exact
 * previousReport object is required to prevent accidental loaded-history use;
 * this is an API guard, not proof of execution or provenance. Full private
 * restart datasets are compared before their approved fields are projected.
 * The resulting bounded envelope contains no source report, journal or path.
 */
export function projectThousandQualificationReceipt(chain) {
  try {
    exact(chain, ['small', 'thousand']);
    const small = read(chain, 'small'), thousand = read(chain, 'thousand');
    requireValue(read(thousand, 'previousReport') === small);
    const smallAfter = read(read(small, 'restart'), 'after');
    const thousandBefore = read(read(thousand, 'restart'), 'before');
    requireValue(isDeepStrictEqual(smallAfter, thousandBefore));
    const stages = [projectStageReceipt(small, 'small', 2).receipt, projectStageReceipt(thousand, 1000, 1000).receipt];
    const receipt = {
      ...select(stages[0], ['schemaVersion', 'evidenceClass', 'status', 'runId']),
      fingerprint: select(stages[0].fingerprint, ['code', 'corpus', 'runtime']),
      stages, productionSloClaim: false, qualityClaim: false,
    };
    validateThousandReceipt(receipt);
    const envelope = { schemaVersion: 1, sha256: hash(receipt), receipt };
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    return envelope;
  } catch { invalid(); }
}

/**
 * Verify the closed chain schema and canonical SHA-256 integrity only. All five
 * expected bindings must come from independently trusted owned GitHub workflow,
 * run/attempt, checkout and artifact provenance, never this envelope alone.
 * No historical report is reconstructed and no stage is admitted by this API.
 */
export function verifyThousandQualificationReceipt(input, expected) {
  try {
    let envelope = input;
    if (Buffer.isBuffer(input)) {
      requireValue(input.length <= MAX_RECEIPT_BYTES);
      envelope = input.toString('utf8');
    }
    if (typeof envelope === 'string') {
      requireValue(Buffer.byteLength(envelope) <= MAX_RECEIPT_BYTES);
      envelope = JSON.parse(envelope);
    }
    exact(expected, ['code', 'corpus', 'runtime', 'runId', 'sha256']);
    requireValue(sha(expected.code, 40) && sha(expected.corpus) && sha(expected.runtime) && uuid(expected.runId) && sha(expected.sha256));
    exact(envelope, ['schemaVersion', 'sha256', 'receipt']);
    requireValue(envelope.schemaVersion === 1 && sha(envelope.sha256));
    validateThousandReceipt(envelope.receipt);
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    requireValue(envelope.sha256 === hash(envelope.receipt) && envelope.sha256 === expected.sha256
      && envelope.receipt.runId === expected.runId
      && ['code', 'corpus', 'runtime'].every(key => envelope.receipt.fingerprint[key] === expected[key]));
    return { receipt: JSON.parse(JSON.stringify(envelope.receipt)), integrityVerified: true, authenticityVerified: false };
  } catch { invalid(); }
}

function validateTenThousandReceipt(receipt) {
  const headerKeys = ['schemaVersion', 'evidenceClass', 'status', 'runId', 'fingerprint', 'productionSloClaim', 'qualityClaim'];
  exact(receipt, [...headerKeys, 'stages']);
  array(receipt.stages, 3, () => true);
  const [small, thousand, tenThousand] = receipt.stages;
  validateThousandReceipt({ ...select(receipt, headerKeys), stages: [small, thousand] });
  validateReceipt(tenThousand, 10000, 10000);
  requireValue(tenThousand.runId === receipt.runId && isDeepStrictEqual(tenThousand.fingerprint, receipt.fingerprint)
    && Date.parse(thousand.finishedAt) <= Date.parse(tenThousand.startedAt)
    && isDeepStrictEqual(thousand.restart.after, tenThousand.restart.before)
    && isDeepStrictEqual(thousand.restart.processes.after, tenThousand.restart.processes.before));
  const documentIds = receipt.stages.flatMap(stage => [...stage.evidence.documentIds, ...stage.negativeCorpus.documentIds]);
  requireValue(new Set(documentIds).size === documentIds.length);
}

/**
 * Project exactly the fresh in-memory small -> 1000 -> 10000 full-report chain.
 * Both previousReport references and both full private restart identities must
 * match before the allowlisted projection. Retain all positive/negative UUIDs
 * within the existing one-MiB bound; no compact or 100000 schema is accepted.
 * These guards establish continuity of the supplied data, not provenance.
 */
export function projectTenThousandQualificationReceipt(chain) {
  try {
    exact(chain, ['small', 'thousand', 'tenThousand']);
    const small = read(chain, 'small'), thousand = read(chain, 'thousand'), tenThousand = read(chain, 'tenThousand');
    requireValue(read(tenThousand, 'previousReport') === thousand);
    const thousandAfter = read(read(thousand, 'restart'), 'after');
    const tenThousandBefore = read(read(tenThousand, 'restart'), 'before');
    requireValue(isDeepStrictEqual(thousandAfter, tenThousandBefore));
    const prefix = projectThousandQualificationReceipt({ small, thousand }).receipt;
    const receipt = { ...prefix, stages: [...prefix.stages, projectStageReceipt(tenThousand, 10000, 10000).receipt] };
    validateTenThousandReceipt(receipt);
    const envelope = { schemaVersion: 1, sha256: hash(receipt), receipt };
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    return envelope;
  } catch { invalid(); }
}

/**
 * Verify the closed three-stage schema and canonical SHA-256 integrity only.
 * All five expected bindings require independently trusted owned workflow,
 * run/attempt, checkout and artifact provenance. This does not authenticate
 * execution, reconstruct an admission report or authorize the 100000 stage.
 */
export function verifyTenThousandQualificationReceipt(input, expected) {
  try {
    let envelope = input;
    if (Buffer.isBuffer(input)) {
      requireValue(input.length <= MAX_RECEIPT_BYTES);
      envelope = input.toString('utf8');
    }
    if (typeof envelope === 'string') {
      requireValue(Buffer.byteLength(envelope) <= MAX_RECEIPT_BYTES);
      envelope = JSON.parse(envelope);
    }
    exact(expected, ['code', 'corpus', 'runtime', 'runId', 'sha256']);
    requireValue(sha(expected.code, 40) && sha(expected.corpus) && sha(expected.runtime) && uuid(expected.runId) && sha(expected.sha256));
    exact(envelope, ['schemaVersion', 'sha256', 'receipt']);
    requireValue(envelope.schemaVersion === 1 && sha(envelope.sha256));
    validateTenThousandReceipt(envelope.receipt);
    requireValue(Buffer.byteLength(JSON.stringify(envelope)) <= MAX_RECEIPT_BYTES);
    requireValue(envelope.sha256 === hash(envelope.receipt) && envelope.sha256 === expected.sha256
      && envelope.receipt.runId === expected.runId
      && ['code', 'corpus', 'runtime'].every(key => envelope.receipt.fingerprint[key] === expected[key]));
    return { receipt: JSON.parse(JSON.stringify(envelope.receipt)), integrityVerified: true, authenticityVerified: false };
  } catch { invalid(); }
}
