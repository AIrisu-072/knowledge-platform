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
const names = ['small', 'thousand', 'tenThousand'];

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
  const tenThousand = stageReport(10000, 10000, 1005, '2026-10-08T16:09:03.000Z', '2026-10-08T17:39:03.000Z', [102, 202], [103, 203]);
  thousand.previousReport = small;
  tenThousand.previousReport = thousand;
  return { small, thousand, tenThousand };
}
function projected(source = chain()) {
  assert.equal(typeof api.projectTenThousandQualificationReceipt, 'function');
  return api.projectTenThousandQualificationReceipt(source);
}
function verified(envelope, bindings = expected(envelope)) {
  assert.equal(typeof api.verifyTenThousandQualificationReceipt, 'function');
  return api.verifyTenThousandQualificationReceipt(envelope, bindings);
}

test('projects exactly small, 1000 and 10000 into one deterministic bounded receipt retaining all IDs', () => {
  const source = chain(), before = JSON.stringify(source), envelope = projected(source);
  assert.equal(JSON.stringify(source), before);
  assert.deepEqual(envelope, projected());
  assert.deepEqual(Object.keys(envelope).sort(), ['receipt', 'schemaVersion', 'sha256']);
  assert.equal(envelope.sha256, digest(envelope.receipt));
  assert.deepEqual(Object.keys(envelope.receipt).sort(), ['evidenceClass', 'fingerprint', 'productionSloClaim', 'qualityClaim', 'runId', 'schemaVersion', 'stages', 'status']);
  assert.deepEqual(envelope.receipt.stages.map(stage => [stage.stage, stage.documentCount]), [['small', 2], [1000, 1000], [10000, 10000]]);
  assert.deepEqual(envelope.receipt.stages.slice(0, 2), api.projectThousandQualificationReceipt({ small: source.small, thousand: source.thousand }).receipt.stages);
  assert.deepEqual(envelope.receipt.stages[2].evidence.documentIds, source.tenThousand.evidence.documentIds);
  assert.deepEqual(envelope.receipt.stages[2].counts, { targetDocuments: 10000, confirmedCreatedDocuments: 10000, confirmedPublishedDocuments: 10000 });
  assert.equal(new Set(envelope.receipt.stages.flatMap(stage => [...stage.evidence.documentIds, ...stage.negativeCorpus.documentIds])).size, 11005);
  assert.equal(api.MAX_RECEIPT_BYTES, 1024 * 1024);
  assert.ok(Buffer.byteLength(JSON.stringify(envelope)) < api.MAX_RECEIPT_BYTES);
  for (const forbidden of ['PRIVATE_SENTINEL', 'ports', 'fixtureHash', 'snapshots', 'observations', 'timings', 'credentials', 'rawPdf', 'environment', 'previousReport', 'plan']) assert.ok(!JSON.stringify(envelope).includes(forbidden), forbidden);
});

test('private report extras are ignored without traversal, stringification or mutation', () => {
  const source = chain();
  for (const name of names) {
    source[name].unknown = source[name];
    source[name].toJSON = () => { throw Error('PRIVATE_SENTINEL'); };
    Object.defineProperty(source[name], 'privateGetter', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } });
  }
  assert.deepEqual(projected(source), projected());
});

test('the verifier requires all independently trusted bindings and reports integrity without authenticity', () => {
  const envelope = projected();
  for (const input of [envelope, JSON.stringify(envelope), Buffer.from(JSON.stringify(envelope))]) {
    const checked = api.verifyTenThousandQualificationReceipt(input, expected(envelope));
    assert.deepEqual(checked, { receipt: envelope.receipt, integrityVerified: true, authenticityVerified: false });
    assert.notEqual(checked.receipt, envelope.receipt);
  }
  for (const key of ['code', 'corpus', 'runtime', 'runId', 'sha256']) {
    const missing = expected(envelope); delete missing[key];
    assert.throws(() => verified(envelope, missing), invalid);
    assert.throws(() => verified(envelope, { ...expected(envelope), [key]: key === 'runId' ? uuid(99999) : 'f'.repeat(key === 'code' ? 40 : 64) }), invalid);
  }
  assert.throws(() => api.verifyTenThousandQualificationReceipt(envelope), invalid);
  assert.throws(() => verified(envelope, { ...expected(envelope), secret: 'PRIVATE_SENTINEL' }), invalid);
  const tampered = structuredClone(envelope); tampered.receipt.stages[2].metrics.totalElapsedMs++;
  assert.throws(() => verified(tampered, expected(envelope)), invalid);
  const forged = seal(tampered.receipt);
  assert.throws(() => verified(forged, expected(envelope)), invalid);
  assert.equal(verified(forged).authenticityVerified, false);
});

test('exactly three source keys and every exported nested object reject secret-bearing extras', () => {
  const stageLocations = [r => r, r => r.fingerprint, r => r.metrics, r => r.counts, r => r.evidence, r => r.restart, r => r.restart.before, r => r.restart.after, r => r.restart.processes, r => r.negativeCorpus, r => r.negativeCorpus.counts, r => r.negativeCorpus.diagnostic];
  const locations = [r => r, r => r.fingerprint, ...[0, 1, 2].flatMap(index => stageLocations.map(locate => r => locate(r.stages[index])))];
  const valid = projected();
  for (const locate of locations) {
    const receipt = structuredClone(valid.receipt); locate(receipt).secret = 'PRIVATE_SENTINEL';
    assert.throws(() => verified(seal(receipt)), invalid);
  }
  const envelope = structuredClone(valid); envelope.path = '/private/PRIVATE_SENTINEL';
  assert.throws(() => verified(envelope), invalid);
  for (const key of ['secret', 'status', 'hundredThousand']) { const source = chain(); source[key] = 'PRIVATE_SENTINEL'; assert.throws(() => projected(source), invalid); }
  for (const name of names) { const source = chain(); delete source[name]; assert.throws(() => projected(source), invalid); }
});

test('every stage requires complete real successful counts, metrics and one rejected negative', () => {
  const changes = [r => { r.status = 'FAILED'; }, r => { r.status = 'RUNNING'; }, r => { r.evidenceClass = 'test-double'; }, r => { r.documentCount--; }, r => { r.metricQualification = 'partial-failed-stage'; }, r => { r.productionSloClaim = true; }, r => { r.qualityClaim = true; }, r => { r.counts.confirmedCreatedDocuments--; }, r => { r.counts.confirmedPublishedDocuments--; }, r => { delete r.metrics; }, r => { delete r.metrics.peakRssBytes; }, r => { delete r.metrics.databaseGrowthBytes; }, r => { r.metrics.totalElapsedMs = 0; }, r => { r.metrics.diskGrowthBytes++; }, r => { delete r.negativeCorpus; }, r => { r.negativeCorpus.status = 'FAILED'; }, r => { r.negativeCorpus.counts.confirmedPublishedDocuments = 1; }, r => { r.negativeCorpus.counts.confirmedRejectedDocuments = 0; }, r => { r.negativeCorpus.counts.confirmedHttp422Responses = 0; }, r => { r.negativeCorpus.documents[0].failureDiagnostic.httpStatus = 403; }, r => { r.negativeCorpus.documents[0].workerDiagnostic.failureCode = 'PRIVATE_SENTINEL'; }];
  for (const name of names) for (const change of changes) { const source = chain(); change(source[name]); assert.throws(() => projected(source), invalid); }
  for (const stage of ['10000', 'small', 1000, 100000]) { const source = chain(); source.tenThousand.stage = stage; assert.throws(() => projected(source), invalid); }
});

test('all 11005 positive and negative IDs must remain complete and disjoint across every stage', () => {
  const changes = [c => { c.tenThousand.evidence.documentIds.pop(); }, c => { c.tenThousand.evidence.documentIds.push(uuid(11006)); }, c => { c.tenThousand.evidence.documentIds[1] = c.tenThousand.evidence.documentIds[0]; }];
  for (const previous of ['small', 'thousand']) for (const kind of ['evidence', 'negativeCorpus']) {
    changes.push(c => { c.tenThousand.evidence.documentIds[500] = c[previous][kind].documentIds[0]; });
    changes.push(c => { const duplicate = c[previous][kind].documentIds[0]; c.tenThousand.negativeCorpus.documentIds[0] = duplicate; c.tenThousand.negativeCorpus.documents[0].documentId = duplicate; });
  }
  for (const change of changes) { const source = chain(); change(source); assert.throws(() => projected(source), invalid); }
});

test('both boundaries bind full fresh reports with identical retained datasets and in-memory previous pointers', () => {
  for (const name of ['thousand', 'tenThousand']) {
    for (const key of ['code', 'corpus', 'runtime']) {
      const source = chain(); source[name].fingerprint[key] = 'f'.repeat(key === 'code' ? 40 : 64);
      if (key === 'code') for (const part of ['before', 'after']) source[name].restart[part].sourceHead = source[name].fingerprint.code;
      assert.throws(() => projected(source), invalid);
    }
    for (const key of ['databaseIdentitySha256', 'storageIdentitySha256', 'fixtureHash']) { const source = chain(); for (const part of ['before', 'after']) source[name].restart[part][key] = '0'.repeat(64); assert.throws(() => projected(source), invalid); }
    const differentRun = chain(); differentRun[name].runId = uuid(99999); for (const part of ['before', 'after']) differentRun[name].restart[part].runId = differentRun[name].runId; assert.throws(() => projected(differentRun), invalid);
    const ports = chain(); for (const part of ['before', 'after']) ports[name].restart[part].ports.human++; assert.throws(() => projected(ports), invalid);
    const missing = chain(); delete missing[name].previousReport; assert.throws(() => projected(missing), invalid);
    const reconstructed = chain(); reconstructed[name].previousReport = structuredClone(reconstructed[name].previousReport); assert.throws(() => projected(reconstructed), invalid);
  }
  const source = chain(), old = api.projectThousandQualificationReceipt({ small: source.small, thousand: source.thousand });
  for (const index of [0, 1, 2]) { const replacement = chain(); replacement[names[index]] = projected().receipt.stages[index]; assert.throws(() => projected(replacement), invalid); }
  assert.throws(() => projected({ small: old.receipt.stages[0], thousand: old.receipt.stages[1], tenThousand: source.tenThousand }), invalid);
});

test('each boundary requires chronological stages and the exact ordered restarted process pair', () => {
  for (const [previous, current] of [['small', 'thousand'], ['thousand', 'tenThousand']]) {
    for (const change of [r => { r.restart.processes.before = [301, 401]; }, r => { r.restart.processes.before.reverse(); }, r => { r.restart.processes.after = [...r.restart.processes.before]; }, r => { r.restart.processes.after = [0, 203]; }, r => { r.restart.processes.after = [203, 203]; }, r => { r.restart.processes.after = [103, 203.5]; }, r => { r.finishedAt = r.startedAt; }]) { const source = chain(); change(source[current]); assert.throws(() => projected(source), invalid); }
    const overlap = chain(); overlap[current].startedAt = overlap[previous].startedAt; assert.throws(() => projected(overlap), invalid);
    const adjacent = chain(); adjacent[current].startedAt = adjacent[previous].finishedAt; assert.ok(projected(adjacent));
  }
});

test('rehashing cannot bypass three-stage order, identity, count, PID, timestamp or uniqueness checks', () => {
  const changes = [r => { r.stages.reverse(); }, r => { r.stages.pop(); }, r => { r.stages.splice(1, 1); }, r => { r.stages.push(structuredClone(r.stages[2])); }, r => { r.runId = uuid(99999); }, r => { r.fingerprint.runtime = 'f'.repeat(64); }, r => { r.stages[2].stage = 100000; }, r => { r.stages[2].counts.confirmedCreatedDocuments = 9999; }, r => { r.stages[2].counts.confirmedPublishedDocuments = 9999; }, r => { delete r.stages[2].metrics.peakRssBytes; }, r => { r.stages[2].evidence.documentIds[20] = r.stages[0].negativeCorpus.documentIds[0]; }, r => { r.stages[2].restart.processes.before = [301, 401]; }, r => { r.stages[2].startedAt = r.stages[1].startedAt; }, r => { for (const part of ['before', 'after']) r.stages[2].restart[part].storageIdentitySha256 = 'f'.repeat(64); }, r => { r.stages[2].negativeCorpus.counts.confirmedPublishedDocuments = 1; }];
  const valid = projected();
  for (const change of changes) { const receipt = structuredClone(valid.receipt); change(receipt); assert.throws(() => verified(seal(receipt)), invalid); }
});

test('secret values, exotic object shapes and inputs over one MiB fail closed', () => {
  const paths = [['runId'], ['fingerprint', 'code'], ['fingerprint', 'corpus'], ['fingerprint', 'runtime'], ['startedAt'], ['finishedAt'], ['evidence', 'documentIds', 0], ['negativeCorpus', 'documentIds', 0], ['restart', 'before', 'databaseIdentitySha256'], ['restart', 'before', 'storageIdentitySha256'], ['metrics', 'totalElapsedMs'], ['metrics', 'peakRssBytes']];
  for (const path of paths) for (const bad of ['PRIVATE_SENTINEL', '/private/report.json', null, undefined, Infinity, -1]) { const source = chain(); const parent = path.slice(0, -1).reduce((node, key) => node[key], source.tenThousand); parent[path.at(-1)] = bad; assert.throws(() => projected(source), invalid); }
  const envelope = projected();
  for (const input of ['PRIVATE_SENTINEL', '{', ' '.repeat(api.MAX_RECEIPT_BYTES + 1), Buffer.alloc(api.MAX_RECEIPT_BYTES + 1), null, [], {}, new Date()]) assert.throws(() => api.verifyTenThousandQualificationReceipt(input, expected(envelope)), invalid);
  const padded = JSON.stringify(envelope) + ' '.repeat(api.MAX_RECEIPT_BYTES - Buffer.byteLength(JSON.stringify(envelope)) + 1);
  for (const input of [padded, Buffer.from(padded)]) assert.throws(() => api.verifyTenThousandQualificationReceipt(input, expected(envelope)), invalid);
  for (const mutate of [e => { e.receipt.stages.secret = 'PRIVATE_SENTINEL'; }, e => { delete e.receipt.stages[2]; }, e => { e.receipt.stages[2].evidence.documentIds.secret = 'PRIVATE_SENTINEL'; }, e => { e.receipt.stages[2].restart.processes.after.toJSON = () => 'PRIVATE_SENTINEL'; }, e => { e.receipt[Symbol('secret')] = 'PRIVATE_SENTINEL'; }, e => { Object.defineProperty(e.receipt, 'status', { enumerable: true, get() { throw Error('PRIVATE_SENTINEL'); } }); }]) { const value = structuredClone(envelope); mutate(value); assert.throws(() => verified(value, expected(envelope)), invalid); }
});

test('small and thousand APIs preserve the existing canonical fixtures and reject the new schema', () => {
  const source = chain(), small = api.projectQualificationReceipt(source.small), thousand = api.projectThousandQualificationReceipt({ small: source.small, thousand: source.thousand });
  // Recorded from the unchanged small/thousand projectors before this extension.
  assert.equal(small.sha256, 'a7ad686267b1caf261a1dabe3ff3a7ede903ebc45507654c0845a99f49a3f0ee');
  assert.equal(thousand.sha256, '9009ac75a3e4e7f0446b44e399e5adf06de1283b0e34e3a466cda5769cd27308');
  assert.deepEqual(api.verifyQualificationReceipt(small, expected(small)).receipt, small.receipt);
  assert.deepEqual(api.verifyThousandQualificationReceipt(thousand, expected(thousand)).receipt, thousand.receipt);
  const current = projected();
  assert.throws(() => api.verifyQualificationReceipt(current, expected(current)), invalid);
  assert.throws(() => api.verifyThousandQualificationReceipt(current, expected(current)), invalid);
  for (const old of [small, thousand]) assert.throws(() => verified(old), invalid);
  assert.throws(() => api.projectQualificationReceipt(source.tenThousand), invalid);
  assert.throws(() => api.projectThousandQualificationReceipt(source), invalid);
});
