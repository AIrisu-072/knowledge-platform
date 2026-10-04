import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { browserFailureDiagnostics, readBrowserFailureDiagnostics } from './browser-diagnostics.mjs';

const titles = {
  journey: '実2名UIで根拠・候補・3種の人間判断を選択提出し、差戻後の新試行を非公開で再提出する',
  persistence: '両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する',
};
const report = (result, phase = 'journey', spec = {}) => JSON.stringify({ suites: [{ specs: [{
  title: titles[phase], file: `${phase}.spec.ts`, line: 10, column: 1, ...spec, tests: [{ results: [result] }],
}] }] });

test('standard JSON timeout retains only the current result action annotation without inferring completion', () => {
  const raw = JSON.parse(report({ status: 'timedOut', error: { message: 'Test timeout of 120000ms exceeded.' },
    annotations: [{ type: 'organization-stage', description: 'source-file-select', detail: 'PRIVATE' }] }));
  raw.suites[0].specs[0].tests[0].annotations = [{ type: 'organization-stage', description: 'journey-setup' }];
  assert.deepEqual(browserFailureDiagnostics(JSON.stringify(raw), 'journey'), { phase: 'journey', availability: 'available', failure: {
    test: 'journey', status: 'timedOut', errorCategory: 'test-timeout', currentAction: 'source-file-select',
  } });
});

test('all thirty-five fixed action names survive the closed projection', () => {
  const stages = ['journey-setup', 'office-navigation', 'sales-navigation', 'document-navigation', 'task-navigation',
    'draft-save', 'source-read', 'evidence-module', 'source-document-select', 'source-file-select', 'evidence-input',
    'evidence-submit', 'finding-input', 'finding-submit', 'decision-select', 'decision-input', 'decision-preview',
    'decision-confirm', 'visibility-verify', 'submit-preview', 'submit-selection', 'submit-confirm', 'office-claim',
    'return-preview', 'return-confirm', 'sales-reclaim', 'resubmit', 'office-reclaim', 'final-verify', 'persistence-verify', 'agent-module', 'agent-input', 'agent-request', 'agent-result', 'agent-replay'];
  assert.equal(stages.length, 35);
  for (const description of stages) {
    const raw = report({ status: 'timedOut', annotations: [{ type: 'organization-stage', description }] });
    assert.equal(browserFailureDiagnostics(raw, 'journey').failure.currentAction, description);
  }
});

test('unknown, malformed, excessive and stale test-level annotations cannot disclose or invent a current action', () => {
  const secret = 'https://PRIVATE:credential@private.example/body';
  const known = { type: 'organization-stage', description: 'source-read' };
  for (const annotations of [undefined, secret, [null], [{ type: secret, description: 'source-read' }],
    [{ type: 'organization-stage', description: secret }], [known, { type: 'organization-stage', description: secret }],
    Array(33).fill(known)]) {
    const raw = JSON.parse(report({ status: 'timedOut', annotations }));
    raw.suites[0].specs[0].tests[0].annotations = [known];
    const actual = browserFailureDiagnostics(JSON.stringify(raw), 'journey');
    assert.equal(actual.failure.currentAction, undefined);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
});

test('failed assertion reports exact known failure location rather than test declaration', () => {
  const actual = browserFailureDiagnostics(report({ status: 'failed', error: {
    message: 'Error: expect(locator).toBeVisible() failed: strict mode violation: PRIVATE_LOCATOR',
    location: { file: '/private/checkout/support.ts', line: 192, column: 7 },
  } }), 'journey');
  assert.deepEqual(actual, { phase: 'journey', availability: 'available', failure: {
    test: 'journey', source: 'support.ts', line: 192, column: 7,
    status: 'failed', errorCategory: 'strict-locator', matcher: 'toBeVisible',
  } });
});

test('standard reporter errorLocation and known stack frames retain exact numeric coordinates', () => {
  for (const phase of ['journey', 'persistence']) {
    const error = { message: 'locator.selectOption: Timeout 15000ms exceeded.', stack: `Error: PRIVATE\n    at /private/${phase}.spec.ts:73:11` };
    for (const result of [{ status: 'failed', error }, { status: 'failed', error: {}, errorLocation: { file: `${phase}.spec.ts`, line: 73, column: 11 } }]) {
      const actual = browserFailureDiagnostics(report(result, phase), phase).failure;
      assert.equal(actual.source, `${phase}.spec.ts`);
      assert.equal(actual.line, 73);
      assert.equal(actual.column, 11);
      assert.equal(actual.test, phase);
    }
  }
});

test('all raw messages, values, URLs, unknown fields, steps and attachments stay private', () => {
  const secret = 'https://user:PRIVATE_CREDENTIAL@private.example/body';
  const raw = report({ status: 'failed', stdout: [secret], stderr: [secret], steps: [{ title: secret }], attachments: [{ body: secret, path: secret }],
    error: { message: secret, stack: secret, body: secret, value: secret, snippet: secret,
      location: { file: secret, line: secret, column: -1 }, matcherResult: { name: secret, expected: secret, actual: secret } },
    unknown: secret }, 'journey', { title: secret, file: secret });
  assert.deepEqual(browserFailureDiagnostics(raw, 'journey'), { phase: 'journey', availability: 'available', failure: { status: 'failed', errorCategory: 'unavailable' } });
});

test('categories and matchers are closed and never disclose actual or expected values', () => {
  for (const [status, message, category, matcher] of [
    ['timedOut', 'Test timeout of 120000ms exceeded. PRIVATE', 'test-timeout'],
    ['failed', 'locator.click: Timeout 15000ms exceeded. PRIVATE', 'locator-timeout'],
    ['failed', 'expect(received).toEqual(expected) Expected: PRIVATE Received: PRIVATE', 'assertion', 'toEqual'],
    ['interrupted', 'PRIVATE', 'unavailable'],
  ]) {
    const actual = browserFailureDiagnostics(report({ status, error: { message } }), 'journey').failure;
    assert.equal(actual.status, status);
    assert.equal(actual.errorCategory, category);
    assert.equal(actual.matcher, matcher);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
});

test('global failures are bounded and never return unknown filenames or invalid coordinates', () => {
  const raw = JSON.stringify({ errors: [{ message: 'PRIVATE', location: { file: 'support.ts', line: 0, column: 1_000_001 } }, { message: 'PRIVATE' }] });
  assert.deepEqual(browserFailureDiagnostics(raw, 'journey'), { phase: 'journey', availability: 'available', failure: { source: 'support.ts', status: 'failed', errorCategory: 'unavailable' } });
});

test('missing, malformed, truncated, oversized and failure-free reports are explicitly unavailable', () => {
  for (const raw of [undefined, '{malformed', '{"suites":[', 'null', '{}', report({ status: 'passed' }), ' '.repeat(8 * 1024 * 1024 + 1), JSON.stringify({ suites: Array(1001).fill({}) })]) {
    assert.deepEqual(browserFailureDiagnostics(raw, 'journey'), { phase: 'journey', availability: 'unavailable' });
  }
  assert.deepEqual(browserFailureDiagnostics(report({ status: 'failed' }), 'PRIVATE'), { availability: 'unavailable' });
});

test('reader uses only private fixed-phase JSON and rejects missing, malformed and symlink reports', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'organization-diagnostics-'));
  try {
    await mkdir(join(directory, 'browser-journey'), { mode: 0o700 });
    const path = join(directory, 'browser-journey', 'results.json');
    assert.equal((await readBrowserFailureDiagnostics(directory, 'journey')).availability, 'unavailable');
    await writeFile(path, report({ status: 'failed', error: { message: 'PRIVATE' } }), { mode: 0o600 });
    assert.equal((await readBrowserFailureDiagnostics(directory, 'journey')).availability, 'available');
    assert.deepEqual(await readBrowserFailureDiagnostics(directory, '../PRIVATE'), { availability: 'unavailable' });
    await writeFile(path, '{"suites":[');
    assert.equal((await readBrowserFailureDiagnostics(directory, 'journey')).availability, 'unavailable');
    await rm(path);
    await symlink('/does/not/matter', path);
    assert.equal((await readBrowserFailureDiagnostics(directory, 'journey')).availability, 'unavailable');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('runner pins the JSON environment override, rethrows failure and keeps capture disabled', async () => {
  // Read source only: importing run.mjs would launch the owned runtime.
  const runner = await readFile(new URL('./run.mjs', import.meta.url), 'utf8');
  const config = await readFile(new URL('../../apps/document-web/playwright.organization.config.ts', import.meta.url), 'utf8');
  assert.match(runner, /\.\.\.process\.env,[^\n]*PLAYWRIGHT_JSON_OUTPUT_FILE: join\(directory, `browser-\$\{phase\}`, 'results\.json'\)/u);
  assert.match(runner, /catch \(error\) \{\s*console\.error\(`Organization browser failure: \$\{JSON\.stringify\(await readBrowserFailureDiagnostics\(directory, phase\)\)\}`\);\s*throw error;/u);
  assert.match(config, /\['json', \{ outputFile: join\(output, 'results\.json'\) \}\]/u);
  for (const setting of ["preserveOutput: 'never'", "trace: 'off'", "screenshot: 'off'", "video: 'off'"]) assert.ok(config.includes(setting));
});

test('Agent result failures retain only the closed observed execution status and failure code', () => {
  const executionStatuses = ['queued', 'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'];
  const failureCodes = ['none', 'provider_denied', 'context_stale', 'invalid_output', 'dependency_unavailable', 'interrupted', 'commit_outcome_unknown'];
  for (const executionStatus of executionStatuses) for (const failureCode of failureCodes) {
    const raw = report({ status: 'failed', error: { message: 'expect(locator).toContainText() timeout PRIVATE' }, annotations: [
      { type: 'organization-stage', description: 'agent-result' },
      { type: 'organization-agent-status', description: executionStatus, body: 'PRIVATE', id: 'PRIVATE' },
      { type: 'organization-agent-failure-code', description: failureCode, purpose: 'PRIVATE' },
    ] });
    const actual = browserFailureDiagnostics(raw, 'journey').failure;
    assert.equal(actual.executionStatus, executionStatus);
    assert.equal(actual.executionFailureCode, failureCode === 'none' ? null : failureCode);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
});

test('invalid, excessive, incomplete or unrelated Agent observations disclose no execution fields', () => {
  const valid = [
    { type: 'organization-stage', description: 'agent-result' },
    { type: 'organization-agent-status', description: 'failed' },
    { type: 'organization-agent-failure-code', description: 'context_stale' },
  ];
  for (const annotations of [
    undefined, 'PRIVATE', [null], valid.slice(0, 2),
    [...valid, { type: 'organization-agent-status', description: 'PRIVATE' }],
    [...valid, { type: 'organization-agent-failure-code', description: 'PRIVATE' }],
    [...valid, { type: 'organization-stage', description: 'draft-save' }],
    [...valid, ...Array(30).fill({ type: 'PRIVATE', description: 'PRIVATE' })],
  ]) {
    const raw = JSON.parse(report({ status: 'failed', annotations }));
    raw.suites[0].specs[0].tests[0].annotations = valid;
    const actual = browserFailureDiagnostics(JSON.stringify(raw), 'journey').failure;
    assert.equal(actual.executionStatus, undefined);
    assert.equal(actual.executionFailureCode, undefined);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
  const unrelated = browserFailureDiagnostics(report({ status: 'failed', annotations: valid }, 'persistence'), 'persistence').failure;
  assert.equal(unrelated.executionStatus, undefined);
  assert.equal(unrelated.executionFailureCode, undefined);
});

test('Agent observation follows only the unchanged failed UI assertion and rethrows its original error', async () => {
  const support = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  assert.match(support, /try \{\s*await expect\(executionRegion\)\.toContainText\('実行状態：成功'\);\s*\} catch \(error\) \{/u);
  assert.match(support, /request\.get\(`\$\{origin\}\/v1\/organization\/agent-executions\/\$\{result\.execution\.id\}`, \{ timeout: 2000, maxRetries: 0, maxRedirects: 0 \}\)/u);
  assert.match(support, /\} catch \{ \/\* Preserve the original UI failure[^\n]*\n\s*throw error;/u);
  assert.match(support, /annotations\.push\(\{ type: 'organization-agent-status', description: status \}, \{ type: 'organization-agent-failure-code', description: failureCode \?\? 'none' \}\)/u);
});
