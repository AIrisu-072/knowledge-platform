import type { Page } from '@playwright/test';

type Startup = { rootChildren: number; documentStatus: number; scriptResponses: number; scriptFailures: number;
  apiResponses: number; apiFailures: number; cspViolations: number; consoleErrors: number; pageErrors: string[] };
const records = new WeakMap<Page, Startup>();
const bounded = (value: number) => Math.min(value + 1, 1000);
export async function startDiagnostics(page: Page): Promise<void> {
  const record: Startup = { rootChildren: 0, documentStatus: 0, scriptResponses: 0, scriptFailures: 0,
    apiResponses: 0, apiFailures: 0, cspViolations: 0, consoleErrors: 0, pageErrors: [] };
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
    if (new URL(response.url()).pathname.startsWith('/v1/')) { record.apiResponses = bounded(record.apiResponses); if (status >= 400) record.apiFailures = bounded(record.apiFailures); }
  });
  page.on('requestfailed', request => { if (request.resourceType() === 'script') record.scriptFailures = bounded(record.scriptFailures); });
  await page.addInitScript(() => {
    const state = window as unknown as { __kpRuntimeCspCount: number };
    state.__kpRuntimeCspCount = 0;
    document.addEventListener('securitypolicyviolation', () => { state.__kpRuntimeCspCount = Math.min(state.__kpRuntimeCspCount + 1, 1000); });
  });
}
export async function finishDiagnostics(page: Page): Promise<Startup | undefined> {
  const record = records.get(page);
  if (!record) return;
  try {
    const observed = await page.evaluate(() => ({ rootChildren: Math.min(document.getElementById('root')?.childElementCount ?? 0, 1000),
      cspViolations: (window as unknown as { __kpRuntimeCspCount?: number }).__kpRuntimeCspCount ?? 0 }));
    Object.assign(record, observed);
  } catch { /* Closed pages preserve only already-observed bounded events. */ }
  return record;
}
