// Deterministic synthetic reports exercise schema validation only. These are
// test data, never execution evidence or inputs for admission of a real stage.
export const runId = '12345678-1234-4abc-8abc-123456789abc';
export const fingerprint = { code: 'a'.repeat(40), corpus: 'b'.repeat(64), runtime: 'c'.repeat(64) };
export const uuid = n => `0198eada-1234-7000-8000-${n.toString(16).padStart(12, '0')}`;
export const names = ['small', 'thousand', 'tenThousand', 'hundredThousand'];

function stageReport(stage, documentCount, firstId, index) {
  const identity = {
    runId, sourceHead: fingerprint.code, databaseIdentitySha256: 'd'.repeat(64), storageIdentitySha256: 'e'.repeat(64),
    databaseStorageMode: 'owned-ext4', databaseStorageIdentitySha256: '0'.repeat(64),
    fixtureHash: 'f'.repeat(64), ports: { human: 41001, agent: 41002, postgres: 41003, proxy: 41004 },
  };
  const documentIds = Array.from({ length: documentCount }, (_, i) => uuid(firstId + i));
  const negativeId = uuid(firstId + documentCount);
  const sampledDocumentIds = documentCount === 2 ? [...documentIds]
    : [documentIds[0], documentIds[Math.floor(documentCount / 2)], documentIds.at(-1)];
  return {
    schemaVersion: 1, evidenceClass: 'owned-real-process', status: 'SUCCEEDED', runId,
    stage, documentCount, fingerprint: { ...fingerprint },
    startedAt: new Date(Date.UTC(2026, 9, 8, index * 2)).toISOString(),
    finishedAt: new Date(Date.UTC(2026, 9, 8, index * 2 + 1)).toISOString(),
    metricQualification: 'complete-stage', productionSloClaim: false, qualityClaim: false,
    metrics: { totalElapsedMs: 3_600_000.25, peakRssBytes: 4096, diskGrowthBytes: 30,
      storageAllocatedGrowthBytes: 10, databaseAllocatedGrowthBytes: 20, databaseGrowthBytes: 10, storageGrowthBytes: 5,
      rssMeasurement: 'PRIVATE_SENTINEL' },
    counts: { targetDocuments: documentCount, confirmedCreatedDocuments: documentCount, confirmedPublishedDocuments: documentCount },
    evidence: { status: 'AWAITING_RESTART', documentIds, sampledDocumentIds,
      snapshots: Object.fromEntries(sampledDocumentIds.map(id => [id, { private: 'PRIVATE_SENTINEL' }])),
      uniqueOriginalCount: 2, syntheticRepetitionCount: documentCount, versionCount: documentCount + 1, contentQualityClaim: false },
    restart: { identityRetained: true, processesReplaced: true, before: identity, after: structuredClone(identity),
      processes: { before: [100 + index, 200 + index], after: [101 + index, 201 + index] } },
    negativeCorpus: { status: 'SUCCEEDED', contentQualityClaim: false, documentIds: [negativeId],
      counts: { targetDocuments: 1, confirmedCreatedDocuments: 1, confirmedHttp422Responses: 1, confirmedRejectedDocuments: 1, confirmedPublishedDocuments: 0 },
      documents: [{ documentId: negativeId, failureDiagnostic: { operation: 'publish', httpStatus: 422, problemCode: 'BUSINESS_RULE_REJECTED' },
        workerDiagnostic: { status: 'worker-failure', failureCode: 'unsupported_semantic_construct', qualification: false }, snapshot: 'PRIVATE_SENTINEL' }] },
    plan: { privatePath: '/private/PRIVATE_SENTINEL' }, observations: ['PRIVATE_SENTINEL'], credentials: 'PRIVATE_SENTINEL', rawPdf: Buffer.from('PRIVATE_SENTINEL'),
  };
}

export function localScaleReports() {
  const small = stageReport('small', 2, 1, 0);
  const thousand = stageReport(1000, 1000, 4, 1);
  const tenThousand = stageReport(10000, 10000, 1005, 2);
  const hundredThousand = stageReport(100000, 100000, 11006, 3);
  thousand.previousReport = small;
  tenThousand.previousReport = thousand;
  hundredThousand.previousReport = tenThousand;
  const cleanup = { ownedCleanupSucceeded: true, records: [
    { pid: 104, result: 'gracefully-stopped' }, { pid: 204, result: 'gracefully-stopped' },
    { resource: 'owned-database-proxy', result: 'closed' }, { resource: 'owned-postgres', result: 'removed' },
  ] };
  return { small, thousand, tenThousand, hundredThousand, cleanup };
}
