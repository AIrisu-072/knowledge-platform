import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { inspect } from 'node:util';
import { browserDiagnostics, readBrowserDiagnostics, sanitizeBrowserDiagnostics, MAX_BROWSER_REPORT_BYTES } from '../browser-diagnostics.mjs';

const report = (results, overrides = {}) => ({ suites: [{ title: 'never copied', specs: results.map(result => ({
  title: 'never copied', file: '/private/source/document-runtime.spec.ts', line: 12, column: 3,
  tests: [{ results: [result] }], ...overrides,
})) }] });

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
