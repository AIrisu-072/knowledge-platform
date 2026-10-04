import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const app = new URL('../', import.meta.url);
const repo = new URL('../../', app);
const text = async (base, path) => readFile(new URL(path, base), 'utf8');

test('preview keeps the exact production build and separates asset-only startup', async () => {
  const pkg = JSON.parse(await text(app, 'package.json'));
  assert.equal(pkg.scripts.build, 'pnpm schemas:check && tsc -p tsconfig.json && webpack --config webpack.config.cjs --mode production');
  assert.equal(pkg.scripts.dev, 'pnpm build && node scripts/preview.mjs');
  assert.equal(pkg.scripts.e2e, 'pnpm build && playwright test');
  assert.equal(pkg.scripts.preview, 'node scripts/preview.mjs');
  assert.equal(pkg.scripts['test:preview'], 'node --test scripts/preview*.test.mjs');
  assert.equal(pkg.devDependencies['webpack-dev-server'], undefined);
  assert.equal(pkg.devDependencies.webpack, '5.111.1');
  assert.equal(pkg.devDependencies['webpack-cli'], '7.2.3');
});

test('mock startup remains owned loopback with the same functional browser settings', async () => {
  const config = await text(app, 'playwright.config.ts');
  for (const expected of ["command: 'node scripts/preview.mjs'", "url: 'http://127.0.0.1:8080/index.html'", 'reuseExistingServer: false', 'timeout: 60_000', "baseURL: 'http://127.0.0.1:8080'", 'workers: 1', 'retries: 0', "locale: 'ja-JP'", 'viewport: { width: 1440, height: 900 }']) {
    assert.ok(config.includes(expected), expected);
  }
  assert.doesNotMatch(config, /webpack serve|updateSnapshots/);
});

test('required mise entrypoint runs preview qualification before unchanged real acceptance', async () => {
  const mise = await text(repo, 'mise.toml');
  const preview = mise.split('[tasks."document:preview:qualify"]')[1]?.split('\n[tasks.')[0];
  assert.ok(preview, 'missing required preview qualification task');
  for (const command of ['pnpm --filter @knowledge-platform/document-web test:preview', 'pnpm --filter @knowledge-platform/document-web test', 'pnpm --filter @knowledge-platform/document-web e2e']) {
    assert.ok(preview.includes(`"${command}"`), command);
  }
  const runtime = mise.split('[tasks."document:poc:runtime"]')[1].split('\n[tasks.')[0];
  assert.equal(runtime.split('mise run document:preview:qualify').length, 2);
  assert.ok(runtime.indexOf('mise run document:preview:qualify') < runtime.indexOf('node tools/document-poc-runtime/run.mjs'));
  assert.match(runtime, /\nnode tools\/document-poc-runtime\/run\.mjs\n/);
});

test('mock visual baseline condition and the real runtime entry remain distinct', async () => {
  const mock = await text(app, 'e2e/document-workspace.spec.ts');
  assert.match(mock, /process\.platform === 'darwin'/);
  assert.match(mock, /toHaveScreenshot\(name/);
  const real = await text(app, 'playwright.runtime.config.ts');
  assert.match(real, /Intentionally no webServer/);
  assert.doesNotMatch(real, /scripts\/preview\.mjs|webpack serve/);
});
