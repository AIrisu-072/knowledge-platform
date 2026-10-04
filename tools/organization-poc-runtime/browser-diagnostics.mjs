// Closed failure projection only. Raw Playwright JSON stays in the private run directory.
import { constants } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const MAX_BYTES = 8 * 1024 * 1024, MAX_NODES = 1000, MAX_DEPTH = 8, MAX_TEXT = 16 * 1024;
const phases = new Set(['journey', 'persistence']);
const sources = new Set(['journey.spec.ts', 'persistence.spec.ts', 'support.ts']);
const statuses = new Set(['failed', 'timedOut', 'interrupted']);
const tests = new Map([
  ['実2名UIで根拠・候補・3種の人間判断を選択提出し、差戻後の新試行を非公開で再提出する', 'journey'],
  ['両process再起動後も根拠・候補・人間判断・固定提出・試行2・操作結果とprivate非開示を保持する', 'persistence'],
]);
const actions = new Set(['journey-setup', 'office-navigation', 'sales-navigation', 'document-navigation', 'task-navigation',
  'draft-save', 'source-read', 'evidence-module', 'source-document-select', 'source-file-select', 'evidence-input',
  'evidence-submit', 'finding-input', 'finding-submit', 'decision-select', 'decision-input', 'decision-preview',
  'decision-confirm', 'visibility-verify', 'submit-preview', 'submit-selection', 'submit-confirm', 'office-claim',
  'return-preview', 'return-confirm', 'sales-reclaim', 'resubmit', 'office-reclaim', 'final-verify', 'persistence-verify', 'agent-module', 'agent-input', 'agent-request', 'agent-result', 'agent-replay']);
const matchers = new Set(['toBe', 'toEqual', 'toStrictEqual', 'toMatchObject', 'toMatch', 'toContain', 'toContainEqual',
  'toBeNull', 'toBeVisible', 'toBeHidden', 'toBeFocused', 'toBeEnabled', 'toBeDisabled', 'toBeChecked',
  'toHaveCount', 'toHaveText', 'toContainText', 'toHaveURL', 'toHaveAttribute', 'toHaveLength', 'toHaveValue',
  'toHaveProperty', 'toBeGreaterThan', 'toBeLessThan', 'toBeGreaterThanOrEqual', 'toBeLessThanOrEqual']);
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = value => typeof value === 'string' ? value.slice(0, MAX_TEXT).replace(/\x1b\[[0-9;]*m/gu, '') : '';
const coordinate = value => Number.isInteger(value) && value > 0 && value <= 1_000_000;
const unavailable = phase => ({ ...(phases.has(phase) ? { phase } : {}), availability: 'unavailable' });

function location(value) {
  if (!object(value) || typeof value.file !== 'string' || value.file.length > MAX_TEXT) return undefined;
  const source = value.file.split(/[\\/]/u).at(-1);
  return sources.has(source) ? { source, ...(coordinate(value.line) ? { line: value.line } : {}),
    ...(coordinate(value.column) ? { column: value.column } : {}) } : undefined;
}
function stackLocation(value) {
  const match = text(value).match(/(?:^|[\\/\s(])((?:journey|persistence)\.spec\.ts|support\.ts):(\d{1,7}):(\d{1,7})(?:\D|$)/u);
  return match ? location({ file: match[1], line: Number(match[2]), column: Number(match[3]) }) : undefined;
}
function failure(result, title) {
  const error = result.error, message = text(error?.message);
  // Pinned standard JSON retains test.info().annotations on this exact result, including timeout.
  // This is only the last action entered, not evidence that it is waiting or completed.
  const stage = Array.isArray(result.annotations) && result.annotations.length <= 32
    ? result.annotations.findLast(annotation => annotation?.type === 'organization-stage')?.description : undefined;
  const name = error?.matcherResult?.name ?? message.match(/\b(to[A-Z][A-Za-z]+)\s*\(/u)?.[1];
  const matcher = matchers.has(name) ? name : undefined;
  let errorCategory = 'unavailable';
  if (/strict mode violation/iu.test(message)) errorCategory = 'strict-locator';
  else if (result.status === 'timedOut' || /test timeout of/iu.test(message)) errorCategory = 'test-timeout';
  else if (/timeout/iu.test(message) && /locator(?:\.|\))|expect\(locator\)/iu.test(message)) errorCategory = 'locator-timeout';
  else if (matcher || /assertion(?:error| failed)|expect\(/iu.test(message)) errorCategory = 'assertion';
  return { ...(tests.has(title) ? { test: tests.get(title) } : {}),
    ...(location(error?.location) ?? location(result.errorLocation) ?? stackLocation(error?.stack)),
    status: result.status, errorCategory, ...(matcher ? { matcher } : {}),
    ...(actions.has(stage) ? { currentAction: stage } : {}) };
}

export function browserFailureDiagnostics(raw, phase) {
  if (!phases.has(phase) || typeof raw !== 'string' || Buffer.byteLength(raw) > MAX_BYTES) return unavailable(phase);
  let report;
  try { report = JSON.parse(raw); } catch { return unavailable(phase); }
  if (!object(report)) return unavailable(phase);
  if (Array.isArray(report.errors) && object(report.errors[0])) {
    return { phase, availability: 'available', failure: failure({ status: 'failed', error: report.errors[0] }) };
  }
  const queue = [{ node: report, depth: 0 }];
  for (let index = 0; index < queue.length && index < MAX_NODES; index++) {
    const { node, depth, title } = queue[index];
    if (!object(node)) continue;
    if (depth > MAX_DEPTH) return unavailable(phase);
    const result = Array.isArray(node.results) ? node.results.at(-1) : undefined;
    if (statuses.has(result?.status)) return { phase, availability: 'available', failure: failure(result, title) };
    for (const key of ['suites', 'specs', 'tests']) {
      if (!Array.isArray(node[key])) continue;
      if (queue.length + node[key].length > MAX_NODES) return unavailable(phase);
      queue.push(...node[key].map(child => ({ node: child, depth: depth + 1, title: key === 'tests' ? node.title : title })));
    }
  }
  return unavailable(phase);
}

export async function readBrowserFailureDiagnostics(runDirectory, phase) {
  if (!phases.has(phase)) return unavailable(phase);
  let file;
  try {
    const root = resolve(runDirectory), directory = join(root, `browser-${phase}`);
    for (const path of [root, directory]) {
      const info = await lstat(path);
      if (!info.isDirectory() || info.isSymbolicLink() || await realpath(path) !== path) return unavailable(phase);
    }
    file = await open(join(directory, 'results.json'), constants.O_RDONLY | constants.O_NOFOLLOW);
    const info = await file.stat();
    if (!info.isFile() || info.size > MAX_BYTES) return unavailable(phase);
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
      if (!bytesRead) break;
      length += bytesRead;
    }
    return length > MAX_BYTES ? unavailable(phase) : browserFailureDiagnostics(buffer.subarray(0, length).toString('utf8'), phase);
  } catch { return unavailable(phase); }
  finally { await file?.close().catch(() => {}); }
}
