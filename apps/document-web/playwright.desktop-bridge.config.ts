import { defineConfig, devices } from '@playwright/test';

// Same React production build served by the bounded preview, with the desktop
// adapter wired to the real broker through a TEST-ONLY stdio bridge. Requires
// `cargo build -p local-workspace-runtime --example stdio_bridge` first.
export default defineConfig({
  testDir: './e2e-desktop',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:8080',
    browserName: 'chromium',
    locale: 'ja-JP',
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    ...(process.env.KP_CHROMIUM_EXECUTABLE ? { launchOptions: { executablePath: process.env.KP_CHROMIUM_EXECUTABLE } } : {}),
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: 'node scripts/preview.mjs',
    url: 'http://127.0.0.1:8080/index.html',
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
