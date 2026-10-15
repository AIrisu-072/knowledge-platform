import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import * as api from '../receipt.mjs';

const runId = '12345678-1234-4abc-8abc-123456789abc';
const fingerprint = { code: 'a'.repeat(40), corpus: 'b'.repeat(64), runtime: 'c'.repeat(64) };
const uuid = n => `0198eada-1234-7000-8000-${n.toString(16).padStart(12, '0')}`;
const canonical = value => JSON.stringify(value, function (key, item) {
  return item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map(name => [name, item[name]])) : item;
});
const digest = value => createHash('sha256').update(canonical(value)).digest('hex');
const seal = receipt => ({ schemaVersion: 1, sha256: digest(receipt), receipt });
const expected = envelope => ({ ...fingerprint, runId, sha256: envelope.sha256 });
const invalid = /^Error: Invalid qualification receipt$/;

function stageReport(stage, documentCount, firstId, start, finish, beforePids, afterPids) {
  const identity = { runId, sourceHead: fingerprint.code, databaseIdentitySha256: 'd'.repeat(64), storageIdentitySha256: 'e'.repeat(64), fixtureHash: 'f'.repeat(64), ports: { human: 41001, agent: 41002, postgres: 41003, proxy: 41004 } };
  const documentIds = Array.from({ length: documentCount }, (_, i) => uuid(firstId + i));
  const negativeId = uuid(firstId + documentCount);
  return {
    schemaVersion: 1, evidenceClass: 'owned-real-process', status: 'SUCCEEDED', runId,
    stage, documentCount, fingerprint: { ...fingerprint }, startedAt: start, finishedAt: finish,
    metricQualification: 'complete-stage', productionSloClaim: false, qualityClaim: false,
    metrics: { totalElapsedMs: documentCount * 500.25, peakRssBytes: 4096, diskGrowthBytes: 30, storageAllocatedGrowthBytes: 10, databaseAllocatedGrowthBytes: 20, databaseGrowthBytes: 10, storageGrowthBytes: 5, rssMeasurement: 'PRIVATE_SENTINEL' },
    counts: { targetDocuments: documentCount, confirmedCreatedDocuments: documentCount, confirmedPublishedDocuments: documentCount },
    evidence: { status: 'AWAITING_RESTART', documentIds, snapshots: { private: 'PRIVATE_SENTINEL' } },
    restart: { identityRetained: true, processesReplaced: true, before: identity, after: structuredClone(identity), processes: { before: beforePids, after: afterPids } },
    negativeCorpus: { status: 'SUCCEEDED', contentQualityClaim: false, documentIds: [negativeId], counts: { targetDocuments: 1, confirmedCreatedDocuments: 1, confirmedHttp422Responses: 1, confirmedRejectedDocuments: 1, confirmedPublishedDocuments: 0 }, documents: [{ documentId: negativeId, failureDiagnostic: { operation: 'publish', httpStatus: 422, problemCode: 'BUSINESS_RULE_REJECTED' }, workerDiagnostic: { status: 'worker-failure', failureCode: 'unsupported_semantic_construct', qualification: false }, snapshot: 'PRIVATE_SENTINEL' }] },
    plan: { privatePath: '/private/PRIVATE_SENTINEL' }, observations: ['PRIVATE_SENTINEL'], timings: { PRIVATE_SENTINEL: {} }, credentials: 'PRIVATE_SENTINEL', rawPdf: Buffer.from('PRIVATE_SENTINEL'), environment: { PASSWORD: 'PRIVATE_SENTINEL' },
  };
}
function chain() {
  const small = stageReport('small', 2, 1, '2026-10-08T16:00:00.000Z', '2026-10-08T16:00:01.000Z', [100, 200], [101, 201]);
  const thousand = stageReport(1000, 1000, 4, '2026-10-08T16:00:02.000Z', '2026-10-08T16:09:02.000Z', [101, 201], [102, 202]);
  thousand.previousReport = small;
  return { small, thousand };
}
function projected(source = chain()) {
  assert.equal(typeof api.projectThousandQualificationReceipt, 'function');
  return api.projectThousandQualificationReceipt(source);
}

test('projects the full fresh small then 1000 chain into one deterministic bounded safe envelope', () => {
  const source = chain(), before = JSON.stringify(source);
  const envelope = projected(source);
  assert.equal(JSON.stringify(source), before);
  assert.deepEqual(envelope, projected());
  assert.deepEqual(Object.keys(envelope).sort(), ['receipt', 'schemaVersion', 'sha256']);
  assert.equal(envelope.schemaVersion, 1);
  assert.equal(envelope.sha256, digest(envelope.receipt));
  assert.deepEqual(Object.keys(envelope.receipt).sort(), ['evidenceClass', 'fingerprint', 'productionSloClaim', 'qualityClaim', 'runId', 'schemaVersion', 'stages', 'status']);
  assert.deepEqual(envelope.receipt.stages.map(stage => [stage.stage, stage.documentCount]), [['small', 2], [1000, 1000]]);
  assert.deepEqual(envelope.receipt.stages[0], api.projectQualificationReceipt(source.small).receipt);
  assert.deepEqual(envelope.receipt.stages[1].evidence.documentIds, source.thousand.evidence.documentIds);
  assert.equal(new Set(envelope.receipt.stages.flatMap(stage => [...stage.evidence.documentIds, ...stage.negativeCorpus.documentIds])).size, 1004);
  assert.equal(api.MAX_RECEIPT_BYTES, 1024 * 1024);
  assert.ok(Buffer.byteLength(JSON.stringify(envelope)) < api.MAX_RECEIPT_BYTES);
  for (const forbidden of ['PRIVATE_SENTINEL', 'ports', 'fixtureHash', 'snapshots', 'observations', 'timings', 'credentials', 'rawPdf', 'environment', 'previousReport', 'plan']) assert.ok(!JSON.stringify(envelope).includes(forbidden), forbidden);
});

test('chain projection ignores private report fields without traversing, stringifying or mutating them', () => {
  const source = chain();
  for (const report of [source.small, source.thousand]) {
    report.unknown = report;
    report.toJSON = () => { throw Error('PRIVATE_SENTINEL'); };
    Object.defineProperty(report, 'privateGetter', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } });
  }
  assert.deepEqual(projected(source), projected());
});

test('chain verifier binds all five trusted external values and proves integrity only', () => {
  assert.equal(typeof api.verifyThousandQualificationReceipt, 'function');
  const envelope = projected();
  for (const input of [envelope, JSON.stringify(envelope), Buffer.from(JSON.stringify(envelope))]) {
    const checked = api.verifyThousandQualificationReceipt(input, expected(envelope));
    assert.deepEqual(checked, { receipt: envelope.receipt, integrityVerified: true, authenticityVerified: false });
    assert.notEqual(checked.receipt, envelope.receipt);
  }
  for (const key of ['code', 'corpus', 'runtime', 'runId', 'sha256']) {
    const missing = expected(envelope); delete missing[key];
    assert.throws(() => api.verifyThousandQualificationReceipt(envelope, missing), invalid);
    assert.throws(() => api.verifyThousandQualificationReceipt(envelope, { ...expected(envelope), [key]: key === 'runId' ? uuid(9999) : 'f'.repeat(key === 'code' ? 40 : 64) }), invalid);
  }
  assert.throws(() => api.verifyThousandQualificationReceipt(envelope), invalid);
  assert.throws(() => api.verifyThousandQualificationReceipt(envelope, { ...expected(envelope), source: 'PRIVATE_SENTINEL' }), invalid);
  const tampered = structuredClone(envelope); tampered.receipt.stages[1].metrics.totalElapsedMs++;
  assert.throws(() => api.verifyThousandQualificationReceipt(tampered, expected(envelope)), invalid);
  const forged = seal(tampered.receipt);
  assert.throws(() => api.verifyThousandQualificationReceipt(forged, expected(envelope)), invalid);
  assert.equal(api.verifyThousandQualificationReceipt(forged, expected(forged)).authenticityVerified, false);
});

test('new chain and every retained object reject unknown keys even when rehashed', () => {
  const stageLocations = [r => r, r => r.fingerprint, r => r.metrics, r => r.counts, r => r.evidence, r => r.restart, r => r.restart.before, r => r.restart.after, r => r.restart.processes, r => r.negativeCorpus, r => r.negativeCorpus.counts, r => r.negativeCorpus.diagnostic];
  const locations = [r => r, r => r.fingerprint, ...[0, 1].flatMap(index => stageLocations.map(locate => r => locate(r.stages[index])))];
  for (const locate of locations) {
    const receipt = projected().receipt; locate(receipt).secret = 'PRIVATE_SENTINEL';
    const envelope = seal(receipt);
    assert.throws(() => api.verifyThousandQualificationReceipt(envelope, expected(envelope)), invalid);
  }
  const envelope = projected(); envelope.path = 'PRIVATE_SENTINEL';
  assert.throws(() => api.verifyThousandQualificationReceipt(envelope, expected(envelope)), invalid);
  const source = chain(); source.secret = 'PRIVATE_SENTINEL';
  assert.throws(() => projected(source), invalid);
});

test('neither stage may be failed, incomplete, fake, differently sized or missing its expected negative proof', () => {
  const changes = [r => { r.schemaVersion = 2; }, r => { r.status = 'FAILED'; }, r => { r.status = 'RUNNING'; }, r => { r.status = 'AWAITING_RESTART'; }, r => { r.status = 'NOT_ADMITTED'; }, r => { r.evidenceClass = 'test-double'; }, r => { r.documentCount--; }, r => { r.documentCount = String(r.documentCount); }, r => { r.metricQualification = 'partial-failed-stage'; }, r => { r.productionSloClaim = true; }, r => { r.qualityClaim = true; }, r => { r.counts.confirmedCreatedDocuments--; }, r => { r.counts.confirmedPublishedDocuments--; }, r => { delete r.negativeCorpus; }, r => { r.negativeCorpus.status = 'FAILED'; }, r => { r.negativeCorpus.counts.confirmedPublishedDocuments = 1; }, r => { r.negativeCorpus.counts.confirmedRejectedDocuments = 0; }, r => { r.negativeCorpus.counts.confirmedHttp422Responses = 0; }, r => { r.negativeCorpus.documents[0].failureDiagnostic.httpStatus = 403; }, r => { r.negativeCorpus.documents[0].workerDiagnostic.failureCode = 'PRIVATE_SENTINEL'; }];
  for (const name of ['small', 'thousand']) for (const change of changes) {
    const source = chain(); change(source[name]); assert.throws(() => projected(source), invalid);
  }
  for (const value of ['1000', 'small', 10000, 100000]) {
    const source = chain(); source.thousand.stage = value; assert.throws(() => projected(source), invalid);
  }
  const source = chain(); source.small.stage = 1000; assert.throws(() => projected(source), invalid);
});

test('all stage IDs must be complete, unique within their stage and disjoint throughout the chain', () => {
  const changes = [c => { c.thousand.evidence.documentIds.pop(); }, c => { c.thousand.evidence.documentIds.push(uuid(1005)); }, c => { c.thousand.evidence.documentIds[1] = c.thousand.evidence.documentIds[0]; }, c => { c.thousand.evidence.documentIds[500] = c.small.evidence.documentIds[0]; }, c => { c.thousand.evidence.documentIds[500] = c.small.negativeCorpus.documentIds[0]; }, c => { c.thousand.negativeCorpus.documentIds[0] = c.small.evidence.documentIds[1]; c.thousand.negativeCorpus.documents[0].documentId = c.small.evidence.documentIds[1]; }, c => { c.thousand.negativeCorpus.documentIds[0] = c.small.negativeCorpus.documentIds[0]; c.thousand.negativeCorpus.documents[0].documentId = c.small.negativeCorpus.documentIds[0]; }];
  for (const change of changes) { const source = chain(); change(source); assert.throws(() => projected(source), invalid); }
});

test('cross-run, source, corpus, runtime and retained dataset identity drift fail closed', () => {
  for (const key of ['code', 'corpus', 'runtime']) {
    const source = chain(); source.thousand.fingerprint[key] = 'f'.repeat(key === 'code' ? 40 : 64);
    if (key === 'code') for (const part of ['before', 'after']) source.thousand.restart[part].sourceHead = source.thousand.fingerprint.code;
    assert.throws(() => projected(source), invalid);
  }
  const source = chain(); source.thousand.runId = uuid(9999);
  for (const part of ['before', 'after']) source.thousand.restart[part].runId = source.thousand.runId;
  assert.throws(() => projected(source), invalid);
  for (const key of ['databaseIdentitySha256', 'storageIdentitySha256', 'fixtureHash']) {
    const value = chain(); for (const part of ['before', 'after']) value.thousand.restart[part][key] = '0'.repeat(64);
    assert.throws(() => projected(value), invalid);
  }
  const ports = chain(); for (const part of ['before', 'after']) ports.thousand.restart[part].ports.human++;
  assert.throws(() => projected(ports), invalid);
});

test('cross-stage continuity requires exact process pairs and chronological nonoverlapping stages', () => {
  const source = chain(); source.thousand.restart.processes.before = [301, 401]; assert.throws(() => projected(source), invalid);
  for (const change of [c => { c.thousand.restart.processes.before.reverse(); }, c => { c.thousand.startedAt = c.small.startedAt; }, c => { c.thousand.finishedAt = c.thousand.startedAt; }, c => { c.small.finishedAt = c.small.startedAt; }]) {
    const value = chain(); change(value); assert.throws(() => projected(value), invalid);
  }
  const adjacent = chain(); adjacent.thousand.startedAt = adjacent.small.finishedAt; assert.ok(projected(adjacent));
});

test('sanitized historical receipts cannot substitute for either full fresh report', () => {
  const value = chain(), old = api.projectQualificationReceipt(value.small);
  assert.throws(() => projected({ small: old.receipt, thousand: value.thousand }), invalid);
  assert.throws(() => projected({ small: old, thousand: value.thousand }), invalid);
  const envelope = projected(value);
  assert.throws(() => projected({ small: envelope.receipt.stages[0], thousand: envelope.receipt.stages[1] }), invalid);
  const reconstructed = chain(); reconstructed.thousand.previousReport = structuredClone(reconstructed.small);
  assert.throws(() => projected(reconstructed), invalid);
});

test('the chain verifier independently enforces stage and cross-stage binding after rehashing', () => {
  const changes = [r => { r.runId = uuid(9999); }, r => { r.fingerprint.runtime = 'f'.repeat(64); }, r => { r.status = 'FAILED'; }, r => { r.productionSloClaim = true; }, r => { r.qualityClaim = true; }, r => { r.stages.reverse(); }, r => { r.stages.pop(); }, r => { r.stages[1].counts.confirmedPublishedDocuments = 999; }, r => { r.stages[1].evidence.documentIds[20] = r.stages[0].negativeCorpus.documentIds[0]; }, r => { r.stages[1].restart.processes.before = [301, 401]; }, r => { r.stages[1].startedAt = r.stages[0].startedAt; }, r => { for (const part of ['before', 'after']) r.stages[1].restart[part].storageIdentitySha256 = 'f'.repeat(64); }];
  for (const change of changes) {
    const receipt = projected().receipt; change(receipt); const envelope = seal(receipt);
    assert.throws(() => api.verifyThousandQualificationReceipt(envelope, expected(envelope)), invalid);
  }
});

test('secret-bearing values, malformed types, exotic objects and oversized JSON cannot escape either API', () => {
  const paths = [['runId'], ['fingerprint', 'code'], ['fingerprint', 'corpus'], ['fingerprint', 'runtime'], ['startedAt'], ['finishedAt'], ['evidence', 'documentIds', 0], ['negativeCorpus', 'documentIds', 0], ['restart', 'before', 'databaseIdentitySha256'], ['restart', 'before', 'storageIdentitySha256'], ['metrics', 'totalElapsedMs'], ['metrics', 'peakRssBytes']];
  for (const name of ['small', 'thousand']) for (const path of paths) for (const bad of ['PRIVATE_SENTINEL', '/private/report.json', null, undefined, Infinity, -1]) {
    const source = chain(); const parent = path.slice(0, -1).reduce((node, key) => node[key], source[name]); parent[path.at(-1)] = bad;
    assert.throws(() => projected(source), invalid);
  }
  const envelope = projected();
  for (const input of ['PRIVATE_SENTINEL', '{', ' '.repeat(api.MAX_RECEIPT_BYTES + 1), Buffer.alloc(api.MAX_RECEIPT_BYTES + 1), null, [], {}, new Date()]) assert.throws(() => api.verifyThousandQualificationReceipt(input, expected(envelope)), invalid);
  for (const mutate of [e => { e.receipt.stages.secret = 'PRIVATE_SENTINEL'; }, e => { delete e.receipt.stages[1]; }, e => { e.receipt.stages[1].evidence.documentIds.secret = 'PRIVATE_SENTINEL'; }, e => { e.receipt.stages[1].restart.processes.after.toJSON = () => 'PRIVATE_SENTINEL'; }, e => { e.receipt[Symbol('secret')] = 'PRIVATE_SENTINEL'; }, e => { Object.defineProperty(e.receipt, 'status', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } }); }]) {
    const value = structuredClone(envelope); mutate(value); assert.throws(() => api.verifyThousandQualificationReceipt(value, expected(envelope)), invalid);
  }
});

test('old small APIs retain their schema and reject the new chain or large report', () => {
  const source = chain(), old = api.projectQualificationReceipt(source.small);
  // Recorded with the unmodified old projector before adding the chain API.
  assert.equal(old.sha256, 'a7ad686267b1caf261a1dabe3ff3a7ede903ebc45507654c0845a99f49a3f0ee');
  assert.deepEqual(api.verifyQualificationReceipt(old, expected(old)).receipt, old.receipt);
  assert.throws(() => api.projectQualificationReceipt(source.thousand), invalid);
  assert.throws(() => api.verifyQualificationReceipt(projected(), expected(projected())), invalid);
  assert.throws(() => api.verifyThousandQualificationReceipt(old, expected(old)), invalid);
});
