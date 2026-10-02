#!/usr/bin/env node
// Hosted-only Source Design qualification. No product build or local browser path.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { appendFile, mkdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { snapshotSource, startSourceServer, STATES, MODULES, regularDirectory } from './source-server.mjs';
import { REVIEW, currentReviewSubject, readGitHubInput, reviewGate, requireHostedPrerequisites, requireLiveReview } from './review-gate.mjs';
import { assertFonts, selectScenario, assertGeometry, assertKeyboard, assertReducedMotion, requestAllowed, screenshotOptions, safeFailure } from './browser-checks.mjs';
import { validatePng, exportPixels } from './pixels.mjs';

let category = 'environment', server, browser, privateRoot;
const git = args => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 1024 * 1024 }).trim();
try {
  const mode = process.argv[2];
  assert.ok(process.argv.length === 3 && ['normal', 'capture'].includes(mode), 'environment');
  assert.ok(process.env.GITHUB_ACTIONS === 'true' && process.env.RUNNER_OS === 'Linux' && process.platform === 'linux'
    && process.versions.node === '24.21.0' && process.env.RUNNER_TEMP && process.env.GITHUB_OUTPUT, 'environment');
  assert.equal(execFileSync('pnpm', ['--version'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim(), '12.4.1', 'environment');
  const require = createRequire(resolve('apps/document-web/package.json'));
  assert.equal(require('@playwright/test/package.json').version, '1.63.0', 'environment');
  const { chromium } = require('@playwright/test');
  const input = readGitHubInput(), sourceRoot = resolve('docs/design/organization-client-v0');
  const head = input.localHead, tree = git(['rev-parse', 'HEAD^{tree}']);
  assert.ok(!input.gitDirty && /^[a-f0-9]{40}$/.test(head) && /^[a-f0-9]{40}$/.test(tree), 'source');
  assert.ok(input.eventName === 'pull_request' && input.repository === REVIEW.repository
    && input.event.number === REVIEW.pr && currentReviewSubject(input.event.pull_request, head), 'source');
  if (mode === 'normal') assert.ok(['opened', 'synchronize', 'reopened'].includes(input.event.action), 'gate');
  category = 'gate';
  const gate = reviewGate(input);
  let prerequisites;
  if (mode === 'capture') {
    assert.ok(gate.enabled, 'gate'); category = 'prerequisites';
    prerequisites = await requireHostedPrerequisites(gate); await requireLiveReview(gate);
  }
  category = 'source';
  const snapshot = await snapshotSource(sourceRoot);
  for (const source of snapshot.identity) assert.equal(source.blob, git(['rev-parse', `HEAD:docs/design/organization-client-v0/${source.name}`]), 'source');
  const scenarios = JSON.parse(snapshot.files.get('scenarios.json').toString('utf8')).scenarios;
  assert.deepEqual(scenarios.map(s => s.id), STATES, 'source'); assert.deepEqual(scenarios.map(s => s.module), MODULES, 'source');
  const jsdom = createRequire(require.resolve('jest-environment-jsdom')).resolve('jsdom');
  // Raw Node assertion output may contain source/DOM bodies. It stays captured,
  // never printed, persisted, or uploaded. A failure emits only the category.
  execFileSync(process.execPath, ['--test', join(sourceRoot, 'source-design.test.mjs'), join(sourceRoot, 'interaction.test.mjs')], {
    env: { ...process.env, ORG_DESIGN_JSDOM: jsdom }, stdio: ['ignore', 'pipe', 'pipe'], timeout: 30_000, maxBuffer: 1024 * 1024,
  });
  assert.ok(/^[1-9][0-9]{0,19}$/.test(input.runId) && /^[1-9][0-9]{0,2}$/.test(input.runAttempt), 'environment');
  await regularDirectory(process.env.RUNNER_TEMP);
  privateRoot = join(process.env.RUNNER_TEMP, `organization-d2-${input.runId}-${input.runAttempt}`);
  await mkdir(privateRoot, { mode: 0o700 }); await regularDirectory(privateRoot, true);
  const capture = join(privateRoot, 'capture');
  if (mode === 'capture') await mkdir(capture, { mode: 0o700 });
  server = await startSourceServer(snapshot);
  category = 'browser';
  // Explicit small environment: Actions credentials never enter Chromium.
  const browserEnvironment = Object.fromEntries(['PATH', 'HOME', 'LANG', 'LC_ALL', 'FONTCONFIG_FILE'].filter(key => process.env[key]).map(key => [key, process.env[key]]));
  browser = await chromium.launch({ headless: true, env: browserEnvironment,
    args: ['--disable-background-networking', '--disable-component-update', '--disable-sync', '--no-first-run'] });
  async function withPage(archetype, width, work) {
    let violation = false;
    const context = await browser.newContext({ viewport: { width, height: 900 }, deviceScaleFactor: 1, locale: 'ja-JP', timezoneId: 'Asia/Tokyo', colorScheme: 'light', reducedMotion: 'no-preference', serviceWorkers: 'block', acceptDownloads: false });
    try {
      await context.route('**/*', async route => {
        if (!requestAllowed(server.origin, route.request().url(), route.request().method())) { violation = true; await route.abort(); } else await route.continue();
      });
      await context.routeWebSocket('**/*', async socket => { violation = true; await socket.close(); });
      const page = await context.newPage();
      page.setDefaultTimeout(5000); page.on('pageerror', () => { violation = true; }); page.on('download', () => { violation = true; });
      context.on('page', extra => { if (extra !== page) { violation = true; void extra.close(); } });
      await page.goto(`${server.origin}/${archetype}.html`, { waitUntil: 'load' });
      await work(page);
      category = 'network'; assert.equal(violation, false, 'network');
    } finally { await context.close(); }
  }
  async function qualifyAll() {
    for (const archetype of ['sales', 'office']) for (const width of [1280, 1440]) await withPage(archetype, width, async page => {
      category = 'keyboard'; await assertKeyboard(page, stage => { category = stage; });
      category = 'geometry'; await assertReducedMotion(page);
      for (const state of STATES) {
        category = 'source'; await selectScenario(page, state, scenarios);
        category = 'font'; await assertFonts(page, archetype);
        category = 'geometry'; await assertGeometry(page, server.origin, width);
      }
    });
  }
  async function captureAll() {
    for (const archetype of ['sales', 'office']) for (const state of STATES) await withPage(archetype, 1440, async page => {
        category = 'source'; await selectScenario(page, state, scenarios);
        category = 'font'; await assertFonts(page, archetype);
        category = 'geometry'; await assertGeometry(page, server.origin, 1440);
        category = 'pixels'; const name = `${archetype}-${state}.png`;
        const focus = await page.evaluateHandle(() => document.activeElement);
        let bytes;
        try { bytes = await page.screenshot(screenshotOptions()); assert.equal(await focus.evaluate(element => element === document.activeElement), true, 'pixels'); }
        finally { await focus.dispose(); }
        validatePng(bytes, name); await writeFile(join(capture, name), bytes, { flag: 'wx', mode: 0o600 });
    });
  }
  await qualifyAll();
  if (mode === 'capture') await captureAll();
  category = 'cleanup'; await browser.close(); browser = undefined; await server.close(); server = undefined;
  category = 'source';
  const finalSource = await snapshotSource(sourceRoot);
  assert.deepEqual(finalSource.identity, snapshot.identity, 'source'); assert.equal(git(['rev-parse', 'HEAD']), head, 'source'); assert.equal(git(['status', '--porcelain']), '', 'source');
  if (mode === 'capture') {
    category = 'gate'; assert.equal(reviewGate(readGitHubInput()).enabled, true, 'gate'); await requireLiveReview(gate);
    category = 'export'; await exportPixels({ capture, destination: join(privateRoot, 'visual-export'), qualified: true, sourceUnchanged: true });
    await rm(capture, { recursive: true });
    await appendFile(process.env.GITHUB_OUTPUT, `exported=true\nhead=${head}\n`);
  } else { await rm(privateRoot, { recursive: true }); privateRoot = undefined; }
  // Fixed schema, safe names and immutable source IDs only. No body/path/CDP payload.
  console.log(JSON.stringify({ subject: mode === 'capture' ? 'organization-d2-capture' : 'organization-d2-normal', head, tree, baseBranch: REVIEW.baseBranch, baseHead: REVIEW.baseSha, runId: input.runId, runAttempt: input.runAttempt,
    assertions: 'source-geometry-keyboard-focus-reduced-motion-origin-network-font-passed', geometry: '1280-and-1440-by-900',
    font: 'kosugi-regular-actual-japanese-heading-body-cdp', files: snapshot.identity, prerequisites,
    screenshots: mode === 'capture' ? 'twenty-1440-wide-fullpage-900-to-2400-high' : 'none', actualPixelReview: 'NOT RUN' }));
} catch {
  try { if (browser) await browser.close(); if (server) await server.close(); if (privateRoot) await rm(privateRoot, { recursive: true, force: true }); } catch { /* No raw cleanup errors in public logs. */ }
  console.error(safeFailure(category)); process.exitCode = 1;
}
