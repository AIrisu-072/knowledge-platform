import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { localScaleReports, fingerprint, runId, uuid, names } from './fixtures/local-scale-reports.mjs';

const api = await import('../local-receipt.mjs').catch(() => ({}));
const canonical = value => JSON.stringify(value, function (key, item) {
  return item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map(name => [name, item[name]])) : item;
});
const digest = value => createHash('sha256').update(canonical(value)).digest('hex');
const idsDigest = values => createHash('sha256').update(values.map(value => value.toLowerCase()).sort().join('\n') + '\n').digest('hex');
const seal = receipt => ({ schemaVersion: 1, sha256: digest(receipt), receipt });
const bindings = envelope => ({ ...fingerprint, runId, sha256: envelope.sha256 });
const invalid = /^Error: Invalid local scale receipt$/;
function project(source = localScaleReports()) {
  assert.equal(typeof api.projectLocalScaleReceipt, 'function');
  return api.projectLocalScaleReceipt(source);
}
function verify(envelope, expected = bindings(envelope)) {
  assert.equal(typeof api.verifyLocalScaleReceipt, 'function');
  return api.verifyLocalScaleReceipt(envelope, expected);
}
function rejectSource(change) {
  const source = localScaleReports(); change(source);
  assert.throws(() => project(source), invalid);
}

test('projects full 111002 positive IDs into four compact digests after verified owned cleanup', () => {
  const source = localScaleReports(), envelope = project(source), serialized = JSON.stringify(envelope);
  assert.deepEqual(envelope, project());
  assert.equal(envelope.schemaVersion, 1);
  assert.equal(envelope.sha256, digest(envelope.receipt));
  assert.equal(envelope.receipt.receiptKind, 'local-scale-compact');
  assert.equal(envelope.receipt.status, 'SUCCEEDED');
  assert.deepEqual(envelope.receipt.stages.map(stage => [stage.stage, stage.documentCount]), [['small', 2], [1000, 1000], [10000, 10000], [100000, 100000]]);
  for (const [index, name] of names.entries()) {
    const report = source[name], stage = envelope.receipt.stages[index];
    assert.deepEqual(stage.evidence, { positiveDocumentCount: report.documentCount,
      positiveDocumentIdsSha256: idsDigest(report.evidence.documentIds), detailedSampleCount: index === 0 ? 2 : 3, allOriginalHashesVerified: false });
    assert.deepEqual(stage.negativeCorpus.documentIds, report.negativeCorpus.documentIds);
    assert.deepEqual(stage.metrics, Object.fromEntries(Object.entries(report.metrics).filter(([key]) => key !== 'rssMeasurement')));
    assert.equal(stage.restart.before.databaseStorageMode, 'owned-ext4');
    assert.equal(stage.restart.before.databaseStorageIdentitySha256, '0'.repeat(64));
  }
  assert.deepEqual(envelope.receipt.cleanup, source.cleanup);
  assert.equal(api.MAX_LOCAL_SCALE_RECEIPT_BYTES, 1024 * 1024);
  assert.ok(Buffer.byteLength(serialized) < 16 * 1024);
  for (const forbidden of ['PRIVATE_SENTINEL', 'ports', 'fixtureHash', 'snapshots', 'sampledDocumentIds', 'observations', 'credentials', 'rawPdf', 'previousReport', '/private/']) assert.ok(!serialized.includes(forbidden), forbidden);
  assert.ok(!serialized.includes(source.hundredThousand.evidence.documentIds[55]));
  assert.equal(source.hundredThousand.evidence.documentIds.length, 100000);
});

test('digest normalizes uppercase UUIDs and sorts without changing the source arrays', () => {
  const source = localScaleReports();
  const report = source.hundredThousand;
  const original = [...report.evidence.documentIds];
  report.evidence.documentIds = [...original].reverse().map(id => id.toUpperCase());
  report.evidence.sampledDocumentIds = [report.evidence.documentIds[0], report.evidence.documentIds[50000], report.evidence.documentIds.at(-1)];
  report.evidence.snapshots = Object.fromEntries(report.evidence.sampledDocumentIds.map(id => [id, {}]));
  assert.equal(project(source).receipt.stages[3].evidence.positiveDocumentIdsSha256, idsDigest(original));
  assert.equal(report.evidence.documentIds[0], original.at(-1).toUpperCase());
});

test('verification binds code, corpus, runtime, run and digest but does not authenticate execution', () => {
  const envelope = project();
  for (const input of [envelope, JSON.stringify(envelope), Buffer.from(JSON.stringify(envelope))]) {
    const result = verify(input, bindings(envelope));
    assert.deepEqual(result, { receipt: envelope.receipt, integrityVerified: true, authenticityVerified: false });
    assert.notEqual(result.receipt, envelope.receipt);
  }
  for (const key of ['code', 'corpus', 'runtime', 'runId', 'sha256']) {
    const expected = bindings(envelope); delete expected[key];
    assert.throws(() => verify(envelope, expected), invalid);
    assert.throws(() => verify(envelope, { ...bindings(envelope), [key]: key === 'runId' ? uuid(999999) : 'f'.repeat(key === 'code' ? 40 : 64) }), invalid);
  }
  assert.throws(() => api.verifyLocalScaleReceipt(envelope), invalid);
  const changed = structuredClone(envelope); changed.receipt.stages[3].metrics.totalElapsedMs++;
  assert.throws(() => verify(changed, bindings(envelope)), invalid);
  assert.throws(() => verify(seal(changed.receipt), bindings(envelope)), invalid);
  assert.equal(verify(seal(changed.receipt)).authenticityVerified, false);
  // The compact digest cannot reconstruct or independently re-check full IDs.
  const rebound = structuredClone(envelope.receipt); rebound.stages[3].evidence.positiveDocumentIdsSha256 = 'f'.repeat(64);
  assert.equal(verify(seal(rebound)).authenticityVerified, false);
});

test('projection requires all exact full-report counts and complete successful measurements', () => {
  const changes = [r => { r.status = 'FAILED'; }, r => { r.evidenceClass = 'test-double'; }, r => { r.documentCount--; },
    r => { r.metricQualification = 'partial-failed-stage'; }, r => { r.qualityClaim = true; }, r => { r.productionSloClaim = true; },
    r => { r.counts.confirmedCreatedDocuments--; }, r => { r.counts.confirmedPublishedDocuments--; }, r => { delete r.metrics; },
    r => { r.metrics.peakRssBytes = null; }, r => { r.metrics.totalElapsedMs = 0; }, r => { r.metrics.storageGrowthBytes = null; },
    r => { r.metrics.diskGrowthBytes++; }, r => { r.metrics.databaseAllocatedGrowthBytes = -1; }, r => { r.metrics.databaseGrowthBytes = 21; },
    r => { r.negativeCorpus.status = 'FAILED'; }, r => { r.negativeCorpus.counts.confirmedPublishedDocuments = 1; },
    r => { r.negativeCorpus.counts.confirmedHttp422Responses = 0; }, r => { r.negativeCorpus.documents[0].workerDiagnostic.qualification = true; },
    r => { r.negativeCorpus.documents[0].failureDiagnostic.httpStatus = 403; }, r => { r.negativeCorpus.documents[0].workerDiagnostic.failureCode = 'PRIVATE_SENTINEL'; }];
  for (const name of names) for (const change of changes) rejectSource(source => change(source[name]));
  for (const stage of ['100000', 10000, 'small']) rejectSource(source => { source.hundredThousand.stage = stage; });
});

test('projection checks every positive and negative UUID globally, including case aliases', () => {
  for (const name of names) {
    rejectSource(source => { source[name].evidence.documentIds.pop(); });
    rejectSource(source => { source[name].evidence.documentIds.push(uuid(999999)); });
    rejectSource(source => { source[name].evidence.documentIds[1] = source[name].evidence.documentIds[0]; });
    rejectSource(source => { source[name].negativeCorpus.documents[0].documentId = uuid(999999); });
  }
  for (const previous of names.slice(0, 3)) for (const kind of ['evidence', 'negativeCorpus']) {
    rejectSource(source => { source.hundredThousand.evidence.documentIds[55555] = source[previous][kind].documentIds[0].toUpperCase(); });
    rejectSource(source => { const duplicate = source[previous][kind].documentIds[0];
      source.hundredThousand.negativeCorpus.documentIds[0] = duplicate; source.hundredThousand.negativeCorpus.documents[0].documentId = duplicate; });
  }
});

test('the exact bounded source samples are required without claiming all original hashes were verified', () => {
  for (const name of names) for (const change of [
    r => { delete r.evidence.sampledDocumentIds; }, r => { r.evidence.sampledDocumentIds.pop(); },
    r => { r.evidence.sampledDocumentIds.push(r.evidence.documentIds[1]); },
    r => { r.evidence.sampledDocumentIds.reverse(); }, r => { r.evidence.snapshots = {}; },
    r => { r.evidence.snapshots[r.evidence.documentIds[0]] = null; }, r => { r.evidence.snapshots.secret = 'PRIVATE_SENTINEL'; },
    r => { r.evidence.status = 'FAILED'; }, r => { r.evidence.contentQualityClaim = true; },
  ]) rejectSource(source => change(source[name]));
});

test('fresh previous references, complete dataset identity, ext4 mount, time and PID chain remain mandatory', () => {
  for (const name of names.slice(1)) {
    for (const key of ['code', 'corpus', 'runtime']) rejectSource(source => { source[name].fingerprint[key] = 'f'.repeat(key === 'code' ? 40 : 64); });
    for (const key of ['databaseIdentitySha256', 'storageIdentitySha256', 'databaseStorageIdentitySha256', 'fixtureHash']) rejectSource(source => {
      for (const part of ['before', 'after']) source[name].restart[part][key] = '1'.repeat(64);
    });
    rejectSource(source => { for (const part of ['before', 'after']) source[name].restart[part].ports.human++; });
    rejectSource(source => { source[name].previousReport = structuredClone(source[name].previousReport); });
    rejectSource(source => { delete source[name].previousReport; });
    rejectSource(source => { source[name].runId = uuid(999999); });
  }
  for (const name of names) for (const change of [
    r => { r.restart.before.databaseStorageMode = 'host-filesystem'; }, r => { delete r.restart.after.databaseStorageIdentitySha256; },
    r => { r.restart.processes.after = [...r.restart.processes.before]; }, r => { r.restart.processes.after.reverse(); },
    r => { r.restart.processes.before[0] = 0; }, r => { r.restart.processes.before = [300, 300]; },
    r => { r.finishedAt = r.startedAt; }, r => { r.startedAt = 'PRIVATE_SENTINEL'; },
  ]) rejectSource(source => change(source[name]));
  rejectSource(source => { source.hundredThousand.startedAt = source.tenThousand.startedAt; });
  const adjacent = localScaleReports(); adjacent.hundredThousand.startedAt = adjacent.tenThousand.finishedAt;
  assert.ok(project(adjacent));
});

test('the small stage starts a fresh chain without a hidden previous report', () => {
  rejectSource(source => { source.small.previousReport = { status: 'SUCCEEDED', private: 'PRIVATE_SENTINEL' }; });
  rejectSource(source => { source.small.previousReport = source.hundredThousand; });
  const source = localScaleReports(); source.small.previousReport = undefined;
  assert.ok(project(source));
});

test('success requires actual fixed cleanup records for the final process pair, proxy and owned database', () => {
  for (const change of [
    c => { c.ownedCleanupSucceeded = false; }, c => { delete c.ownedCleanupSucceeded; }, c => { c.records = []; },
    c => { c.records.pop(); }, c => { c.records.push({ resource: 'other', result: 'removed' }); },
    c => { c.records[0].pid = 103; }, c => { c.records[1].pid = c.records[0].pid; },
    c => { c.records[0].result = 'forced-test-cleanup'; }, c => { c.records[2].result = 'cleanup-unconfirmed'; },
    c => { c.records[3].resource = 'external-postgres'; }, c => { c.records[0].path = '/private/PRIVATE_SENTINEL'; },
    c => { c.secret = 'PRIVATE_SENTINEL'; },
  ]) rejectSource(source => change(source.cleanup));
  rejectSource(source => { delete source.cleanup; });
  rejectSource(source => { source.cleanup = { failed: false, cleanup: source.cleanup.records }; });
});

test('closed verifier rejects all unknown nested fields and malicious rehashed values', () => {
  const valid = project();
  const stageLocations = [r => r, r => r.fingerprint, r => r.metrics, r => r.counts, r => r.evidence, r => r.restart,
    r => r.restart.before, r => r.restart.after, r => r.restart.processes, r => r.negativeCorpus,
    r => r.negativeCorpus.counts, r => r.negativeCorpus.diagnostic];
  const locations = [r => r, r => r.fingerprint, r => r.cleanup, ...[0, 1, 2, 3].map(i => r => r.cleanup.records[i]),
    ...[0, 1, 2, 3].flatMap(index => stageLocations.map(locate => r => locate(r.stages[index])))];
  for (const locate of locations) {
    const receipt = structuredClone(valid.receipt); locate(receipt).secret = 'PRIVATE_SENTINEL';
    assert.throws(() => verify(seal(receipt)), invalid);
  }
  const changes = [r => { r.stages.reverse(); }, r => { r.stages.pop(); }, r => { r.runId = uuid(999999); },
    r => { r.stages[3].counts.confirmedPublishedDocuments--; }, r => { r.stages[3].evidence.detailedSampleCount = 100000; },
    r => { r.stages[3].evidence.positiveDocumentCount--; }, r => { r.stages[3].evidence.positiveDocumentIdsSha256 = null; },
    r => { r.stages[3].evidence.allOriginalHashesVerified = true; }, r => { r.stages[3].metrics.storageGrowthBytes = null; },
    r => { r.stages[3].restart.before.databaseStorageMode = 'external'; }, r => { r.stages[3].restart.processes.before = [300, 400]; },
    r => { r.stages[3].negativeCorpus.documentIds[0] = r.stages[0].negativeCorpus.documentIds[0]; },
    r => { r.stages[3].startedAt = r.stages[2].startedAt; }, r => { r.cleanup.ownedCleanupSucceeded = false; },
    r => { r.cleanup.records[0].pid = 103; }];
  for (const change of changes) { const receipt = structuredClone(valid.receipt); change(receipt); assert.throws(() => verify(seal(receipt)), invalid); }
});

test('exotic objects, getters, sparse arrays and over-limit envelopes fail with redacted errors', () => {
  const envelope = project();
  const badValues = ['PRIVATE_SENTINEL', '/private/report.json', null, undefined, Infinity, -1];
  for (const bad of badValues) for (const path of [['runId'], ['fingerprint', 'code'], ['metrics', 'totalElapsedMs'], ['metrics', 'storageGrowthBytes'], ['evidence', 'documentIds', 55555]]) {
    rejectSource(source => { const node = path.slice(0, -1).reduce((node, key) => node[key], source.hundredThousand); node[path.at(-1)] = bad; });
  }
  for (const input of ['PRIVATE_SENTINEL', '{', ' '.repeat(1024 * 1024 + 1), Buffer.alloc(1024 * 1024 + 1), null, [], {}, new Date()]) assert.throws(() => api.verifyLocalScaleReceipt(input, bindings(envelope)), invalid);
  const padded = JSON.stringify(envelope) + ' '.repeat(1024 * 1024 - Buffer.byteLength(JSON.stringify(envelope)) + 1);
  assert.throws(() => verify(padded, bindings(envelope)), invalid);
  for (const mutate of [e => { e.receipt.stages.secret = 'PRIVATE_SENTINEL'; }, e => { delete e.receipt.stages[3]; },
    e => { e.receipt.stages[3].restart.processes.after.toJSON = () => 'PRIVATE_SENTINEL'; },
    e => { e.receipt[Symbol('secret')] = 'PRIVATE_SENTINEL'; }, e => { e.receipt = new (class Receipt {})(); },
    e => { Object.defineProperty(e.receipt, 'status', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } }); },
  ]) { const value = structuredClone(envelope); mutate(value); assert.throws(() => verify(value, bindings(envelope)), invalid); }
  for (const change of [source => { source.extra = 'PRIVATE_SENTINEL'; }, source => { delete source.hundredThousand.evidence.documentIds[55555]; },
    source => { Object.defineProperty(source.hundredThousand.metrics, 'totalElapsedMs', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } }); },
    source => { source.hundredThousand.evidence.documentIds.toJSON = () => 'PRIVATE_SENTINEL'; },
  ]) rejectSource(change);
});

test('private report extras are ignored without traversal, stringification or mutation', () => {
  const source = localScaleReports();
  for (const name of names) {
    source[name].unknown = source[name];
    source[name].toJSON = () => { throw Error('PRIVATE_SENTINEL'); };
    Object.defineProperty(source[name], 'privateGetter', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } });
  }
  assert.deepEqual(project(source), project());
});

test('full 100000-report projection and verification fit a 128 MiB heap without serializing full reports', () => {
  assert.equal(typeof api.projectLocalScaleReceipt, 'function');
  const receiptUrl = new URL('../local-receipt.mjs', import.meta.url).href;
  const fixtureUrl = new URL('./fixtures/local-scale-reports.mjs', import.meta.url).href;
  const script = `import {projectLocalScaleReceipt, verifyLocalScaleReceipt} from ${JSON.stringify(receiptUrl)};
    import {localScaleReports, fingerprint, runId} from ${JSON.stringify(fixtureUrl)};
    const result = projectLocalScaleReceipt(localScaleReports());
    verifyLocalScaleReceipt(result, {...fingerprint, runId, sha256:result.sha256});
    process.stdout.write(String(Buffer.byteLength(JSON.stringify(result))));`;
  const output = execFileSync(process.execPath, ['--max-old-space-size=128', '--input-type=module', '-e', script], { encoding: 'utf8', timeout: 15000, maxBuffer: 1024 });
  assert.ok(Number(output) > 0 && Number(output) < 16 * 1024);
});
