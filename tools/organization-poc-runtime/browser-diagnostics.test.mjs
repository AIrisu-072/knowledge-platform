import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';
import { browserFailureDiagnostics, readBrowserFailureDiagnostics } from './browser-diagnostics.mjs';

const titles = {
  journey: '実2名UIで根拠・候補・3種の人間判断を選択提出し、差戻後の新試行を非公開で再提出する',
  persistence: '両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する',
};
const report = (result, phase = 'journey', spec = {}) => JSON.stringify({ suites: [{ specs: [{
  title: titles[phase], file: `${phase}.spec.ts`, line: 10, column: 1, ...spec, tests: [{ results: [result] }],
}] }] });

const readEndpoints = ['session', 'task-list', 'task', 'snapshot', 'return-instruction', 'artifact', 'operation',
  'evidence', 'finding', 'decision', 'agent', 'agent-result', 'document', 'folder-root', 'folder-children'];
const readAnnotation = description => ({ type: 'organization-read-failure', description });
const readFailure = annotations => ({ status: 'failed', annotations, error: {
  message: 'expect(received).toBe(expected) PRIVATE', location: { file: 'support.ts', line: 66, column: 74 },
} });

test('failed GET projection retains only an atomic bounded HTTP status and closed endpoint class', () => {
  for (const phase of ['journey', 'persistence']) for (const endpoint of readEndpoints) for (const httpStatus of [100, 199, 201, 301, 400, 401, 403, 404, 409, 429, 500, 503, 599]) {
    const annotation = { ...readAnnotation(`${httpStatus}:${endpoint}`), body: 'PRIVATE', url: 'PRIVATE' };
    const actual = browserFailureDiagnostics(report(readFailure([annotation]), phase), phase).failure;
    assert.deepEqual(actual, { test: phase, source: 'support.ts', line: 66, column: 74,
      status: 'failed', errorCategory: 'assertion', matcher: 'toBe', httpStatus, readEndpoint: endpoint });
  }
});

test('invalid, duplicate, excessive, stale or unrelated GET annotations disclose no read fields', () => {
  const valid = readAnnotation('404:operation');
  for (const annotations of [undefined, 'PRIVATE', [null], [readAnnotation(null)], [readAnnotation(404)],
    ...['099:task', '600:task', '200:task', '0404:task', '404:unknown', '404:task\n', '404:PRIVATE', '404:task:PRIVATE', 'PRIVATE'.repeat(10000)].map(value => [readAnnotation(value)]),
    [valid, valid], [valid, readAnnotation('PRIVATE')], Array(33).fill(valid)]) {
    const raw = JSON.parse(report(readFailure(annotations), 'persistence'));
    raw.suites[0].specs[0].tests[0].annotations = [valid];
    const actual = browserFailureDiagnostics(JSON.stringify(raw), 'persistence').failure;
    assert.equal(actual.httpStatus, undefined);
    assert.equal(actual.readEndpoint, undefined);
    assert.ok(!JSON.stringify(actual).includes('PRIVATE'));
  }
  for (const result of [
    { ...readFailure([valid]), status: 'timedOut' },
    { ...readFailure([valid]), error: { message: 'expect(value).toEqual(expected)' } },
    { ...readFailure([valid]), error: { message: 'expect(value).toBe(expected)', location: { file: 'journey.spec.ts' } } },
  ]) {
    const actual = browserFailureDiagnostics(report(result), 'journey').failure;
    assert.equal(actual.httpStatus, undefined);
    assert.equal(actual.readEndpoint, undefined);
  }
});

// Execute the actual get() body with only its request, assertion and annotation boundaries replaced.
// Importing the runtime runner or starting Playwright/HTTP is neither needed nor allowed here.
async function isolatedGet(annotations, failure) {
  const source = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  const require = createRequire(new URL('../../apps/document-web/package.json', import.meta.url));
  const ts = require('typescript');
  const body = source.slice(source.indexOf('export async function get<T>'), source.indexOf('export async function assertSessions'));
  const output = ts.transpileModule(body, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  runInNewContext(output, { exports, URL, test: { info: () => ({ annotations }) },
    expect: actual => ({ toBe: expected => { assert.equal(expected, 200); if (actual !== expected) throw failure; } }) });
  return exports.get;
}

test('get emits safe endpoint families only on its unchanged failing status assertion without reading bodies or retrying', async () => {
  const routes = [
    ['/v1/organization/session', 'session'], ['/v1/organization/tasks?view=context', 'task-list'],
    ['/v1/organization/tasks?view=queue', 'task-list'], ['/v1/organization/tasks/PRIVATE', 'task'],
    ['/v1/organization/handoff-snapshots/PRIVATE', 'snapshot'], ['/v1/organization/return-instructions/PRIVATE', 'return-instruction'],
    ['/v1/organization/working-artifacts/PRIVATE', 'artifact'], ['/v1/organization/operations/PRIVATE', 'operation'],
    ['/v1/organization/tasks/PRIVATE/evidence', 'evidence'], ['/v1/organization/evidence/PRIVATE', 'evidence'],
    ['/v1/organization/tasks/PRIVATE/findings', 'finding'], ['/v1/organization/findings/PRIVATE', 'finding'],
    ['/v1/organization/findings/PRIVATE/decisions', 'decision'], ['/v1/organization/agent-executions/PRIVATE', 'agent'],
    ['/v1/organization/agent-executions/PRIVATE/result', 'agent-result'], ['/v1/documents/PRIVATE?view=published', 'document'],
    ['/v1/documents/PRIVATE/revisions?pageSize=100', 'document'], ['/v1/documents/PRIVATE/versions/PRIVATE/files?purpose=published', 'document'],
    ['/v1/folders/root', 'folder-root'], ['/v1/folders/PRIVATE/children?pageSize=200', 'folder-children'],
  ];
  const annotations = [], original = new Error('PRIVATE original assertion');
  const get = await isolatedGet(annotations, original);
  for (const [path, readEndpoint] of routes) {
    annotations.length = 0;
    let calls = 0, bodies = 0;
    const request = { get: async (...args) => {
      calls++; assert.deepEqual(args, [`http://127.0.0.1:1${path}`]);
      return { status: () => 503, json: () => { bodies++; return {}; } };
    } };
    await assert.rejects(get(request, 'http://127.0.0.1:1', path), error => error === original);
    assert.equal(calls, 1); assert.equal(bodies, 0);
    assert.deepEqual(JSON.parse(JSON.stringify(annotations)), [readAnnotation(`503:${readEndpoint}`)]);
    const projected = browserFailureDiagnostics(report(readFailure(annotations), 'persistence'), 'persistence').failure;
    assert.equal(projected.httpStatus, 503); assert.equal(projected.readEndpoint, readEndpoint);
    assert.ok(!JSON.stringify(projected).includes('PRIVATE'));
  }
  annotations.length = 0;
  const payload = { unchanged: true };
  assert.equal(await get({ get: async () => ({ status: () => 200, json: async () => payload }) }, 'http://127.0.0.1:1', '/v1/organization/session'), payload);
  assert.equal(annotations.length, 0);
  for (const path of ['/v1/organization/PRIVATE', '/v1/organization/tasks/PRIVATE/unknown', '/v1/organization/session?PRIVATE', '/v1/organization/tasks/' + 'PRIVATE'.repeat(1000),
    '/v1/folders/root?PRIVATE', '/v1/folders/PRIVATE/children?name=PRIVATE', '/v1/folders/PRIVATE/children?pageSize=200&cursor=PRIVATE']) {
    await assert.rejects(get({ get: async () => ({ status: () => 404 }) }, 'http://127.0.0.1:1', path), error => error === original);
    assert.equal(annotations.length, 0);
  }
  const brokenAnnotations = Object.freeze([]);
  const brokenGet = await isolatedGet(brokenAnnotations, original);
  await assert.rejects(brokenGet({ get: async () => ({ status: () => 404 }) }, 'http://127.0.0.1:1', '/v1/organization/session'), error => error === original);
});

const rootFolderTitles = {
  journey: '実2名UIでSystem Root直下にフォルダーを作成し固定要求replayと現在Readを確認する',
  persistence: '両process再起動後もRoot直下フォルダーと固定要求replayと現在権限を保持する',
};
const rootFolderActions = ['root-folder-read', 'root-folder-preview', 'root-folder-cancel', 'root-folder-input',
  'root-folder-create', 'root-folder-verify', 'root-folder-replay', 'root-folder-office', 'root-folder-persistence'];

test('Root folder tests retain only their fixed case and action without disclosing names, reasons or receipts', () => {
  const secret = 'https://PRIVATE_NAME:PRIVATE_REASON@private.example/PRIVATE_OPERATION';
  for (const phase of ['journey', 'persistence']) for (const action of rootFolderActions) {
    const raw = report({ status: 'failed', annotations: [{ type: 'organization-stage', description: action, name: secret }],
      error: { message: `expect(value).toBe(expected) ${secret}`, matcherResult: { name: 'toBe', actual: secret, expected: secret } },
      request: { name: secret, reason: secret }, receipt: secret, stdout: [secret], attachments: [{ body: secret }] }, phase, { title: rootFolderTitles[phase] });
    assert.deepEqual(browserFailureDiagnostics(raw, phase).failure, {
      test: `root-folder-${phase}`, status: 'failed', errorCategory: 'assertion', matcher: 'toBe', currentAction: action,
    });
  }
  for (const phase of ['journey', 'persistence']) {
    const raw = report(readFailure([readAnnotation('403:folder-root')]), phase, { title: rootFolderTitles[phase] });
    assert.equal(browserFailureDiagnostics(raw, phase).failure.readEndpoint, 'folder-root');
    const unknown = report({ status: 'failed', error: { message: secret }, annotations: [{ type: 'organization-stage', description: `${rootFolderActions[0]}:${secret}` }] }, phase, { title: `${rootFolderTitles[phase]} ${secret}` });
    assert.deepEqual(browserFailureDiagnostics(unknown, phase).failure, { status: 'failed', errorCategory: 'unavailable' });
  }
});

test('existing phase selection collects the separate Root folder cases with explicit capture off and actual GUI receipts', async () => {
  const config = await readFile(new URL('../../apps/document-web/playwright.organization.config.ts', import.meta.url), 'utf8');
  assert.match(config, /testMatch: phase === 'journey' \? 'journey\.spec\.ts' : 'persistence\.spec\.ts'/u);
  for (const phase of ['journey', 'persistence']) {
    const source = await readFile(new URL(`../../apps/document-web/e2e-organization/${phase}.spec.ts`, import.meta.url), 'utf8');
    assert.equal((source.match(/\btest\('/gu) ?? []).length, 2);
    assert.ok(source.includes(`test('${rootFolderTitles[phase]}'`));
    const root = source.slice(source.indexOf("test.describe('System Root folder creation'"));
    // Capture options are worker-scoped in pinned Playwright, so they must be file-level.
    const captureSettings = source.indexOf("test.use({ screenshot: 'off', trace: 'off', video: 'off' })");
    assert.ok(captureSettings >= 0 && captureSettings < source.indexOf("test('"));
    for (const setting of ["serviceWorkers: 'block'", 'acceptDownloads: false']) assert.ok(root.includes(setting));
    assert.match(root, /finally \{\s*await officeContext\.close\(\);/u);
    assert.doesNotMatch(root, /test\.(?:skip|fixme|setTimeout)|waitForTimeout|\.attach\(|\.screenshot\(|\.tracing\.|recordVideo: \{/u);
    if (phase === 'journey') {
      assert.ok(root.indexOf('page.waitForResponse(') < root.indexOf("name: '作成する'"));
      assert.match(root, /response\.request\(\)\.postDataJSON\(\)/u);
      assert.match(root, /expect\(folderPosts\)\.toBe\(0\)/u);
      assert.match(root, /expect\(folderPosts\)\.toBe\(1\)/u);
      assert.match(root, /await saveRootFolderState\(context,/u);
    } else {
      assert.match(root, /await loadRootFolderState\(context\)/u);
      assert.match(root, /await replayRootFolderCreate\(request, context, state\)/u);
    }
  }
});

test('existing Root cases extend only the selected pagination tail create and restart oracle', async () => {
  const support = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  const journey = await readFile(new URL('../../apps/document-web/e2e-organization/journey.spec.ts', import.meta.url), 'utf8');
  const persistence = await readFile(new URL('../../apps/document-web/e2e-organization/persistence.spec.ts', import.meta.url), 'utf8');
  assert.match(journey, /await createSelectedFolderFromUi\(page, request, context, state\)/u);
  assert.match(journey, /expect\(folderPosts\)\.toBe\(2\)/u);
  assert.match(persistence, /await assertSelectedFolderUi\(page, request, context, state\)/u);
  assert.match(support, /selectedCreate: \{ request: CreateFolderData\['body'\]; receipt: MutationResult; child: Folder \}/u);
  const create = support.slice(support.indexOf('export async function createSelectedFolderFromUi'), support.indexOf('export async function assertSelectedFolderUi'));
  assert.match(create, /getByRole\('button', \{ name: '選択したフォルダーに子フォルダーを作成', exact: true \}\)/u);
  assert.ok(create.indexOf('page.waitForResponse(') < create.indexOf("name: '作成する'"));
  assert.match(create, /response\.request\(\)\.postDataJSON\(\)/u);
  assert.doesNotMatch(create, /request\.post|waitForTimeout|test\.setTimeout|screenshot|tracing/u);
});

test('Root folder restart oracle is separate, private, exclusive and bound to the owned run', async () => {
  const source = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  assert.ok(source.includes('export type RootFolderState'));
  const require = createRequire(new URL('../../apps/document-web/package.json', import.meta.url));
  const ts = require('typescript');
  const body = source.slice(source.indexOf('export type RootFolderState'), source.indexOf('export type EvidenceReceipt'));
  const output = ts.transpileModule(body, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  runInNewContext(output, { exports, readFile, writeFile, expect: actual => ({ toBe: expected => assert.equal(actual, expected) }) });
  const directory = await mkdtemp(join(tmpdir(), 'organization-root-folder-'));
  const context = { statePath: join(directory, 'state.json'), documentId: 'synthetic-run-document' };
  const state = { schemaVersion: 3, documentId: context.documentId, request: { name: 'PRIVATE_NAME', reason: 'PRIVATE_REASON' }, paginationChildren: [], selectedCreate: { request: { parentFolderId: 'tail' }, receipt: { resourceId: 'child' }, child: { folderId: 'child' } } };
  try {
    await writeFile(context.statePath, 'unchanged Work state', { mode: 0o600 });
    await exports.saveRootFolderState(context, state);
    assert.equal((await stat(`${context.statePath}.root-folder`)).mode & 0o777, 0o600);
    assert.deepEqual(JSON.parse(JSON.stringify(await exports.loadRootFolderState(context))), state);
    await assert.rejects(exports.saveRootFolderState(context, state), { code: 'EEXIST' });
    assert.equal(await readFile(context.statePath, 'utf8'), 'unchanged Work state');
    await assert.rejects(exports.loadRootFolderState({ ...context, documentId: 'different-run' }));
    for (const schemaVersion of [1, 2]) {
      await writeFile(`${context.statePath}.root-folder`, JSON.stringify({ ...state, schemaVersion }));
      await assert.rejects(exports.loadRootFolderState(context));
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('pagination fixture creates 201 descendants once and stops on an unknown write without new IDs or retries', async () => {
  const source = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  const require = createRequire(new URL('../../apps/document-web/package.json', import.meta.url));
  const ts = require('typescript');
  const body = source.slice(source.indexOf('export type RootFolderState'), source.indexOf('export type EvidenceReceipt'));
  const output = ts.transpileModule(body, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {}; let nextId = 0;
  runInNewContext(output, { exports, createOperationId: () => `synthetic-${++nextId}`, currentAction: () => {},
    expect: actual => ({ toBe: expected => assert.equal(actual, expected) }) });
  assert.equal(typeof exports.prepareFolderPagination, 'function');
  for (const failureAt of [null, 3]) {
    const commands = [];
    const request = { post: async (url, { data }) => {
      assert.equal(url, 'http://127.0.0.1:1/v1/folders'); commands.push(data);
      if (commands.length === failureAt) throw new Error('unknown synthetic response');
      return { status: () => 201, json: async () => ({ operationId: data.operationId, resourceId: data.folderId, resultingRevision: 0, changed: true }) };
    } };
    const result = exports.prepareFolderPagination(request, 'http://127.0.0.1:1', 'gui-created-parent', 0);
    if (failureAt) {
      await assert.rejects(result, /unknown synthetic response/u);
      assert.equal(commands.length, failureAt);
    } else {
      const rows = await result;
      assert.equal(rows.length, 201); assert.equal(commands.length, 201);
      assert.equal(new Set(commands.flatMap(command => [command.operationId, command.folderId])).size, 402);
      assert.equal(new Set(rows.map(row => row.name)).size, 201);
      for (const [index, row] of rows.entries()) {
        assert.equal(row.folderId, commands[index].folderId);
        assert.equal(row.parentFolderId, 'gui-created-parent'); assert.equal(row.revision, 0);
      }
    }
    for (const command of commands) {
      assert.equal(command.parentFolderId, 'gui-created-parent'); assert.equal(command.expectedParentRevision, 0);
    }
  }
});

test('selected-create replay retains exact GUI request and receipt and checks current office authorization without changing snapshots', async () => {
  const source = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts', import.meta.url), 'utf8');
  const require = createRequire(new URL('../../apps/document-web/package.json', import.meta.url));
  const ts = require('typescript');
  const body = source.slice(source.indexOf('export type RootFolderState'), source.indexOf('export type EvidenceReceipt'));
  const output = ts.transpileModule(body, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const context = { sales: 'http://127.0.0.1:1', office: 'http://127.0.0.1:2' };
  const child = { folderId: 'child', parentFolderId: 'tail', revision: 0, name: 'PRIVATE_NAME' };
  const state = { request: { parentFolderId: 'root', folderId: 'parent' },
    sales: { root: { folderId: 'root' }, children: { items: [], nextCursor: null } }, office: { root: { folderId: 'root' }, children: { items: [], nextCursor: null } },
    selectedCreate: { request: { operationId: 'operation', folderId: child.folderId, parentFolderId: child.parentFolderId, expectedParentRevision: 0, name: child.name, reason: 'PRIVATE_REASON' },
      receipt: { operationId: 'operation', resourceId: child.folderId, resultingRevision: 0, changed: true, occurredAt: '2026-10-05T23:00:00Z' }, child } };
  const calls = []; const reads = []; const exports = {};
  runInNewContext(output, { exports, currentAction: () => {}, isDeepStrictEqual: (a, b) => JSON.stringify(a) === JSON.stringify(b),
    expect: actual => ({ toBe: expected => assert.equal(actual, expected), toBeNull: () => assert.equal(actual, null) }),
    get: async (_request, origin, path) => { reads.push([origin, path]); return path === '/v1/folders/root' ? state.sales.root : path.includes('/tail/') ? { items: [child], nextCursor: null } : state.sales.children; } });
  const request = { post: async (url, { data }) => {
    calls.push([url, data]); assert.equal(data, state.selectedCreate.request);
    return url.startsWith(context.sales) ? { status: () => 201, json: async () => state.selectedCreate.receipt }
      : { status: () => 403, json: async () => ({ code: 'FORBIDDEN' }) };
  } };
  await exports.replaySelectedFolderCreate(request, context, state);
  assert.deepEqual(calls.map(([url]) => url), [`${context.sales}/v1/folders`, `${context.office}/v1/folders`]);
  assert.equal(reads.filter(([, path]) => path.includes('/tail/')).length, 4);
  assert.equal(reads.filter(([, path]) => path === '/v1/folders/root').length, 2);
});

test('standard JSON timeout retains only the current result action annotation without inferring completion', () => {
  const raw = JSON.parse(report({ status: 'timedOut', error: { message: 'Test timeout of 120000ms exceeded.' },
    annotations: [{ type: 'organization-stage', description: 'source-file-select', detail: 'PRIVATE' }] }));
  raw.suites[0].specs[0].tests[0].annotations = [{ type: 'organization-stage', description: 'journey-setup' }];
  assert.deepEqual(browserFailureDiagnostics(JSON.stringify(raw), 'journey'), { phase: 'journey', availability: 'available', failure: {
    test: 'journey', status: 'timedOut', errorCategory: 'test-timeout', currentAction: 'source-file-select',
  } });
});

test('all forty-four fixed action names survive the closed projection', () => {
  const stages = ['journey-setup', 'office-navigation', 'sales-navigation', 'document-navigation', 'task-navigation',
    'draft-save', 'source-read', 'evidence-module', 'source-document-select', 'source-file-select', 'evidence-input',
    'evidence-submit', 'finding-input', 'finding-submit', 'decision-select', 'decision-input', 'decision-preview',
    'decision-confirm', 'visibility-verify', 'submit-preview', 'submit-selection', 'submit-confirm', 'office-claim',
    'return-preview', 'return-confirm', 'sales-reclaim', 'resubmit', 'office-reclaim', 'final-verify', 'persistence-verify', 'agent-module', 'agent-input', 'agent-request', 'agent-result', 'agent-replay', 'complete-preview', 'complete-confirm', 'complete-replay', 'hold-preview', 'hold-confirm', 'hold-replay', 'resume-preview', 'resume-confirm', 'resume-replay'];
  assert.equal(stages.length, 44);
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
