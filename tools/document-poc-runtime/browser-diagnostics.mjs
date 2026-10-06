// Privacy boundary for Playwright JSON: raw text is inspected but never returned.
import { constants } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { join, resolve } from 'node:path';

export const MAX_BROWSER_REPORT_BYTES = 8 * 1024 * 1024;
const MAX_RECORDS = 20, MAX_NODES = 1000, MAX_DEPTH = 8, MAX_TEXT = 16 * 1024;
const sources = new Set(['document-runtime.spec.ts', 'initial-registration.spec.ts', 'metadata-editor.spec.ts', 'document-schedule-cancellation.spec.ts', 'lifecycle-operations.spec.ts', 'lifecycle-operations-persistence.spec.ts', 'working-version-editor.spec.ts', 'working-version-editor-persistence.spec.ts', 'human-agent-consistency.spec.ts', 'worker-failure.spec.ts', 'persistence.spec.ts', 'timestamp-layout.spec.ts', 'support.ts', 'japanese-font.ts']);
const statuses = new Set(['passed', 'failed', 'timedOut', 'skipped', 'interrupted', 'unavailable']);
const categories = new Set(['strict-locator', 'locator-timeout', 'test-timeout', 'HTTP-status-assertion', 'assertion', 'response-loss', 'unavailable']);
const responseLossCodes = new Set(['configuration', 'admission', 'unarmed-retry', 'payload', 'upstream-status', 'upstream-result',
  'retry-payload', 'retry-result', 'unrecovered', 'observation-window', 'upstream-transport']);
const matchers = new Set(['toBe', 'toEqual', 'toStrictEqual', 'toBeVisible', 'toBeHidden', 'toHaveCount', 'toHaveText', 'toContainText',
  'toHaveURL', 'toHaveAttribute', 'toHaveCSS', 'toHaveLength', 'toBeFocused', 'toBeEnabled', 'toBeDisabled', 'toBeChecked',
  'toBeGreaterThan', 'toBeLessThan', 'toBeGreaterThanOrEqual', 'toBeLessThanOrEqual', 'toContain', 'toMatchObject', 'toMatch']);
// A disclosure allowlist, not a new API enum or interpretation of business semantics.
const problemCodes = new Set(['FORBIDDEN', 'REVISION_CONFLICT', 'DOCUMENT_NOT_FOUND', 'DOCUMENT_VERSION_NOT_FOUND', 'REVISION_NOT_FOUND',
  'FOLDER_NOT_FOUND', 'DEPENDENCY_UNAVAILABLE', 'INTERNAL', 'VALIDATION_FAILED', 'PUBLISH_QUALITY_REJECTED', 'OPERATION_CONFLICT',
  'TIMEOUT', 'COMMIT_OUTCOME_UNKNOWN', 'AUTHENTICATION_REQUIRED', 'IDENTITY_UNAVAILABLE', 'INTEGRITY_VIOLATION']);
const text = value => typeof value === 'string' ? value.slice(0, MAX_TEXT).replace(/\x1b\[[0-9;]*m/gu, '') : '';
const integer = value => Number.isInteger(value) && value > 0 && value <= 1_000_000;
const httpCode = value => Number.isInteger(value) && value >= 200 && value <= 599;
const object = value => value && typeof value === 'object' && !Array.isArray(value);
const unavailable = () => ({ availability: 'unavailable', counts: { passed: 0, failed: 0, skipped: 0 }, tests: [], truncated: false });

const startupErrors = new Set(['eval-blocked', 'require-undefined', 'exports-undefined', 'process-undefined', 'reference-error', 'type-error', 'syntax-error', 'chunk-load', 'other']);
const uiStages = new Set(['before-folder-wait', 'before-folder-click', 'after-folder-click', 'test-end']);
const apiRoutes = new Set(['folder-root', 'folder-children', 'document-list', 'document-detail', 'session', 'other']);
function sanitizeUi(value) {
  if (!object(value) || !uiStages.has(value.stage)) return undefined;
  if (value.availability !== 'available') return { stage: value.stage, availability: 'unavailable' };
  const counts = {};
  for (const key of ['viewportWidth', 'viewportHeight', 'rootChildren', 'folderRegionCount', 'sharedFolderButtonCount',
    'tableCount', 'alertCount', 'inViewportCount', 'receivesPointerCount', 'disabledCount', 'hiddenAncestorCount']) {
    counts[key] = Number.isInteger(value[key]) && value[key] >= 0 && value[key] <= (key.startsWith('viewport') ? 10000 : 1000) ? value[key] : 0;
  }
  return { stage: value.stage, availability: 'available', ...counts };
}
const journeyStages = new Set(['context-read', 'sessions-verified', 'api-preflight-complete', 'gui-loaded', 'folder-selected',
  'document-selected', 'detail-opened', 'download-requested', 'download-received', 'download-saved', 'snapshot-read',
  'history-opened', 'comparison-verified', 'policy-saved', 'version-form-opened', 'version-created',
  'publication-form-opened', 'publication-response-accepted', 'publication-success-visible', 'publication-confirmed', 'state-verified', 'snapshot-saved',
  'gui-initial-capabilities-verified', 'gui-initial-cancel-verified', 'gui-initial-created',
  'gui-initial-working-verified', 'gui-initial-published-shared', 'gui-initial-snapshot-saved',
  'gui-metadata-created', 'gui-metadata-cancel-verified', 'gui-metadata-working-verified',
  'gui-metadata-published-verified', 'gui-metadata-minor-verified', 'gui-metadata-noop-verified',
  'gui-metadata-list-return-pressed', 'gui-metadata-created-from-verified', 'gui-metadata-unread-list-verified',
  'gui-unread-readonly-verified', 'gui-metadata-revision-entry-ready', 'gui-metadata-revision-first-page-verified',
  'gui-metadata-comparison-pair-verified', 'gui-metadata-comparison-tab-pressed', 'gui-metadata-comparison-response-verified',
  'gui-metadata-revision-first-comparison-verified', 'gui-metadata-revision-reload-page-verified',
  'gui-metadata-snapshot-saved', 'gui-metadata-restart-verified',
  'gui-document-move-verified', 'gui-document-move-replay-verified',
  'gui-formal-revisions-readonly-verified', 'gui-formal-revisions-restart-readonly-verified',
  'gui-lifecycle-fixture-ready', 'gui-lifecycle-cancel-verified', 'gui-withdraw-fallback-verified',
  'gui-withdraw-null-verified', 'gui-publication-end-verified', 'gui-lifecycle-snapshot-saved', 'gui-lifecycle-restart-verified',
  'gui-schedule-created', 'gui-schedule-dismissed', 'gui-schedule-cancelled',
  'gui-schedule-replaced', 'gui-schedule-final-state-saved', 'gui-schedule-restart-verified',
  'gui-working-loss-armed', 'gui-working-loss-save-clicked', 'gui-working-loss-dropped',
  'gui-working-loss-headers-observed', 'gui-working-loss-unknown-visible', 'gui-working-loss-retry-armed', 'gui-working-loss-recovered',
  'gui-working-initial-updated', 'gui-working-manifest-ready', 'gui-working-cancel-verified',
  'gui-working-created', 'gui-working-updated', 'gui-working-publication-preserved',
  'gui-working-published', 'gui-working-snapshot-saved', 'gui-working-restart-verified',
  'pdf-context-read', 'pdf-fixtures-read', 'pdf-base-created', 'pdf-base-detail-read', 'pdf-base-published',
  'pdf-published-detail-read', 'pdf-target-created', 'pdf-target-detail-read', 'pdf-target-published', 'pdf-comparison-read',
  'pdf-base-files-read', 'pdf-base-download-verified', 'pdf-target-files-read', 'pdf-target-download-verified',
  'pdf-gui-verified', 'pdf-shared-state-verified', 'pdf-snapshot-saved']);
const fontSelection = 'kosugi-regular-japanese-heading-body';
const timestampLayout = 'long-iana-both-folds-1280-1440';
function timestampReceipt(test, status) {
  return status === 'passed' && (Array.isArray(test?.annotations) ? test.annotations.slice(0, 40) : []).some(annotation =>
    annotation?.type === 'runtime-timestamp-layout' && annotation.description === timestampLayout) ? { timestampLayout } : {};
}
function fontReceipt(test) {
  return (Array.isArray(test?.annotations) ? test.annotations.slice(0, 40) : []).some(annotation =>
    annotation?.type === 'runtime-font' && annotation.description === fontSelection) ? { fontSelection } : {};
}
function lastCompletedStage(test) {
  let stage;
  for (const annotation of (Array.isArray(test?.annotations) ? test.annotations.slice(0, 40) : [])) {
    if (annotation?.type === 'runtime-completed' && journeyStages.has(annotation.description)) stage = annotation.description;
  }
  return stage;
}
function sanitizeStartup(value) {
  if (!object(value)) return undefined;
  const count = v => Number.isInteger(v) && v >= 0 && v <= 1000 ? v : 0;
  return { rootChildren: count(value.rootChildren), documentStatus: httpCode(value.documentStatus) ? value.documentStatus : 0,
    domObservation: value.domObservation === 'available' ? 'available' : 'unavailable',
    apiEvents: (Array.isArray(value.apiEvents) ? value.apiEvents.slice(0, 12) : []).filter(object).map(event => ({
      route: apiRoutes.has(event.route) ? event.route : 'other', status: httpCode(event.status) ? event.status : 0 })),
    uiSnapshots: (Array.isArray(value.uiSnapshots) ? value.uiSnapshots.slice(0, 4) : []).map(sanitizeUi).filter(Boolean),
    scriptResponses: count(value.scriptResponses), scriptFailures: count(value.scriptFailures),
    apiResponses: count(value.apiResponses), apiFailures: count(value.apiFailures),
    cspViolations: count(value.cspViolations), consoleErrors: count(value.consoleErrors),
    pageErrors: (Array.isArray(value.pageErrors) ? value.pageErrors.slice(0, 10) : []).map(v => startupErrors.has(v) ? v : 'other') };
}
function startupAttachment(result) {
  const attachment = (Array.isArray(result?.attachments) ? result.attachments.slice(0, 20) : []).find(item =>
    item?.name === 'runtime-startup.json' && item.contentType === 'application/json');
  if (typeof attachment?.body !== 'string' || attachment.body.length > 8192) return undefined;
  try { return sanitizeStartup(JSON.parse(Buffer.from(attachment.body, 'base64').toString('utf8'))); } catch { return undefined; }
}

function location(value) {
  if (!object(value) || typeof value.file !== 'string' || value.file.length > MAX_TEXT) return undefined;
  const source = value.file.split(/[\\/]/u).at(-1);
  if (!sources.has(source)) return undefined;
  return { source, ...(integer(value.line) ? { line: value.line } : {}), ...(integer(value.column) ? { column: value.column } : {}) };
}
function stackLocation(value) {
  const match = text(value).match(/(?:^|[\\/\s(])((?:document-runtime|initial-registration|metadata-editor|document-schedule-cancellation|lifecycle-operations(?:-persistence)?|working-version-editor(?:-persistence)?|human-agent-consistency|worker-failure|persistence|timestamp-layout)\.spec\.ts|support\.ts):(\d{1,7}):(\d{1,7})(?:\D|$)/u);
  return match ? location({ file: match[1], line: Number(match[2]), column: Number(match[3]) }) : undefined;
}
// Node util.inspect uses unquoted property names and quoted string values. Accept
// only its bounded, flat primitive-property form; do not evaluate source text or
// mistake a code-like substring inside another quoted field for a property.
function inspectedProblemCode(value) {
  if (typeof value !== 'string' || value.length > MAX_TEXT) return undefined;
  const primitive = String.raw`(?:'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|true|false|null|undefined|[0-9]+)`;
  const field = String.raw`[A-Za-z_$][\w$]*:\s*${primitive}`;
  const pattern = new RegExp(String.raw`^\{\s*(?:${field}\s*,\s*)*code:\s*'([A-Z_]+)'(?:\s*,\s*${field})*\s*\}$`, 'u');
  return value.trim().match(pattern)?.[1];
}

function describeError(error, status) {
  const message = text(error?.message), snippet = text(error?.snippet);
  const lossCode = message.match(/^(?:Error: )?\[working-loss:([a-z-]+)\](?: |$)/u)?.[1];
  if (responseLossCodes.has(lossCode)) return { errorCategory: 'response-loss', responseLossCode: lossCode };
  const rawMatcher = error?.matcherResult?.name ?? message.match(/\b(to[A-Z][A-Za-z]+)\s*\(/u)?.[1];
  const matcher = matchers.has(rawMatcher) ? rawMatcher : undefined;
  let expected = error?.matcherResult?.expected, actual = error?.matcherResult?.actual;
  if (!httpCode(expected)) expected = Number(message.match(/\bExpected:\s*(\d{3})\b/u)?.[1]);
  if (!httpCode(actual)) actual = Number(message.match(/\bReceived:\s*(\d{3})\b/u)?.[1]);
  const httpAssertion = httpCode(expected) && httpCode(actual) && /\bstatus(?:Code)?\b/iu.test(`${message}\n${snippet}`);
  let thrownValue = object(error?.value) ? error.value : undefined;
  if (typeof error?.value === 'string' && error.value.length <= MAX_TEXT) {
    try { const parsed = JSON.parse(error.value); if (object(parsed)) thrownValue = parsed; } catch { /* no raw value disclosure */ }
  }
  const rawProblem = error?.problem?.code ?? thrownValue?.code ?? inspectedProblemCode(error?.value) ?? (object(error?.matcherResult?.actual) ? error.matcherResult.actual.code : undefined)
    ?? message.match(/(?:"code"\s*:\s*"|\bProblem\.code:\s*)([A-Z_]+)\b/u)?.[1];
  let errorCategory = 'unavailable';
  if (/strict mode violation/iu.test(message)) errorCategory = 'strict-locator';
  else if (/test timeout of/iu.test(message) || status === 'timedOut') errorCategory = 'test-timeout';
  else if (/timeout/iu.test(message) && /locator(?:\.|\))|expect\(locator\)/iu.test(message)) errorCategory = 'locator-timeout';
  else if (httpAssertion) errorCategory = 'HTTP-status-assertion';
  else if (matcher || /assertion(?:error| failed)|expect\(/iu.test(message)) errorCategory = 'assertion';
  return { errorCategory, ...(matcher ? { matcher } : {}),
    ...(httpAssertion ? { expected, actual } : {}), ...(problemCodes.has(rawProblem) ? { problemCode: rawProblem } : {}) };
}

export function browserDiagnostics(report) {
  if (!object(report) || (!Array.isArray(report.suites) && !Array.isArray(report.errors))) return unavailable();
  const output = { availability: 'available', counts: { passed: 0, failed: 0, skipped: 0 }, tests: [], truncated: false };
  const globalErrors = Array.isArray(report.errors) ? report.errors : [];
  output.globalErrorsPresent = globalErrors.length > 0;
  output.globalErrorCount = Math.min(globalErrors.length, MAX_NODES);
  for (const error of globalErrors.slice(0, MAX_RECORDS)) {
    output.tests.push({ scope: 'global', ...(location(error?.location) ?? stackLocation(error?.stack)), status: 'failed', ...describeError(error, 'failed') });
  }
  if (globalErrors.length > MAX_RECORDS) output.truncated = true;
  const suites = Array.isArray(report.suites) ? report.suites : [];
  const queue = suites.slice(0, MAX_NODES).map(suite => ({ suite, depth: 0 }));
  if (suites.length > MAX_NODES) output.truncated = true;
  let visited = 0;
  const seen = new WeakSet();
  while (queue.length && visited < MAX_NODES) {
    const { suite, depth } = queue.shift(); visited++;
    if (!object(suite)) continue;
    if (seen.has(suite) || depth > MAX_DEPTH) { output.truncated = true; continue; }
    seen.add(suite);
    for (const spec of (Array.isArray(suite.specs) ? suite.specs.slice(0, MAX_NODES) : [])) {
      if (!object(spec)) continue;
      for (const test of (Array.isArray(spec.tests) ? spec.tests.slice(0, MAX_NODES) : [])) {
        if (++visited > MAX_NODES) { output.truncated = true; break; }
        const result = Array.isArray(test?.results) ? test.results.at(-1) : undefined;
        const status = statuses.has(result?.status) ? result.status : test?.status === 'skipped' ? 'skipped' : 'unavailable';
        if (status === 'passed') output.counts.passed++;
        else if (status === 'skipped') output.counts.skipped++;
        else if (['failed', 'timedOut', 'interrupted'].includes(status)) output.counts.failed++;
        if (output.tests.length === MAX_RECORDS) { output.truncated = true; continue; }
        const error = object(result?.error) ? result.error : Array.isArray(result?.errors) ? result.errors.slice(0, 4).find(object) : undefined;
        const source = location(error?.location) ?? location(result?.errorLocation) ?? stackLocation(error?.stack) ?? location(spec) ?? location(suite);
        output.tests.push({ ...source, status, ...fontReceipt(test), ...timestampReceipt(test, status), ...(lastCompletedStage(test) ? { lastCompletedStage: lastCompletedStage(test) } : {}), ...(startupAttachment(result) ? { startup: startupAttachment(result) } : {}), ...(['passed', 'skipped'].includes(status) ? {} : describeError(error, status)) });
      }
      if (visited > MAX_NODES) break;
    }
    if (Array.isArray(suite.specs) && suite.specs.length > MAX_NODES) output.truncated = true;
    if (Array.isArray(suite.suites)) {
      const available = Math.max(0, MAX_NODES - queue.length - visited);
      queue.push(...suite.suites.slice(0, available).map(child => ({ suite: child, depth: depth + 1 })));
      if (suite.suites.length > available) output.truncated = true;
    }
  }
  if (queue.length) output.truncated = true;
  return sanitizeBrowserDiagnostics(output);
}

// Re-apply the exact disclosure boundary when report.json is later summarized.
export function sanitizeBrowserDiagnostics(value) {
  if (!object(value) || value.availability !== 'available') return unavailable();
  const tests = (Array.isArray(value.tests) ? value.tests.slice(0, MAX_RECORDS) : []).filter(object).map(record => ({
    ...(journeyStages.has(record.lastCompletedStage) ? { lastCompletedStage: record.lastCompletedStage } : {}),
    ...(record.scope === 'global' ? { scope: 'global' } : {}),
    ...(record.fontSelection === fontSelection ? { fontSelection } : {}),
    ...(record.status === 'passed' && record.timestampLayout === timestampLayout ? { timestampLayout } : {}),
    ...(sanitizeStartup(record.startup) ? { startup: sanitizeStartup(record.startup) } : {}),
    ...(sources.has(record.source) ? { source: record.source, ...(integer(record.line) ? { line: record.line } : {}),
      ...(integer(record.column) ? { column: record.column } : {}) } : {}),
    status: statuses.has(record.status) ? record.status : 'unavailable',
    ...(['passed', 'skipped'].includes(record.status) ? {} : { errorCategory: categories.has(record.errorCategory) ? record.errorCategory : 'unavailable' }),
    ...(!['passed', 'skipped'].includes(record.status) && record.errorCategory === 'response-loss' && responseLossCodes.has(record.responseLossCode)
      ? { responseLossCode: record.responseLossCode } : {}),
    ...(matchers.has(record.matcher) ? { matcher: record.matcher } : {}),
    ...(record.errorCategory === 'HTTP-status-assertion' && httpCode(record.expected) && httpCode(record.actual) ? { expected: record.expected, actual: record.actual } : {}),
    ...(problemCodes.has(record.problemCode) ? { problemCode: record.problemCode } : {}),
  }));
  const count = value => Number.isInteger(value) && value >= 0 && value <= MAX_NODES ? value : 0;
  return { availability: 'available', counts: { passed: count(value.counts?.passed), failed: count(value.counts?.failed), skipped: count(value.counts?.skipped) },
    globalErrorsPresent: value.globalErrorsPresent === true, globalErrorCount: count(value.globalErrorCount),
    tests, truncated: value.truncated === true || (Array.isArray(value.tests) && value.tests.length > MAX_RECORDS) };
}

export async function readBrowserDiagnostics(runDirectory, phase) {
  if (!['journey', 'persistence'].includes(phase)) return unavailable();
  let file;
  try {
    const requested = resolve(runDirectory);
    if ((await lstat(requested)).isSymbolicLink()) return unavailable();
    const root = await realpath(requested), directory = join(root, `browser-${phase}`), path = join(directory, 'results.json');
    for (const candidate of [root, directory]) {
      const info = await lstat(candidate);
      if (!info.isDirectory() || info.isSymbolicLink() || await realpath(candidate) !== candidate) return unavailable();
    }
    file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const info = await file.stat();
    if (!info.isFile() || info.size > MAX_BROWSER_REPORT_BYTES) return unavailable();
    const buffer = Buffer.alloc(MAX_BROWSER_REPORT_BYTES + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
      if (!bytesRead) break;
      length += bytesRead;
    }
    if (length > MAX_BROWSER_REPORT_BYTES) return unavailable();
    return browserDiagnostics(JSON.parse(buffer.subarray(0, length).toString('utf8')));
  } catch { return unavailable(); }
  finally { await file?.close(); }
}


export function sanitizeBrowserPhases(value) {
  const journey = sanitizeBrowserDiagnostics(value?.journey);
  const persistence = sanitizeBrowserDiagnostics(value?.persistence);
  const candidates = [...journey.tests, ...persistence.tests];
  const unsuccessful = record => !['passed', 'skipped'].includes(record.status);
  const selected = new Set([...candidates.filter(unsuccessful), ...candidates.filter(record => !unsuccessful(record))].slice(0, MAX_RECORDS));
  for (const phase of [journey, persistence]) {
    const retained = phase.tests.filter(record => selected.has(record));
    if (retained.length < phase.tests.length) phase.truncated = true;
    phase.tests = retained;
  }
  return { journey, persistence };
}
