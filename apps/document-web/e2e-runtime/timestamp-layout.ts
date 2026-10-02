import assert from 'node:assert/strict';
import type { Browser } from '@playwright/test';
import { formatDateTime } from '../src/view-model/date-time.ts';
import { assertApplicationJapaneseFonts } from './japanese-font.ts';
import type { runtime as readRuntime } from './support';

type Rect = { left: number; top: number; right: number; bottom: number };
type TimestampGeometry = {
  viewport: Rect; scroller: Rect; cell: Rect; row: Rect;
  previousRow: Rect; nextRow: Rect; previousCell: Rect; nextCell: Rect; fragments: Rect[];
};

// Range fragments include text hidden by overflow/ellipsis, unlike element boxes.
// Only subpixel rounding is tolerated; an expanded row cannot evade the 48px slot.
export function assertTimestampGeometry(value: TimestampGeometry): void {
  const epsilon = 0.5;
  const valid = (r: Rect) => [r.left, r.top, r.right, r.bottom].every(Number.isFinite)
    && r.right > r.left && r.bottom > r.top;
  const contains = (outer: Rect, inner: Rect) => inner.left >= outer.left - epsilon
    && inner.right <= outer.right + epsilon && inner.top >= outer.top - epsilon && inner.bottom <= outer.bottom + epsilon;
  assert.ok(value.fragments.length > 0 && value.fragments.length <= 100, 'Timestamp text fragments unavailable');
  assert.ok([value.viewport, value.scroller, value.cell, value.row, value.previousRow, value.nextRow,
    value.previousCell, value.nextCell, ...value.fragments].every(valid), 'Timestamp geometry is invalid');
  for (const row of [value.previousRow, value.row, value.nextRow]) {
    assert.ok(Math.abs(row.bottom - row.top - 48) <= epsilon, 'Timestamp virtual row must remain48px');
  }
  assert.ok(Math.abs(value.row.top - value.previousRow.top - 48) <= epsilon
    && Math.abs(value.nextRow.top - value.row.top - 48) <= epsilon
    && value.previousRow.bottom <= value.row.top + epsilon && value.row.bottom <= value.nextRow.top + epsilon,
  'Timestamp neighboring virtual rows overlap or drift');
  for (const fragment of value.fragments) {
    assert.ok([value.viewport, value.scroller, value.cell].every(bounds => contains(bounds, fragment)), 'Timestamp text is clipped');
    assert.ok(fragment.top >= value.row.top - epsilon && fragment.bottom <= value.row.bottom + epsilon
      && fragment.top >= value.previousRow.bottom - epsilon && fragment.bottom <= value.nextRow.top + epsilon,
    'Timestamp text overlaps a neighboring row');
    assert.ok(fragment.left >= value.previousCell.right - epsilon && fragment.right <= value.nextCell.left + epsilon,
      'Timestamp text overlaps a neighboring cell');
  }
}

// Browser layout evidence only: no API mocking, backend timestamp update, or
// persistence/equality claim. This context loads the actual built app and data;
// its disconnected React root is replaced by an inert DOM clone before changing
// display text. Production CSS, machine-readable attributes and inputs are intact.
export async function assertTimestampBrowserLayout(browser: Browser, runtime: Awaited<ReturnType<typeof readRuntime>>): Promise<void> {
  const cases = [
    { timezoneId: 'America/North_Dakota/New_Salem', instants: ['2026-07-01T12:30:00Z'], offsets: ['UTC-05:00'] },
    { timezoneId: 'America/New_York', instants: ['2026-11-01T05:30:00Z', '2026-11-01T06:30:00Z'], offsets: ['UTC-04:00', 'UTC-05:00'] },
  ];
  for (const sample of cases) {
    // Explicit options retain the real journey's browser contract. These manually
    // owned contexts enable no recording and never enter a capture checkpoint.
    const context = await browser.newContext({ baseURL: runtime.human, timezoneId: sample.timezoneId,
      locale: 'ja-JP', deviceScaleFactor: 1, serviceWorkers: 'block', viewport: { width: 1440, height: 900 } });
    try {
      const page = await context.newPage();
      let nonReadRequest = false;
      context.on('request', request => { if (!['GET', 'HEAD'].includes(request.method())) nonReadRequest = true; });
      for (const width of [1280, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(`/documents?view=published&folderId=${runtime.manifest.folders.shared.folderId}&panel=open&selectedDocumentId=${runtime.manifest.documents.regulation!.create!.result!.documentId}`);
        await page.waitForFunction(() => document.querySelectorAll('[data-document-table-scroll] [role="rowgroup"] [role="row"] time').length >= 3
          && !document.querySelector('[aria-busy="true"]') && !document.getAnimations().some(animation =>
            (animation.playState === 'running' || animation.pending) && Number.isFinite(animation.effect?.getComputedTiming().iterations)),
        undefined, { timeout: 15_000 });
        await assertApplicationJapaneseFonts(page);
        await page.evaluate(() => {
          const root = document.getElementById('root');
          if (!root) throw Error('Timestamp application root unavailable');
          const clone = root.cloneNode(true);
          root.replaceWith(clone);
        });
        let firstWallTime: string | undefined;
        for (const [index, instant] of sample.instants.entries()) {
          // Playwright serializes the actual product function into this Chromium
          // timezone context; Node's Intl never supplies the display text.
          const label = await page.evaluate(formatDateTime, instant);
          const suffix = ` (${sample.timezoneId}, ${sample.offsets[index]})`;
          assert.ok(label.endsWith(suffix), 'Timestamp browser formatter zone or offset differs');
          const wallTime = label.slice(0, -suffix.length);
          if (sample.instants.length === 2) {
            assert.ok(wallTime === '2026/11/01 1:30' && (!firstWallTime || firstWallTime === wallTime), 'Timestamp fold wall times differ');
            firstWallTime = wallTime;
          }
          const geometry = await page.evaluate(async displayText => {
            const scroller = document.querySelector<HTMLElement>('[data-document-table-scroll]');
            const rows = [...document.querySelectorAll<HTMLElement>('[data-document-table-scroll] [role="rowgroup"] [role="row"]')];
            const row = rows[1], previousRow = rows[0], nextRow = rows[2];
            if (!scroller || !row || !previousRow || !nextRow) throw Error('Timestamp fixture rows unavailable');
            for (const current of rows) {
              const time = current.querySelector('time');
              if (!time) throw Error('Timestamp fixture time unavailable');
              time.textContent = displayText;
            }
            const time = row.querySelector('time')!, cell = time.parentElement!;
            const previousCell = cell.previousElementSibling, nextCell = cell.nextElementSibling;
            if (!previousCell || !nextCell) throw Error('Timestamp neighboring cells unavailable');
            // Expose the complete timestamp through the existing horizontal scroller.
            scroller.scrollLeft += cell.getBoundingClientRect().left - scroller.getBoundingClientRect().left - scroller.clientLeft - 8;
            await document.fonts.ready;
            await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
            const rect = (value: DOMRect): Rect => ({ left: value.left, top: value.top, right: value.right, bottom: value.bottom });
            const range = document.createRange(); range.selectNodeContents(time);
            const style = getComputedStyle(cell), bounds = cell.getBoundingClientRect(), scroll = scroller.getBoundingClientRect();
            if (time.textContent !== displayText || getComputedStyle(time).visibility !== 'visible'
              || document.documentElement.scrollWidth > document.documentElement.clientWidth) throw Error('Timestamp fixture visibility differs');
            return {
              viewport: { left: 0, top: 0, right: innerWidth, bottom: innerHeight },
              scroller: { left: scroll.left + scroller.clientLeft, top: scroll.top + scroller.clientTop,
                right: scroll.left + scroller.clientLeft + scroller.clientWidth, bottom: scroll.top + scroller.clientTop + scroller.clientHeight },
              cell: { left: bounds.left + parseFloat(style.paddingLeft), right: bounds.right - parseFloat(style.paddingRight), top: bounds.top, bottom: bounds.bottom },
              row: rect(row.getBoundingClientRect()), previousRow: rect(previousRow.getBoundingClientRect()), nextRow: rect(nextRow.getBoundingClientRect()),
              previousCell: rect(previousCell.getBoundingClientRect()), nextCell: rect(nextCell.getBoundingClientRect()),
              fragments: [...range.getClientRects()].map(rect),
            };
          }, label);
          assertTimestampGeometry(geometry);
        }
      }
      assert.ok(!nonReadRequest, 'Timestamp display fixture issued a non-read request');
    } finally { await context.close(); }
  }
}
