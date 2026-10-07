import { defineConfig, devices } from '@playwright/test';
import { join } from 'node:path';
import { readRuntimeContext } from './e2e-organization/support';

const context = readRuntimeContext();
const phase = process.env.KP_ORGANIZATION_RUNTIME_PHASE;
const output = process.env.KP_ORGANIZATION_BROWSER_OUTPUT;
// Two-principal phases plus the separate fresh-database six-principal policy and context phases.
// Exact basenames: `journey` must never also select `policy-journey` or `context-journey`.
const specs: Record<string, RegExp> = { journey: /(?:^|[\\/])journey\.spec\.ts$/u, persistence: /(?:^|[\\/])persistence\.spec\.ts$/u, 'policy-journey': /(?:^|[\\/])policy-journey\.spec\.ts$/u, 'policy-persistence': /(?:^|[\\/])policy-persistence\.spec\.ts$/u, 'context-journey': /(?:^|[\\/])context-journey\.spec\.ts$/u, 'context-persistence': /(?:^|[\\/])context-persistence\.spec\.ts$/u };
if (!output || !phase || !Object.hasOwn(specs, phase)) {
  throw new Error('Organization harness must supply its output directory and a known phase');
}
// Raw standard JSON stays in the private harness directory; only a closed failure projection is logged.
export default defineConfig({
  testDir: './e2e-organization',
  testMatch: specs[phase]!,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  outputDir: join(output, 'artifacts'),
  preserveOutput: 'never',
  reporter: [['list'], ['json', { outputFile: join(output, 'results.json') }]],
  captureGitInfo: { commit: false, diff: false },
  use: {
    baseURL: context.sales,
    browserName: 'chromium',
    locale: 'ja-JP',
    viewport: { width: 1440, height: 900 },
    trace: 'off',
    screenshot: 'off',
    video: 'off',
    // Only the synthetic journey reads/deletes private temporary original downloads.
    acceptDownloads: phase === 'journey',
    serviceWorkers: 'block',
  },
  // No webServer, channel or executablePath: only the pinned bundled Chromium.
  projects: [{ name: 'organization-two-principal-chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } }],
});
