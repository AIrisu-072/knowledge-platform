import test from 'node:test';
import assert from 'node:assert/strict';
import { assertPlatformFont, requestAllowed, safeFailure, screenshotOptions } from '../browser-checks.mjs';
const origin = 'http://127.0.0.1:4567';
test('page network allows exact source origin assets only and refuses credentials and non-GET', () => { assert.equal(requestAllowed(origin, `${origin}/sales.html?scenario=normal`, 'GET'), true); for (const [url, method] of [['https://example.invalid', 'GET'], [`${origin}/.git/config`, 'GET'], [`${origin}/sales.html`, 'POST'], ['http://a:b@127.0.0.1:4567/sales.html', 'GET'], [`${origin}/source-design.test.mjs`, 'GET'], ['data:text/plain,secret', 'GET']]) assert.equal(requestAllowed(origin, url, method), false); });
test('actual Japanese glyph selection requires single exact installed Kosugi regular face', () => { const fonts = [{ familyName: 'Kosugi', postScriptName: 'Kosugi-Regular', isCustomFont: false, glyphCount: 2 }]; assert.doesNotThrow(() => assertPlatformFont(fonts, '現在')); for (const mutation of [{ familyName: 'Other' }, { postScriptName: 'Other' }, { isCustomFont: true }, { glyphCount: 1 }]) assert.throws(() => assertPlatformFont([{ ...fonts[0], ...mutation }], '現在')); assert.throws(() => assertPlatformFont([...fonts, ...fonts], '現在')); assert.throws(() => assertPlatformFont(fonts, 'abc')); });
test('only fullpage1440 CSS-pixel captures; failure sanitizer never emits arbitrary errors', () => { assert.deepEqual(screenshotOptions(), { type: 'png', fullPage: true, scale: 'css' }); assert.equal(safeFailure('browser'), 'Organization D2 qualification failed: browser'); assert.equal(safeFailure('secret/path/token'), 'Organization D2 qualification failed: internal'); });
test('hosted-only entrypoint fails sanitized before any local browser import or launch', async () => {
  const { spawnSync } = await import('node:child_process');
  const result = spawnSync(process.execPath, ['tools/organization-d2/run.mjs', 'normal'], { encoding: 'utf8', env: { PATH: process.env.PATH } });
  assert.equal(result.status, 1); assert.equal(result.stdout, ''); assert.equal(result.stderr, 'Organization D2 qualification failed: environment\n');
});
test('normal qualification must run terminal transition and disabled visual assertions before capture', async () => {
  const checks = await import('../browser-checks.mjs');
  assert.equal(typeof checks.assertHandoffPresentation, 'function');
  assert.equal(typeof checks.assertDisabledPrimary, 'function');
  assert.equal(typeof checks.assertStateTransitions, 'function');
  const { readFileSync } = await import('node:fs');
  const runner = readFileSync(new URL('../run.mjs', import.meta.url), 'utf8');
  assert.match(runner, /await assertStateTransitions\(page\)/);
  assert.ok(runner.indexOf('await assertStateTransitions(page)') < runner.indexOf('for (const state of STATES)'));
  const source = readFileSync(new URL('../browser-checks.mjs', import.meta.url), 'utf8');
  assert.match(source, /state === 'handed_off'.*assertHandoffPresentation\(page\)/);
  assert.match(source, /state === 'blocked'/);
});
test('handoff and disabled style assertions fail closed with fixed categories', async () => {
  const checks = await import('../browser-checks.mjs');
  assert.equal(typeof checks.assertHandoffPresentation, 'function');
  assert.equal(typeof checks.assertDisabledPrimary, 'function');
  await assert.rejects(() => checks.assertHandoffPresentation({ evaluate: async () => false }), /^AssertionError.*source|^source/);
  for (const failAt of [0, 1, 2]) {
    let calls = 0;
    const button = { evaluate: async () => calls++ !== failAt, hover: async () => {}, focus: async () => {} };
    await assert.rejects(() => checks.assertDisabledPrimary({ locator: () => button }, '#submit-action'), /^AssertionError.*source|^source/);
  }
});
