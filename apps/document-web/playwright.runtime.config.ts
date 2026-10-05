import { defineConfig, devices } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const contextPath = process.env.KP_POC_RUNTIME_CONTEXT;
const phase = process.env.KP_POC_RUNTIME_PHASE;
const output = process.env.KP_POC_BROWSER_OUTPUT;
if (!contextPath || !output || !['journey', 'persistence'].includes(phase ?? '')) {
  throw new Error('Run the owned composition-root harness; runtime context, phase, and output directory are required');
}
const context = JSON.parse(readFileSync(contextPath, 'utf8')) as { human: string; agent: string; visualCapture?: unknown };
for (const origin of [context.human, context.agent]) {
  const url = new URL(origin);
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Runtime acceptance requires distinct loopback HTTP origins');
  }
}
if (context.human === context.agent) throw new Error('Human and agent must be separate processes');

export default defineConfig({
  testDir: './e2e-runtime',
  testMatch: phase === 'journey' ? ['document-runtime.spec.ts', 'initial-registration.spec.ts', 'metadata-editor.spec.ts', 'lifecycle-operations.spec.ts', 'document-schedule-cancellation.spec.ts', 'human-agent-consistency.spec.ts', 'worker-failure.spec.ts', 'timestamp-layout.spec.ts'] : ['persistence.spec.ts', 'metadata-editor.spec.ts', 'lifecycle-operations-persistence.spec.ts', 'document-schedule-cancellation.spec.ts'],
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  outputDir: join(output, 'artifacts'),
  reporter: [['list'], ['json', { outputFile: join(output, 'results.json') }], ['junit', { outputFile: join(output, 'results.xml') }]],
  use: {
    baseURL: context.human,
    locale: 'ja-JP',
    viewport: { width: 1440, height: 900 },
    trace: context.visualCapture ? 'off' : 'retain-on-failure',
    screenshot: context.visualCapture ? 'off' : 'only-on-failure',
    video: context.visualCapture ? 'off' : 'retain-on-failure',
    serviceWorkers: 'block',
  },
  // Intentionally no webServer, channel or executablePath: use the pinned Playwright Chromium.
  projects: [{ name: 'production-composition-chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } }],
});
