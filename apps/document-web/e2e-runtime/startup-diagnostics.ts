import type { Page } from '@playwright/test';

type Startup = { rootChildren: number; documentStatus: number; scriptResponses: number; scriptFailures: number;
  apiResponses: number; apiFailures: number; cspViolations: number; consoleErrors: number; pageErrors: string[];
  domObservation: 'available' | 'unavailable'; apiEvents: Array<{ route: string; status: number }>;
  uiSnapshots: Array<{ stage: string; availability: 'available' | 'unavailable'; [key: string]: string | number }> };
type UiStage = 'before-folder-wait' | 'before-folder-click' | 'after-folder-click' | 'test-end';
const records = new WeakMap<Page, Startup>();
const bounded = (value: number) => Math.min(value + 1, 1000);
async function observeDom<T>(operation: Promise<T>): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error('DOM observation unavailable')), 1000);
    })]);
  } finally { if (timer !== undefined) clearTimeout(timer); }
}
export async function startDiagnostics(page: Page): Promise<void> {
  const record: Startup = { rootChildren: 0, documentStatus: 0, scriptResponses: 0, scriptFailures: 0,
    apiResponses: 0, apiFailures: 0, cspViolations: 0, consoleErrors: 0, pageErrors: [],
    domObservation: 'unavailable', apiEvents: [], uiSnapshots: [] };
  records.set(page, record);
  page.on('pageerror', error => {
    if (record.pageErrors.length >= 10) return;
    const message = error.message.slice(0, 4096);
    record.pageErrors.push(/unsafe-eval|Refused to evaluate|code generation.*disallowed/i.test(message) ? 'eval-blocked'
      : /require is not defined/.test(message) ? 'require-undefined'
      : /exports is not defined/.test(message) ? 'exports-undefined'
      : /process is not defined/.test(message) ? 'process-undefined'
      : error.name === 'ReferenceError' ? 'reference-error'
      : error.name === 'TypeError' ? 'type-error' : error.name === 'SyntaxError' ? 'syntax-error'
      : /Loading chunk|ChunkLoadError/.test(message) ? 'chunk-load' : 'other');
  });
  page.on('console', message => { if (message.type() === 'error') record.consoleErrors = bounded(record.consoleErrors); });
  page.on('response', response => {
    const type = response.request().resourceType(), status = response.status();
    if (type === 'document') record.documentStatus = status;
    if (type === 'script') { record.scriptResponses = bounded(record.scriptResponses); if (status >= 400) record.scriptFailures = bounded(record.scriptFailures); }
    const path = new URL(response.url()).pathname;
    if (path.startsWith('/v1/')) {
      record.apiResponses = bounded(record.apiResponses); if (status >= 400) record.apiFailures = bounded(record.apiFailures);
      if (record.apiEvents.length < 12) record.apiEvents.push({ status, route: path === '/v1/folders/root' ? 'folder-root'
        : /^\/v1\/folders\/[^/]+\/children$/.test(path) ? 'folder-children' : path === '/v1/documents' ? 'document-list'
        : /^\/v1\/documents\/[^/]+$/.test(path) ? 'document-detail' : path === '/v1/session' ? 'session' : 'other' });
    }
  });
  page.on('requestfailed', request => { if (request.resourceType() === 'script') record.scriptFailures = bounded(record.scriptFailures); });
  await page.addInitScript(() => {
    const state = window as unknown as { __kpRuntimeCspCount: number };
    state.__kpRuntimeCspCount = 0;
    document.addEventListener('securitypolicyviolation', () => { state.__kpRuntimeCspCount = Math.min(state.__kpRuntimeCspCount + 1, 1000); });
  });
}
export async function captureUiDiagnostics(page: Page, stage: UiStage): Promise<void> {
  const record = records.get(page);
  if (!record || record.uiSnapshots.length >= 4) return;
  try {
    const observed = await observeDom(page.evaluate(() => {
      const targets = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).filter(button => button.textContent?.trim() === 'PoC Shared');
      const count = (value: number) => Math.min(value, 1000);
      let inViewportCount = 0, receivesPointerCount = 0, disabledCount = 0, hiddenAncestorCount = 0;
      for (const button of targets.slice(0, 1000)) {
        const box = button.getBoundingClientRect();
        if (button.disabled) disabledCount++;
        if (button.closest('[inert], [aria-hidden="true"]')) hiddenAncestorCount++;
        if (box.width <= 0 || box.height <= 0 || box.right <= 0 || box.bottom <= 0 || box.left >= innerWidth || box.top >= innerHeight) continue;
        inViewportCount++;
        const x = (Math.max(0, box.left) + Math.min(innerWidth, box.right)) / 2;
        const y = (Math.max(0, box.top) + Math.min(innerHeight, box.bottom)) / 2;
        const hit = document.elementFromPoint(x, y);
        if (hit && button.contains(hit)) receivesPointerCount++;
      }
      return { viewportWidth: innerWidth, viewportHeight: innerHeight, rootChildren: count(document.getElementById('root')?.childElementCount ?? 0),
        folderRegionCount: count(document.querySelectorAll('section[aria-label="フォルダー"], [role="region"][aria-label="フォルダー"]').length),
        sharedFolderButtonCount: count(targets.length), tableCount: count(document.querySelectorAll('table, [role="table"]').length),
        alertCount: count(document.querySelectorAll('[role="alert"]').length), inViewportCount, receivesPointerCount, disabledCount, hiddenAncestorCount };
    }));
    record.uiSnapshots.push({ stage, availability: 'available', ...observed });
  } catch { record.uiSnapshots.push({ stage, availability: 'unavailable' }); }
}
export async function finishDiagnostics(page: Page): Promise<Startup | undefined> {
  const record = records.get(page);
  if (!record) return;
  await captureUiDiagnostics(page, 'test-end');
  try {
    const observed = await observeDom(page.evaluate(() => ({ rootChildren: Math.min(document.getElementById('root')?.childElementCount ?? 0, 1000),
      cspViolations: (window as unknown as { __kpRuntimeCspCount?: number }).__kpRuntimeCspCount ?? 0 })));
    Object.assign(record, observed);
    record.domObservation = 'available';
  } catch { /* Closed pages preserve only already-observed bounded events. */ }
  return record;
}
