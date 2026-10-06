import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { inspect } from 'node:util';
import { browserDiagnostics, readBrowserDiagnostics, sanitizeBrowserDiagnostics, sanitizeBrowserPhases, MAX_BROWSER_REPORT_BYTES } from '../browser-diagnostics.mjs';

const report = (results, overrides = {}) => ({ suites: [{ title: 'never copied', specs: results.map(result => ({
  title: 'never copied', file: '/private/source/document-runtime.spec.ts', line: 12, column: 3,
  tests: [{ results: [result] }], ...overrides,
})) }] });

const phaseDiagnostics = (statuses, source = 'document-runtime.spec.ts') => sanitizeBrowserDiagnostics({
  availability: 'available', counts: { passed: statuses.filter(status => status === 'passed').length,
    failed: statuses.filter(status => ['failed', 'timedOut', 'interrupted'].includes(status)).length,
    skipped: statuses.filter(status => status === 'skipped').length },
  tests: statuses.map((status, index) => ({ source, line: index + 1, status, errorCategory: 'assertion' })), truncated: false,
});

test('phase budget retains a late persistence failure and only its allowlisted details', () => {
  const journey = phaseDiagnostics(Array(18).fill('passed'));
  const persistence = phaseDiagnostics(['passed', 'passed', 'failed', 'passed', 'passed'], 'working-version-editor-persistence.spec.ts');
  const secret = 'https://user:PRIVATE_SECRET@private.example/file?payload=PRIVATE_BYTES';
  persistence.tests[2] = { ...persistence.tests[2], column: 8, matcher: 'toBe', problemCode: 'REVISION_CONFLICT',
    message: secret, title: secret, selector: secret, url: secret, stack: secret, actual: secret, expected: secret };
  const actual = sanitizeBrowserPhases({ journey, persistence });
  assert.deepEqual(actual.persistence.tests.find(record => record.status === 'failed'), {
    source: 'working-version-editor-persistence.spec.ts', line: 3, column: 8, status: 'failed',
    errorCategory: 'assertion', matcher: 'toBe', problemCode: 'REVISION_CONFLICT',
  });
  assert.equal(actual.journey.tests.length + actual.persistence.tests.length, 20);
  assert.deepEqual(actual.persistence.tests.map(record => record.line), [1, 3]);
  assert.deepEqual(actual.journey.counts, journey.counts);
  assert.deepEqual(actual.persistence.counts, persistence.counts);
  assert.equal(actual.journey.truncated, false);
  assert.equal(actual.persistence.truncated, true);
  assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  assert.deepEqual(sanitizeBrowserPhases(actual), actual);
});

test('persistence failure displaces a passed journey record when journey fills all 20 slots', () => {
  const journey = phaseDiagnostics(Array(20).fill('passed'));
  const persistence = phaseDiagnostics(['failed'], 'persistence.spec.ts');
  const actual = sanitizeBrowserPhases({ journey, persistence });
  assert.deepEqual(actual.persistence.tests, persistence.tests);
  assert.deepEqual(actual.journey.tests, journey.tests.slice(0, 19));
  assert.equal(actual.journey.truncated, true);
  assert.equal(actual.persistence.truncated, false);
  assert.deepEqual(actual.journey.counts, journey.counts);
  assert.deepEqual(actual.persistence.counts, persistence.counts);
  assert.deepEqual(sanitizeBrowserPhases(actual), actual);
});

test('phase budget preserves failures from both phases and marks each discarded phase', () => {
  const journey = phaseDiagnostics([...Array(10).fill('passed'), ...Array(10).fill('failed')]);
  const persistence = phaseDiagnostics([...Array(10).fill('passed'), ...Array(10).fill('failed')], 'persistence.spec.ts');
  const actual = sanitizeBrowserPhases({ journey, persistence });
  for (const phase of ['journey', 'persistence']) {
    assert.deepEqual(actual[phase].tests, { journey, persistence }[phase].tests.slice(10));
    assert.deepEqual(actual[phase].counts, { journey, persistence }[phase].counts);
    assert.equal(actual[phase].truncated, true);
  }
  assert.equal(actual.journey.tests.length + actual.persistence.tests.length, 20);
  assert.deepEqual(sanitizeBrowserPhases(actual), actual);
});

test('more than 20 failures stay bounded in original phase order with accurate truncation', () => {
  const journey = phaseDiagnostics(Array(10).fill('failed'));
  const persistence = phaseDiagnostics(Array(15).fill('failed'), 'persistence.spec.ts');
  const actual = sanitizeBrowserPhases({ journey, persistence });
  assert.deepEqual(actual.journey, journey);
  assert.deepEqual(actual.persistence.tests, persistence.tests.slice(0, 10));
  assert.deepEqual(actual.persistence.counts, persistence.counts);
  assert.equal(actual.persistence.truncated, true);
  assert.equal(actual.journey.tests.length + actual.persistence.tests.length, 20);
  assert.deepEqual(sanitizeBrowserPhases(actual), actual);
});

test('failed and other unsuccessful records precede skipped and passed records in phase allocation', () => {
  const journey = phaseDiagnostics(Array(20).fill('skipped'));
  const persistence = phaseDiagnostics(['failed', 'timedOut', 'interrupted', 'unavailable', 'passed'], 'persistence.spec.ts');
  const actual = sanitizeBrowserPhases({ journey, persistence });
  assert.deepEqual(actual.persistence.tests, persistence.tests.slice(0, 4));
  assert.deepEqual(actual.journey.tests, journey.tests.slice(0, 16));
  assert.equal(actual.journey.truncated, true);
  assert.equal(actual.persistence.truncated, true);
  assert.deepEqual(actual.journey.counts, journey.counts);
  assert.deepEqual(actual.persistence.counts, persistence.counts);
  assert.deepEqual(sanitizeBrowserPhases(actual), actual);
});

test('passed and skipped phase allocation retains the old leading order and existing truncation', () => {
  for (const includeSkipped of [false, true]) {
    const journey = phaseDiagnostics(Array.from({ length: 18 }, (_, index) => includeSkipped && index % 2 ? 'skipped' : 'passed'));
    journey.truncated = true;
    const persistence = phaseDiagnostics(Array.from({ length: 5 }, (_, index) => includeSkipped && index % 2 ? 'skipped' : 'passed'), 'persistence.spec.ts');
    const actual = sanitizeBrowserPhases({ journey, persistence });
    assert.deepEqual(actual.journey, journey);
    assert.deepEqual(actual.persistence.tests, persistence.tests.slice(0, 2));
    assert.deepEqual(actual.persistence.counts, persistence.counts);
    assert.equal(actual.persistence.truncated, true);
    assert.deepEqual(sanitizeBrowserPhases(actual), actual);
    const unavailable = sanitizeBrowserPhases({ journey });
    assert.deepEqual(unavailable.journey, journey);
    assert.deepEqual(unavailable.persistence, sanitizeBrowserDiagnostics(undefined));
    assert.deepEqual(sanitizeBrowserPhases(unavailable), unavailable);
  }
});

test('journey progress emits only the last completed fixed milestone and is resanitized', () => {
  const input = report([{ status: 'timedOut', error: { message: 'Test timeout of 120000ms exceeded.' } }]);
  input.suites[0].specs[0].tests[0].annotations = [
    { type: 'runtime-completed', description: 'gui-loaded' },
    { type: 'runtime-completed', description: 'folder-selected' },
    { type: 'runtime-completed', description: 'https://credential@private.example/secret' },
    { type: 'other', description: 'download-received' },
  ];
  const actual = browserDiagnostics(input);
  assert.equal(actual.tests[0].lastCompletedStage, 'folder-selected');
  assert.equal(actual.tests[0].errorCategory, 'test-timeout');
  assert.ok(!JSON.stringify(actual).includes('private'));
  assert.equal(sanitizeBrowserDiagnostics({ ...actual, tests: [{ ...actual.tests[0], lastCompletedStage: 'secret' }] }).tests[0].lastCompletedStage, undefined);
  input.suites[0].specs[0].tests[0].annotations = Array.from({ length: 100 }, () => ({ type: 'other' }));
  input.suites[0].specs[0].tests[0].annotations.push({ type: 'runtime-completed', description: 'state-verified' });
  assert.equal(browserDiagnostics(input).tests[0].lastCompletedStage, undefined);
});

test('PDF operation milestones survive both privacy filters beside an allowlisted integrity code', () => {
  const stages = ['pdf-context-read', 'pdf-fixtures-read', 'pdf-base-created', 'pdf-base-detail-read',
    'pdf-base-published', 'pdf-published-detail-read', 'pdf-target-created', 'pdf-target-detail-read',
    'pdf-target-published', 'pdf-comparison-read', 'pdf-base-files-read', 'pdf-base-download-verified',
    'pdf-target-files-read', 'pdf-target-download-verified', 'pdf-gui-verified', 'pdf-shared-state-verified', 'pdf-snapshot-saved'];
  for (const stage of stages) {
    const input = report([{ status: 'failed', error: { value: { code: 'INTEGRITY_VIOLATION', detail: 'PRIVATE_BODY' } } }]);
    input.suites[0].specs[0].tests[0].annotations = [
      { type: 'runtime-completed', description: stage },
      { type: 'runtime-completed', description: 'pdf-PRIVATE_BODY' },
    ];
    const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.equal(actual.tests[0].lastCompletedStage, stage);
    assert.equal(actual.tests[0].problemCode, 'INTEGRITY_VIOLATION');
    assert.ok(!JSON.stringify(actual).includes('PRIVATE_BODY'));
  }
});

test('GUI初回登録の固定到達段階だけを既存の診断境界へ残す', () => {
  const stages = ['gui-initial-capabilities-verified', 'gui-initial-cancel-verified', 'gui-initial-created',
    'gui-initial-working-verified', 'gui-initial-published-shared', 'gui-initial-snapshot-saved'];
  for (const stage of stages) {
    const input = report([{ status: 'passed' }]);
    input.suites[0].specs[0].file = '/private/source/initial-registration.spec.ts';
    input.suites[0].specs[0].tests[0].annotations = [
      { type: 'runtime-completed', description: stage },
      { type: 'runtime-completed', description: 'gui-initial-PRIVATE_BODY' },
    ];
    const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.equal(actual.tests[0].source, 'initial-registration.spec.ts');
    assert.equal(actual.tests[0].lastCompletedStage, stage);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE_BODY'));
  }
  const failure = browserDiagnostics(report([{ status: 'failed', error: {
    message: 'Error: expect(value).toBe() failed',
    stack: 'at /private/source/initial-registration.spec.ts:12:3',
  } }]));
  assert.equal(failure.tests[0].source, 'initial-registration.spec.ts');
  assert.equal(failure.tests[0].line, 12);
});

test('GUI metadata受入は固定到達段階だけを公開し、値や理由を診断へ含めない', () => {
  const stages = ['gui-metadata-created', 'gui-metadata-cancel-verified', 'gui-metadata-working-verified',
    'gui-metadata-published-verified', 'gui-metadata-minor-verified', 'gui-metadata-noop-verified',
    'gui-metadata-snapshot-saved', 'gui-metadata-restart-verified',
    'gui-document-move-verified', 'gui-document-move-replay-verified'];
  for (const stage of stages) {
    const input = report([{ status: 'passed', stdout: ['PRIVATE_METADATA_VALUE'], attachments: [{ body: 'PRIVATE_REASON' }] }]);
    input.suites[0].specs[0].file = '/private/source/metadata-editor.spec.ts';
    input.suites[0].specs[0].tests[0].annotations = [
      { type: 'runtime-completed', description: stage },
      { type: 'runtime-completed', description: 'gui-metadata-PRIVATE_METADATA_VALUE' },
    ];
    const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.equal(actual.tests[0].source, 'metadata-editor.spec.ts');
    assert.equal(actual.tests[0].lastCompletedStage, stage);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
  const failure = browserDiagnostics(report([{ status: 'failed', error: {
    message: 'Error: expect(value).toBe() failed: PRIVATE_METADATA_VALUE',
    stack: 'at /private/source/metadata-editor.spec.ts:17:3',
  } }]));
  assert.equal(failure.tests[0].source, 'metadata-editor.spec.ts');
  assert.equal(failure.tests[0].line, 17);
  assert.ok(!JSON.stringify(failure).includes('PRIVATE'));
});

test('公開予約取消の固定到達段階とsourceだけを両診断境界へ残す', () => {
  const stages = ['gui-schedule-created', 'gui-schedule-dismissed', 'gui-schedule-cancelled',
    'gui-schedule-replaced', 'gui-schedule-final-state-saved', 'gui-schedule-restart-verified'];
  for (const stage of stages) {
    const input = report([{ status: 'passed' }]);
    input.suites[0].specs[0].file = '/private/source/document-schedule-cancellation.spec.ts';
    input.suites[0].specs[0].tests[0].annotations = [
      { type: 'runtime-completed', description: stage },
      { type: 'runtime-completed', description: 'gui-schedule-PRIVATE_BODY' },
    ];
    const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.equal(actual.tests[0].source, 'document-schedule-cancellation.spec.ts');
    assert.equal(actual.tests[0].lastCompletedStage, stage);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE_BODY'));
  }
  const failure = browserDiagnostics(report([{ status: 'failed', error: {
    message: 'Error: expect(value).toBe() failed',
    stack: 'at /private/source/document-schedule-cancellation.spec.ts:12:3',
  } }]));
  assert.equal(failure.tests[0].source, 'document-schedule-cancellation.spec.ts');
  assert.equal(failure.tests[0].line, 12);
});

test('GUI取下げ・公開終了と再起動の固定到達段階だけを既存の診断境界へ残す', () => {
  const stages = ['gui-lifecycle-fixture-ready', 'gui-lifecycle-cancel-verified', 'gui-withdraw-fallback-verified',
    'gui-withdraw-null-verified', 'gui-publication-end-verified', 'gui-lifecycle-snapshot-saved', 'gui-lifecycle-restart-verified'];
  for (const source of ['lifecycle-operations.spec.ts', 'lifecycle-operations-persistence.spec.ts']) {
    for (const stage of stages) {
      const input = report([{ status: 'passed' }], { file: `/private/source/${source}` });
      input.suites[0].specs[0].tests[0].annotations = [
        { type: 'runtime-completed', description: stage },
        { type: 'runtime-completed', description: 'gui-lifecycle-PRIVATE_BODY' },
      ];
      const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
      assert.equal(actual.tests[0].source, source);
      assert.equal(actual.tests[0].lastCompletedStage, stage);
      assert.ok(!JSON.stringify(actual).includes('PRIVATE_BODY'));
    }
    const failure = browserDiagnostics(report([{ status: 'failed', error: {
      message: 'Error: expect(value).toBe() failed', stack: `at /private/source/${source}:12:3`,
    } }]));
    assert.equal(failure.tests[0].source, source);
    assert.equal(failure.tests[0].line, 12);
  }
});

test('classifies strict-locator and emits only allowlisted failure location/matcher', () => {
  const result = browserDiagnostics(report([{ status: 'failed', error: {
    message: 'Error: expect(locator).toBeVisible() failed: strict mode violation: PRIVATE_SELECTOR',
    location: { file: '/private/source/document-runtime.spec.ts', line: 53, column: 9 },
  } }]));
  assert.deepEqual(result.counts, { passed: 0, failed: 1, skipped: 0 });
  assert.deepEqual(result.tests, [{ source: 'document-runtime.spec.ts', line: 53, column: 9,
    status: 'failed', errorCategory: 'strict-locator', matcher: 'toBeVisible' }]);
});

test('classifies timeouts and bounded HTTP assertions without raw messages', () => {
  const result = browserDiagnostics(report([
    { status: 'failed', error: { message: 'locator.click: Timeout 15000ms exceeded.' } },
    { status: 'timedOut', error: { message: 'Test timeout of 120000ms exceeded.' } },
    { status: 'failed', error: { message: 'Error: expect(received).toBe(expected)\nExpected: 200\nReceived: 403', snippet: 'expect(response.status()).toBe(200)', stack: 'at /secret/support.ts:31:7' } },
    { status: 'passed', stdout: ['PRIVATE'] }, { status: 'skipped' },
  ]));
  assert.deepEqual(result.counts, { passed: 1, failed: 3, skipped: 1 });
  assert.equal(result.tests[0].errorCategory, 'locator-timeout');
  assert.equal(result.tests[1].errorCategory, 'test-timeout');
  assert.deepEqual(result.tests[2], { source: 'support.ts', line: 31, column: 7, status: 'failed', errorCategory: 'HTTP-status-assertion', matcher: 'toBe', expected: 200, actual: 403 });
});

test('malicious raw fields cannot appear and unsupported numbers/matchers/problem codes are omitted', () => {
  const secret = 'https://u:credential@secret.example/private';
  const error = { message: `Error: expect(received).toSendSecret() ${secret}\nExpected: 199\nReceived: 600`, stack: secret,
    snippet: `expect(response.status()) ${secret}`, value: secret, location: { file: secret, line: -1, column: 0 },
    problem: { code: secret }, matcherResult: { name: secret, expected: secret, actual: secret } };
  const input = report([{ status: 'failed', error, stdout: [secret], stderr: [secret], attachments: [{ path: secret, body: secret }], steps: [{ title: secret }] }], { title: secret, file: secret, id: secret });
  const result = browserDiagnostics(input);
  const output = JSON.stringify(result);
  for (const fragment of ['credential', 'secret.example', 'private', 'toSendSecret', 'https:', '199', '600']) assert.ok(!output.includes(fragment));
  assert.equal(result.tests[0].source, undefined); assert.equal(result.tests[0].expected, undefined); assert.equal(result.tests[0].matcher, undefined);
  assert.deepEqual(sanitizeBrowserDiagnostics({ ...result, title: secret, tests: [{ ...result.tests[0], errorCategory: secret, matcher: secret, source: secret, line: secret, problemCode: secret }] }).tests,
    [{ status: 'failed', errorCategory: 'unavailable' }]);
});

test('only a known Problem.code and safe valid source integers can be emitted', () => {
  const result = browserDiagnostics(report([{ status: 'failed', error: { message: 'assertion failed', problem: { code: 'FORBIDDEN' }, location: { file: 'persistence.spec.ts', line: 22, column: 4 } } }]));
  assert.equal(result.tests[0].problemCode, 'FORBIDDEN'); assert.equal(result.tests[0].source, 'persistence.spec.ts');
  assert.equal(result.tests[0].line, 22);
});

test('output and traversal are bounded and counts describe only observed results', () => {
  const input = report(Array.from({ length: 30 }, () => ({ status: 'failed', error: { message: 'assertion failed' } })));
  const result = browserDiagnostics(input);
  assert.equal(result.tests.length, 20); assert.equal(result.counts.failed, 30); assert.equal(result.truncated, true);
  const cycle = { suites: [] }; cycle.suites.push(cycle);
  assert.equal(browserDiagnostics(cycle).truncated, true);
  assert.equal(browserDiagnostics({ suites: [] }).availability, 'available');
  assert.equal(browserDiagnostics(null).availability, 'unavailable');
});

test('reader accepts only owned fixed phase result files and rejects symlinks/oversize/malformed input', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'runtime-diagnostics-'));
  try {
    await mkdir(join(directory, 'browser-journey'));
    const file = join(directory, 'browser-journey', 'results.json');
    await writeFile(file, JSON.stringify(report([{ status: 'passed' }])));
    assert.equal((await readBrowserDiagnostics(directory, 'journey')).counts.passed, 1);
    assert.equal((await readBrowserDiagnostics(directory, '../browser-journey')).availability, 'unavailable');
    await rm(file); await symlink('/does/not/matter', file);
    assert.equal((await readBrowserDiagnostics(directory, 'journey')).availability, 'unavailable');
    await rm(file); await writeFile(file, '{malformed');
    assert.equal((await readBrowserDiagnostics(directory, 'journey')).availability, 'unavailable');
    await writeFile(file, ' '.repeat(MAX_BROWSER_REPORT_BYTES + 1));
    assert.equal((await readBrowserDiagnostics(directory, 'journey')).availability, 'unavailable');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('top-level collection/config errors are present with bounded safe global locations and categories', () => {
  const secret = 'postgres://user:credential@secret.example/private';
  const result = browserDiagnostics({ suites: [], errors: [{ message: `Cannot import ${secret}`, stack: `Error ${secret}\n at /private/support.ts:19:2`, snippet: secret }] });
  assert.equal(result.globalErrorsPresent, true); assert.equal(result.globalErrorCount, 1);
  assert.deepEqual(result.tests, [{ scope: 'global', source: 'support.ts', line: 19, column: 2, status: 'failed', errorCategory: 'unavailable' }]);
  assert.ok(!JSON.stringify(result).includes('credential'));
  const many = browserDiagnostics({ suites: report([{ status: 'passed' }]).suites, errors: Array(30).fill({ message: secret }) });
  assert.equal(many.tests.length, 20); assert.equal(many.globalErrorCount, 30); assert.equal(many.truncated, true);
});

test('non-Error generated-client values expose only allowlisted Problem.code', () => {
  const secret = 'https://user:credential@secret.example/private';
  for (const value of [{ code: 'FORBIDDEN', detail: secret, instance: secret }, JSON.stringify({ code: 'REVISION_CONFLICT', detail: secret })]) {
    const result = browserDiagnostics(report([{ status: 'failed', error: { value } }]));
    assert.ok(['FORBIDDEN', 'REVISION_CONFLICT'].includes(result.tests[0].problemCode));
    assert.ok(!JSON.stringify(result).includes('credential'));
  }
  for (const value of [secret, '{bad json', JSON.stringify({ code: secret }), 'x'.repeat(20000)]) {
    const result = browserDiagnostics(report([{ status: 'failed', error: { value } }]));
    assert.equal(result.tests[0].problemCode, undefined); assert.ok(!JSON.stringify(result).includes('credential'));
  }
});


test('known util.inspect serialization exposes only the real top-level allowlisted code', () => {
  const secret = 'https://user:credential@secret.example/private';
  const values = [
    inspect({ type: secret, title: 'Forbidden', status: 403, code: 'FORBIDDEN', detail: secret, traceId: secret }),
    inspect({ status: 409, code: 'REVISION_CONFLICT', detail: secret }, { breakLength: Infinity }),
  ];
  for (const value of values) {
    const result = browserDiagnostics(report([{ status: 'failed', error: { value } }]));
    assert.ok(['FORBIDDEN', 'REVISION_CONFLICT'].includes(result.tests[0].problemCode));
    assert.ok(!JSON.stringify(result).includes('credential'));
  }
  for (const value of [inspect({ detail: ", code: 'FORBIDDEN'", code: 'UNKNOWN_CODE' }),
    inspect({ nested: { code: 'FORBIDDEN' } }), "{ code: 'FORBIDDEN", inspect({ code: secret })]) {
    const result = browserDiagnostics(report([{ status: 'failed', error: { value } }]));
    assert.equal(result.tests[0].problemCode, undefined); assert.ok(!JSON.stringify(result).includes('credential'));
  }
});

test('startup attachment emits only bounded fixed categories and numbers and is resanitized', () => {
  const secret='https://credential@secret.example/private';
  const startup={rootChildren:1,documentStatus:200,scriptResponses:2,scriptFailures:1,apiResponses:3,apiFailures:0,
    pageErrors:['require-undefined',secret],cspViolations:2,consoleErrors:1,details:secret};
  const attachment={name:'runtime-startup.json',contentType:'application/json',body:Buffer.from(JSON.stringify(startup)).toString('base64')};
  const result=browserDiagnostics(report([{status:'failed',attachments:[attachment]}]));
  assert.equal(result.tests[0].startup.documentStatus,200);
  assert.deepEqual(result.tests[0].startup.pageErrors,['require-undefined','other']);
  assert.ok(!JSON.stringify(result).includes('secret'));
  const sanitized=sanitizeBrowserDiagnostics({...result,tests:[{...result.tests[0],startup:{...startup,rootChildren:-1,documentStatus:999}}]});
  assert.equal(sanitized.tests[0].startup.rootChildren,0);
  assert.equal(sanitized.tests[0].startup.documentStatus,0);
  assert.ok(!JSON.stringify(sanitized).includes('secret'));
  const oversized={...attachment,body:'A'.repeat(9000)};
  assert.equal(browserDiagnostics(report([{status:'failed',attachments:[oversized]}])).tests[0].startup,undefined);
});

test('action snapshots distinguish unavailable DOM and disclose only fixed counts and API route families', () => {
  const secret = 'https://credential@private.example/path';
  const startup = { domObservation: 'unavailable', apiEvents: [
    { route: 'folder-children', status: 200, url: secret }, { route: secret, status: 999 }],
    uiSnapshots: [{ stage: 'before-folder-click', availability: 'available', viewportWidth: 1280,
      viewportHeight: 720, rootChildren: 1, folderRegionCount: 1, sharedFolderButtonCount: 1, tableCount: 1,
      alertCount: 0, inViewportCount: 0, receivesPointerCount: 0, disabledCount: 0, hiddenAncestorCount: 0,
      secret }, { stage: 'test-end', availability: 'unavailable', rootChildren: 999, secret },
      { stage: secret, availability: 'available' }] };
  const attachment = { name: 'runtime-startup.json', contentType: 'application/json', body: Buffer.from(JSON.stringify(startup)).toString('base64') };
  const result = browserDiagnostics(report([{ status: 'timedOut', attachments: [attachment] }]));
  const evidence = result.tests[0].startup;
  assert.equal(evidence.domObservation, 'unavailable');
  assert.deepEqual(evidence.apiEvents, [{ route: 'folder-children', status: 200 }, { route: 'other', status: 0 }]);
  assert.equal(evidence.uiSnapshots.length, 2);
  assert.equal(evidence.uiSnapshots[0].sharedFolderButtonCount, 1);
  assert.deepEqual(evidence.uiSnapshots[1], { stage: 'test-end', availability: 'unavailable' });
  assert.ok(!JSON.stringify(result).includes('private'));
  assert.deepEqual(sanitizeBrowserDiagnostics(result), result);
});

test('actual Japanese font assertion emits only its fixed selection receipt and is resanitized', () => {
  const input = report([{ status: 'passed' }]);
  input.suites[0].specs[0].tests[0].annotations = [
    { type: 'runtime-font', description: 'kosugi-regular-japanese-heading-body' },
    { type: 'runtime-font', description: 'PRIVATE_FONT_PATH' },
  ];
  const actual = browserDiagnostics(input);
  assert.equal(actual.tests[0].fontSelection, 'kosugi-regular-japanese-heading-body');
  assert.equal(sanitizeBrowserDiagnostics(actual).tests[0].fontSelection, 'kosugi-regular-japanese-heading-body');
  assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  assert.equal(sanitizeBrowserDiagnostics({ ...actual, tests: [{ ...actual.tests[0], fontSelection: 'PRIVATE' }] }).tests[0].fontSelection, undefined);
  input.suites[0].specs[0].tests[0].annotations = [{ type: 'other', description: 'kosugi-regular-japanese-heading-body' }];
  assert.equal(browserDiagnostics(input).tests[0].fontSelection, undefined);
});

test('timestamp layout receipt requires a passed test and stays fixed through both privacy boundaries', () => {
  const input = report([{ status: 'passed' }]);
  input.suites[0].specs[0].file = '/private/source/timestamp-layout.spec.ts';
  input.suites[0].specs[0].tests[0].annotations = [
    { type: 'runtime-timestamp-layout', description: 'long-iana-both-folds-1280-1440' },
    { type: 'runtime-timestamp-layout', description: 'PRIVATE_DOM_TEXT' },
  ];
  const actual = browserDiagnostics(input);
  assert.equal(actual.tests[0].source, 'timestamp-layout.spec.ts');
  assert.equal(actual.tests[0].timestampLayout, 'long-iana-both-folds-1280-1440');
  assert.deepEqual(sanitizeBrowserDiagnostics(actual), actual);
  assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  for (const status of ['failed', 'skipped', 'timedOut', 'unavailable']) {
    assert.equal(sanitizeBrowserDiagnostics({ ...actual, tests: [{ ...actual.tests[0], status }] }).tests[0].timestampLayout, undefined);
    input.suites[0].specs[0].tests[0].results[0].status = status;
    assert.equal(browserDiagnostics(input).tests[0].timestampLayout, undefined);
  }
  assert.equal(sanitizeBrowserDiagnostics({ ...actual, tests: [{ ...actual.tests[0], timestampLayout: 'PRIVATE' }] }).tests[0].timestampLayout, undefined);
  input.suites[0].specs[0].tests[0].results[0].status = 'passed';
  input.suites[0].specs[0].tests[0].annotations = [{ type: 'other', description: 'long-iana-both-folds-1280-1440' }];
  assert.equal(browserDiagnostics(input).tests[0].timestampLayout, undefined);
});

test('WORKING複数原本編集は有限の段階とsourceだけを公開診断へ残す', () => {
  const stages = ['gui-working-initial-updated', 'gui-working-manifest-ready', 'gui-working-cancel-verified',
    'gui-working-created', 'gui-working-updated', 'gui-working-publication-preserved',
    'gui-working-published', 'gui-working-snapshot-saved', 'gui-working-restart-verified'];
  for (const source of ['working-version-editor.spec.ts', 'working-version-editor-persistence.spec.ts']) {
    for (const stage of stages) {
      const input = report([{ status: 'passed' }], { file: `/private/${source}` });
      input.suites[0].specs[0].tests[0].annotations = [
        { type: 'runtime-completed', description: stage },
        { type: 'runtime-completed', description: 'gui-working-PRIVATE_CONTENT' },
      ];
      const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
      assert.equal(actual.tests[0].source, source);
      assert.equal(actual.tests[0].lastCompletedStage, stage);
      assert.ok(!JSON.stringify(actual).includes('PRIVATE_CONTENT'));
    }
    const failure = browserDiagnostics(report([{ status: 'failed', error: {
      message: 'Error: expect(value).toBe() failed', stack: `at /private/${source}:12:3`,
    } }]));
    assert.equal(failure.tests[0].source, source);
  }
});


test('working loss fixed failure codes survive both privacy boundaries without raw error details', () => {
  const codes = ['configuration', 'admission', 'unarmed-retry', 'payload', 'upstream-status', 'upstream-result',
    'retry-payload', 'retry-result', 'unrecovered', 'observation-window', 'upstream-transport'];
  const privateValue = 'https://user:PRIVATE_SECRET@private.example/file?payload=PRIVATE_BYTES';
  for (const code of codes) {
    const input = report([{ status: 'failed', error: { message: `Error: [working-loss:${code}] ${privateValue}`, stack: privateValue,
      headers: privateValue, payload: privateValue, cause: privateValue } }], { file: 'working-version-editor.spec.ts' });
    const result = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.deepEqual(result.tests[0], { source: 'working-version-editor.spec.ts', line: 12, column: 3,
      status: 'failed', errorCategory: 'response-loss', responseLossCode: code });
    assert.ok(!JSON.stringify(result).includes('PRIVATE'));
  }
  for (const message of [`Error: [working-loss:${privateValue}]`, `Error: PRIVATE [working-loss:unarmed-retry]`,
    '[working-loss:unknown] PRIVATE', '[working-loss:admission]PRIVATE', '[working-loss:admission-extra] PRIVATE']) {
    const result = browserDiagnostics(report([{ status: 'failed', error: { message } }]));
    assert.equal(result.tests[0].responseLossCode, undefined);
    assert.ok(!JSON.stringify(result).includes('PRIVATE'));
  }
  const dirty = { availability: 'available', tests: [{ status: 'failed', errorCategory: 'response-loss',
    responseLossCode: privateValue, phase: privateValue, receipt: { payload: privateValue } }] };
  assert.deepEqual(sanitizeBrowserDiagnostics(dirty).tests, [{ status: 'failed', errorCategory: 'response-loss' }]);
  dirty.tests[0] = { status: 'passed', responseLossCode: 'unarmed-retry', errorCategory: 'response-loss' };
  assert.deepEqual(sanitizeBrowserDiagnostics(dirty).tests, [{ status: 'passed' }]);
});

test('working loss secondary teardown errors never replace the primary test error', () => {
  const primary = { message: 'Error: expect(locator).toBeVisible() failed: PRIVATE' };
  const secondary = { message: 'Error: [working-loss:unrecovered] Mutation is not recovered' };
  for (const result of [{ status: 'failed', error: primary, errors: [primary, secondary] },
    { status: 'failed', errors: [primary, secondary] }]) {
    const actual = browserDiagnostics(report([result])).tests[0];
    assert.equal(actual.errorCategory, 'assertion');
    assert.equal(actual.responseLossCode, undefined);
  }
  const sticky = { message: 'Error: [working-loss:unarmed-retry] Unexpected mutation request or automatic retry' };
  const actual = browserDiagnostics(report([{ status: 'failed', error: sticky, errors: [sticky, secondary] }])).tests[0];
  assert.equal(actual.responseLossCode, 'unarmed-retry');
});

test('working loss save milestones are finite and survive both privacy boundaries', () => {
  for (const suffix of ['armed', 'save-clicked', 'dropped', 'headers-observed', 'unknown-visible', 'retry-armed', 'recovered']) {
    const stage = `gui-working-loss-${suffix}`;
    const input = report([{ status: 'failed' }], { file: 'working-version-editor.spec.ts' });
    input.suites[0].specs[0].tests[0].annotations = [
      { type: 'runtime-completed', description: stage },
      { type: 'runtime-completed', description: 'gui-working-loss-PRIVATE_FILENAME' },
    ];
    const actual = sanitizeBrowserDiagnostics(browserDiagnostics(input));
    assert.equal(actual.tests[0].lastCompletedStage, stage);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
});
