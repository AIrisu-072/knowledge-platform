import test from 'node:test';
import assert from 'node:assert/strict';
import { summarize } from '../ci-summary.mjs';

test('CI summary allowlists hashes, profiles, versions and stage categories without raw reasons or paths', () => {
  const secret = 'postgres://u:secret@127.0.0.1/private';
  const report = { gitHead: 'a'.repeat(40), gitDirty: true, status: 'blocked', acceptanceQualified: false,
    platform: { os: 'linux', arch: 'x64', node: 'v24.21.0', env: secret }, tools: { rustc: 'rustc 1.98.1 (safe)', pnpm: '12.4.1' },
    database: { ownership: 'caller-asserted-disposable', version: secret },
    artifacts: { server: 'b'.repeat(64), dsi: 'c'.repeat(64), diff: 'd'.repeat(64), pdfium: 'e'.repeat(64), web: { '/private/index.html': 'f'.repeat(64) } },
    processes: [{ profile: 'poc-human', origin: secret }, { profile: secret }],
    stages: [{ name: 'human-start', status: 'blocked', reason: secret }, { name: secret, status: secret }], failure: secret, argv: [secret] };
  const output = JSON.stringify(summarize(report));
  assert.ok(!output.includes('secret')); assert.ok(!output.includes('/private')); assert.ok(!output.includes('postgres://'));
  const result = summarize(report);
  assert.equal(result.gitHead, 'a'.repeat(40)); assert.deepEqual(result.profiles, ['poc-human']);
  assert.equal(result.tools.rustc, '1.98.1'); assert.equal(result.postgresVersion, 'unverified-external');
  assert.deepEqual(result.stages, [{ name: 'human-start', status: 'blocked', failureCode: 'worker-preflight-unavailable' }]);
});

test('missing or malformed provenance is bounded unavailable evidence, never pass', () => {
  assert.equal(summarize(null).status, 'not-available');
  const result = summarize({ status: 'passed', acceptanceQualified: true, gitHead: 'https://secret', stages: [{ name: 'browser-journey', status: 'not-run' }] });
  assert.equal(result.acceptanceQualified, false); assert.equal(result.gitHead, 'unverified');
  assert.ok(JSON.stringify(summarize({ stages: Array(10000).fill({ name: 'browser-journey', status: 'failed' }) })).length < 8000);
});

test('browser diagnostics are sanitized again before the CI summary is printed', () => {
  const secret = 'postgres://private:credential@host/database';
  const summary = summarize({ status: 'failed', browserDiagnostics: {
    journey: { availability: 'available', counts: { passed: 0, failed: 1, skipped: 4 }, truncated: false,
      tests: [{ source: 'document-runtime.spec.ts', line: 44, column: 5, status: 'failed', errorCategory: 'strict-locator',
        matcher: 'toBeVisible', message: secret, title: secret, selector: secret, url: secret, stack: secret }], raw: secret },
    persistence: { availability: 'unavailable', tests: [{ title: secret }] }, [secret]: secret,
  } });
  assert.equal(summary.browserDiagnostics.journey.tests[0].line, 44);
  assert.equal(summary.browserDiagnostics.journey.tests[0].errorCategory, 'strict-locator');
  assert.ok(!JSON.stringify(summary).includes('credential'));
  assert.deepEqual(Object.keys(summary.browserDiagnostics), ['journey', 'persistence']);
});
