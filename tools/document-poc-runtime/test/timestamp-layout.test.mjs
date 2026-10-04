import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

// Oracle/wiring qualification only. Actual Chromium layout runs in hosted acceptance.
const api = await import('../../../apps/document-web/e2e-runtime/timestamp-layout.ts').catch(error => {
  if (error.code === 'ERR_MODULE_NOT_FOUND') return {};
  throw error;
});
const rect = (left, top, right, bottom) => ({ left, top, right, bottom });
const good = () => ({
  viewport: rect(0, 0, 1280, 900), scroller: rect(250, 200, 900, 600),
  cell: rect(480, 253, 848, 292), row: rect(250, 248, 900, 296),
  previousRow: rect(250, 200, 900, 248), nextRow: rect(250, 296, 900, 344),
  previousCell: rect(430, 248, 472, 296), nextCell: rect(856, 248, 900, 296),
  fragments: [rect(480, 253, 600, 272), rect(480, 272, 820, 291)],
});

test('timestamp geometry accepts complete two-line text in the padded cell and 48px row', () => {
  assert.equal(typeof api.assertTimestampGeometry, 'function');
  assert.doesNotThrow(() => api.assertTimestampGeometry(good()));
});

test('timestamp geometry rejects clipping, ellipsis, row overlap, invisible text and invalid measurements', () => {
  assert.equal(typeof api.assertTimestampGeometry, 'function');
  const invalid = [
    { fragments: [rect(480, 253, 849, 272)] }, // clipped/ellipsized horizontal text
    { fragments: [rect(479, 253, 820, 272)] },
    { fragments: [rect(480, 252, 820, 272)] },
    { fragments: [rect(480, 272, 820, 297)] }, // next virtual row
    { row: rect(250, 248, 900, 306) }, // expanded row overlaps fixed virtual slot
    { previousRow: rect(250, 200, 900, 249) },
    { nextRow: rect(250, 295, 900, 343) },
    { previousCell: rect(430, 248, 490, 296) },
    { nextCell: rect(810, 248, 900, 296) },
    { scroller: rect(500, 200, 900, 600) }, // left side remains scrolled out
    { scroller: rect(250, 200, 819, 600) },
    { viewport: rect(0, 0, 1280, 270) },
    { fragments: [] }, { fragments: [rect(480, 253, 480, 272)] },
    { fragments: [rect(NaN, 253, 820, 272)] },
    { fragments: [rect(480, 253, Infinity, 272)] },
  ];
  for (const patch of invalid) assert.throws(() => api.assertTimestampGeometry({ ...good(), ...patch }), /Timestamp/);
});

test('normal journey wires isolated real Chromium timestamp fixtures without new capture paths', async () => {
  const source = await readFile(new URL('../../../apps/document-web/e2e-runtime/timestamp-layout.spec.ts', import.meta.url), 'utf8');
  const config = await readFile(new URL('../../../apps/document-web/playwright.runtime.config.ts', import.meta.url), 'utf8');
  assert.match(config, /testMatch: phase === 'journey' \? \[[^\]]*'timestamp-layout\.spec\.ts'/);
  assert.match(source, /test\.use\(\{ trace: 'off', screenshot: 'off', video: 'off' \}\)/);
  assert.doesNotMatch(source, /startDiagnostics|finishDiagnostics|\.attach\(|visualCheckpoint/);
  assert.match(source, /test\('browser-only timestamp layout covers long IANA and both DST folds at1280\/1440'/);
  assert.match(source, /await assertTimestampBrowserLayout\(browser, context\);\s*test\.info\(\)\.annotations\.push\(\{ type: 'runtime-timestamp-layout', description: 'long-iana-both-folds-1280-1440' \}\)/);
  const helper = await readFile(new URL('../../../apps/document-web/e2e-runtime/timestamp-layout.ts', import.meta.url), 'utf8');
  assert.match(helper, /import \{ formatDateTime \} from '\.\.\/src\/view-model\/date-time\.ts'/);
  assert.match(helper, /page\.evaluate\(formatDateTime, instant\)/);
  assert.match(helper, /\[1280, 1440\]/);
  assert.match(helper, /America\/North_Dakota\/New_Salem/);
  assert.match(helper, /America\/New_York/);
  assert.match(helper, /2026-11-01T05:30:00Z/);
  assert.match(helper, /2026-11-01T06:30:00Z/);
  for (const required of ['cloneNode(true)', 'replaceWith(clone)', 'createRange()', 'getClientRects()', 'scrollLeft', "locale: 'ja-JP'", 'deviceScaleFactor: 1', "serviceWorkers: 'block'", 'finally { await context.close(); }']) assert.ok(helper.includes(required), required);
  assert.doesNotMatch(helper, /visualCheckpoint|screenshot\(|\.route\(|recordVideo|tracing|setContent\(|addStyleTag\(/);
});
