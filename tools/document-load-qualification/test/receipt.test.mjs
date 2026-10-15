import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { admitStage } from '../safety.mjs';
const { projectQualificationReceipt, verifyQualificationReceipt, MAX_RECEIPT_BYTES } = await import('../receipt.mjs').catch(() => ({}));

const digest = value => createHash('sha256').update(value).digest('hex');
const canonical = value => JSON.stringify(value, function (key, item) {
  return item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map(name => [name, item[name]])) : item;
});
const seal = receipt => ({ schemaVersion: 1, sha256: digest(canonical(receipt)), receipt });
const runId = '12345678-1234-4abc-8abc-123456789abc';
const ids = ['0198eada-1234-7000-8000-000000000001', '0198eada-1234-7000-8000-000000000002'];
const negativeId = '0198eada-1234-7000-8000-000000000003';
const fingerprint = { code: 'a'.repeat(40), corpus: 'b'.repeat(64), runtime: 'c'.repeat(64) };
function report() {
  const identity = { runId, sourceHead: fingerprint.code, databaseIdentitySha256: 'd'.repeat(64), storageIdentitySha256: 'e'.repeat(64), fixtureHash: 'f'.repeat(64), ports: { human: 41001, agent: 41002, postgres: 41003, proxy: 41004 } };
  return {
    schemaVersion: 1, evidenceClass: 'owned-real-process', status: 'SUCCEEDED', runId,
    stage: 'small', documentCount: 2, fingerprint: { ...fingerprint },
    startedAt: '2026-10-08T16:00:00.000Z', finishedAt: '2026-10-08T16:00:01.000Z',
    metricQualification: 'complete-stage', productionSloClaim: false, qualityClaim: false,
    metrics: { totalElapsedMs: 1000.25, peakRssBytes: 4096, diskGrowthBytes: 30, storageAllocatedGrowthBytes: 10, databaseAllocatedGrowthBytes: 20, databaseGrowthBytes: 10, storageGrowthBytes: 5, rssMeasurement: 'PRIVATE_SENTINEL', diskMeasurement: 'PRIVATE_SENTINEL' },
    counts: { targetDocuments: 2, confirmedCreatedDocuments: 2, confirmedPublishedDocuments: 2 },
    evidence: { status: 'AWAITING_RESTART', documentIds: [...ids], sampledDocumentIds: [...ids], uniqueOriginalCount: 2, syntheticRepetitionCount: 2, versionCount: 3, snapshots: { private: 'PRIVATE_SENTINEL' } },
    restart: { identityRetained: true, processesReplaced: true, before: identity, after: structuredClone(identity), processes: { before: [100, 200], after: [101, 201] } },
    negativeCorpus: { status: 'SUCCEEDED', contentQualityClaim: false, documentIds: [negativeId], counts: { targetDocuments: 1, confirmedCreatedDocuments: 1, confirmedHttp422Responses: 1, confirmedRejectedDocuments: 1, confirmedPublishedDocuments: 0 }, documents: [{ documentId: negativeId, failureDiagnostic: { operation: 'publish', httpStatus: 422, problemCode: 'BUSINESS_RULE_REJECTED' }, workerDiagnostic: { status: 'worker-failure', failureCode: 'unsupported_semantic_construct', qualification: false }, snapshot: 'PRIVATE_SENTINEL' }] },
    plan: { privatePath: '/private/PRIVATE_SENTINEL' }, observations: ['PRIVATE_SENTINEL'], timings: { PRIVATE_SENTINEL: {} }, previousReport: { secret: 'PRIVATE_SENTINEL' }, credentials: 'PRIVATE_SENTINEL', rawPdf: Buffer.from('PRIVATE_SENTINEL'), environment: { PASSWORD: 'PRIVATE_SENTINEL' },
  };
}
const expected = envelope => ({ ...fingerprint, runId, sha256: envelope.sha256 });
function projected() {
  assert.equal(typeof projectQualificationReceipt, 'function');
  return projectQualificationReceipt(report());
}

test('projects one small successful report into a deterministic bounded safe receipt', () => {
  const original = report();
  const before = JSON.stringify(original);
  assert.equal(typeof projectQualificationReceipt, 'function');
  const envelope = projectQualificationReceipt(original);
  assert.equal(JSON.stringify(original), before);
  assert.deepEqual(envelope, projectQualificationReceipt(report()));
  assert.deepEqual(Object.keys(envelope).sort(), ['receipt', 'schemaVersion', 'sha256']);
  assert.equal(envelope.schemaVersion, 1);
  assert.equal(envelope.sha256, digest(canonical(envelope.receipt)));
  assert.equal(MAX_RECEIPT_BYTES, 1024 * 1024);
  assert.ok(Buffer.byteLength(JSON.stringify(envelope)) < MAX_RECEIPT_BYTES);
  assert.equal(envelope.receipt.documentCount, 2);
  assert.deepEqual(envelope.receipt.evidence, { documentIds: ids });
  assert.deepEqual(envelope.receipt.restart.processes, { before: [100, 200], after: [101, 201] });
  assert.deepEqual(Object.keys(envelope.receipt.restart.before).sort(), ['databaseIdentitySha256', 'sourceHead', 'storageIdentitySha256']);
  assert.deepEqual(envelope.receipt.metrics, { totalElapsedMs: 1000.25, peakRssBytes: 4096, diskGrowthBytes: 30, storageAllocatedGrowthBytes: 10, databaseAllocatedGrowthBytes: 20, databaseGrowthBytes: 10, storageGrowthBytes: 5 });
  const text = JSON.stringify(envelope);
  for (const forbidden of ['PRIVATE_SENTINEL', 'ports', 'fixtureHash', 'snapshots', 'observations', 'timings', 'credentials', 'rawPdf', 'environment', 'previousReport', 'plan', 'sampledDocumentIds']) assert.ok(!text.includes(forbidden), forbidden);
});

test('projection never traverses or serializes unknown private report fields', () => {
  const source = report();
  source.unknownPrivateField = source;
  source.toJSON = () => { throw Error('PRIVATE_SENTINEL'); };
  Object.defineProperty(source, 'secretGetter', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } });
  assert.deepEqual(projectQualificationReceipt(source), projected());
});

test('verifier binds exact externally supplied identifiers and content hash but makes no authenticity claim', () => {
  const envelope = projected();
  const result = verifyQualificationReceipt(JSON.stringify(envelope), expected(envelope));
  assert.deepEqual(result, { receipt: envelope.receipt, integrityVerified: true, authenticityVerified: false });
  assert.notEqual(result.receipt, envelope.receipt);
  for (const key of ['code', 'corpus', 'runtime', 'runId', 'sha256']) {
    const missing = expected(envelope); delete missing[key];
    assert.throws(() => verifyQualificationReceipt(envelope, missing), /Invalid qualification receipt/);
    assert.throws(() => verifyQualificationReceipt(envelope, { ...expected(envelope), [key]: key === 'runId' ? '12345678-1234-4abc-8abc-123456789abd' : 'f'.repeat(key === 'code' ? 40 : 64) }), /Invalid qualification receipt/);
  }
  assert.throws(() => verifyQualificationReceipt(envelope), /Invalid qualification receipt/);
  assert.throws(() => verifyQualificationReceipt({ ...envelope, sha256: 'f'.repeat(64) }, expected(envelope)), /Invalid qualification receipt/);
  const tampered = structuredClone(envelope); tampered.receipt.metrics.totalElapsedMs++;
  assert.throws(() => verifyQualificationReceipt(tampered, expected(envelope)), /Invalid qualification receipt/);
  const forged = seal(tampered.receipt);
  assert.throws(() => verifyQualificationReceipt(forged, expected(envelope)), /Invalid qualification receipt/);
  // Anyone can recompute a checksum. Even matching attacker-supplied expected
  // values never authenticates GitHub ownership, the run or real execution.
  assert.equal(verifyQualificationReceipt(forged, expected(forged)).authenticityVerified, false);
});

test('unknown receipt keys are rejected at every object boundary even with a recomputed hash', () => {
  const locations = [r => r, r => r.fingerprint, r => r.metrics, r => r.counts, r => r.evidence, r => r.restart, r => r.restart.before, r => r.restart.after, r => r.restart.processes, r => r.negativeCorpus, r => r.negativeCorpus.counts, r => r.negativeCorpus.diagnostic];
  for (const locate of locations) {
    const receipt = projected().receipt; locate(receipt).secret = 'PRIVATE_SENTINEL';
    const envelope = seal(receipt);
    assert.throws(() => verifyQualificationReceipt(envelope, expected(envelope)), /^Error: Invalid qualification receipt$/);
  }
  const envelope = projected(); envelope.privatePath = 'PRIVATE_SENTINEL';
  assert.throws(() => verifyQualificationReceipt(envelope, expected(envelope)), /^Error: Invalid qualification receipt$/);
});

test('failed, partial, fake, large or incomplete stage reports cannot be exported', () => {
  const changes = [r => { r.schemaVersion = 2; }, r => { r.status = 'FAILED'; }, r => { r.evidenceClass = 'test-double'; }, r => { r.stage = 1000; r.documentCount = 1000; }, r => { r.documentCount = 1; }, r => { r.metricQualification = 'partial-failed-stage'; }, r => { r.productionSloClaim = true; }, r => { r.qualityClaim = true; }, r => { r.counts.confirmedPublishedDocuments = 1; }, r => { delete r.negativeCorpus; }, r => { r.negativeCorpus.status = 'FAILED'; }, r => { r.negativeCorpus.counts.confirmedPublishedDocuments = 1; }, r => { r.negativeCorpus.counts.confirmedRejectedDocuments = 0; }, r => { r.negativeCorpus.documents[0].workerDiagnostic.failureCode = 'PRIVATE_SENTINEL'; }];
  for (const change of changes) { const source = report(); change(source); assert.throws(() => projectQualificationReceipt(source), /^Error: Invalid qualification receipt$/); }
});

test('secret-bearing IDs, hashes, timestamps and unsupported diagnostic strings fail closed', () => {
  const paths = [['runId'], ['fingerprint', 'code'], ['fingerprint', 'corpus'], ['fingerprint', 'runtime'], ['startedAt'], ['finishedAt'], ['evidence', 'documentIds', 0], ['negativeCorpus', 'documentIds', 0], ['restart', 'before', 'databaseIdentitySha256'], ['restart', 'before', 'storageIdentitySha256']];
  for (const path of paths) for (const value of ['PRIVATE_SENTINEL', '/private/report.json', 'a'.repeat(1024 * 1024), null, undefined]) {
    const source = report(); const parent = path.slice(0, -1).reduce((node, key) => node[key], source); parent[path.at(-1)] = value;
    assert.throws(() => projectQualificationReceipt(source), /^Error: Invalid qualification receipt$/);
  }
  for (const change of [r => { r.finishedAt = r.startedAt; }, r => { r.startedAt = '2026-02-30T00:00:00.000Z'; }, r => { r.fingerprint.code = 'A'.repeat(40); }, r => { r.evidence.documentIds[1] = r.evidence.documentIds[0]; }, r => { r.negativeCorpus.documentIds[0] = r.evidence.documentIds[0]; }]) {
    const source = report(); change(source); assert.throws(() => projectQualificationReceipt(source), /^Error: Invalid qualification receipt$/);
  }
});

test('all exported metrics must be finite measured numbers with positive elapsed time and RSS', () => {
  for (const field of ['totalElapsedMs', 'peakRssBytes', 'diskGrowthBytes', 'storageAllocatedGrowthBytes', 'databaseAllocatedGrowthBytes', 'databaseGrowthBytes', 'storageGrowthBytes']) {
    for (const invalid of [NaN, Infinity, -Infinity, -1, '1', 'PRIVATE_SENTINEL', null, undefined, Number.MAX_SAFE_INTEGER + 1]) {
      const source = report(); source.metrics[field] = invalid;
      assert.throws(() => projectQualificationReceipt(source), /^Error: Invalid qualification receipt$/);
    }
  }
  for (const field of ['totalElapsedMs', 'peakRssBytes']) { const source = report(); source.metrics[field] = 0; assert.throws(() => projectQualificationReceipt(source)); }
  const source = report(); for (const key of Object.keys(source.metrics).filter(key => key.endsWith('GrowthBytes'))) source.metrics[key] = 0;
  assert.equal(projectQualificationReceipt(source).receipt.metrics.diskGrowthBytes, 0);
});

test('restart requires complete paired identities, exact source/run binding and actual changed PIDs', () => {
  for (const change of [r => { delete r.restart.processes; }, r => { r.restart = { identityRetained: true, processesReplaced: true }; }, r => { r.restart.processes.before = [100, 100]; }, r => { r.restart.processes.after = [100, 201]; }, r => { r.restart.processes.after = [101, 101]; }, r => { r.restart.processes.after = [101, '201']; }, r => { r.restart.processes.after = [101, -1]; }, r => { r.restart.before.sourceHead = 'f'.repeat(40); }, r => { r.restart.before.runId = '12345678-1234-4abc-8abc-123456789abd'; }, r => { r.restart.after.storageIdentitySha256 = 'f'.repeat(64); }, r => { r.restart.after.ports.human++; }, r => { r.restart.identityRetained = false; }, r => { r.restart.processesReplaced = false; }]) {
    const source = report(); change(source); assert.throws(() => projectQualificationReceipt(source), /^Error: Invalid qualification receipt$/);
  }
});

test('size cap and strict JSON schemas reject malformed, oversized and exotic inputs without revealing content', () => {
  const envelope = projected();
  for (const value of ['PRIVATE_SENTINEL', '{', ' '.repeat(1024 * 1024 + 1), null, [], {}, new Date()]) assert.throws(() => verifyQualificationReceipt(value, expected(envelope)), /^Error: Invalid qualification receipt$/);
  const exotic = structuredClone(envelope); Object.defineProperty(exotic.receipt, 'status', { get() { throw Error('PRIVATE_SENTINEL'); }, enumerable: true });
  assert.throws(() => verifyQualificationReceipt(exotic, expected(envelope)), /^Error: Invalid qualification receipt$/);
  const withPrototype = Object.assign(Object.create({ secret: 'PRIVATE_SENTINEL' }), envelope);
  assert.throws(() => verifyQualificationReceipt(withPrototype, expected(envelope)), /^Error: Invalid qualification receipt$/);
});

test('bounded receipt preserves current capacity-admission proof without weakening safety admission', () => {
  const envelope = projected();
  const checked = verifyQualificationReceipt(envelope, expected(envelope));
  const plan = { stage: 1000, documentCount: 1000, fingerprint, safetyFactor: 2, budgets: { diskReserveBytes: 100, minAvailableMemoryBytes: 100, maxRssBytes: 100000, maxWallTimeMs: 2000000 }, deadlineAt: new Date(Date.now() + 2000000).toISOString() };
  const observation = { observedAt: new Date().toISOString(), diskFreeBytes: 10000000, storageDiskFreeBytes: 10000000, databaseDiskFreeBytes: 10000000, availableMemoryBytes: 10000000, rssBytes: 100, storageBytes: 0, databaseBytes: 100, rssCoverage: 'process-tree', databaseFilesystemVerified: true };
  assert.equal(admitStage(plan, checked.receipt, observation).status, 'ADMITTED');
  const booleanOnly = structuredClone(checked.receipt); delete booleanOnly.restart.processes;
  assert.equal(admitStage(plan, booleanOnly, observation).status, 'NOT_ADMITTED');
  // This is schema compatibility only. A restore path MUST verify independent
  // artifact provenance before passing ANY downloaded receipt to admitStage.
  assert.equal(checked.authenticityVerified, false);
});
