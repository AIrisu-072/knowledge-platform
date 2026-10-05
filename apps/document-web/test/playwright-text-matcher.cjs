const { readFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const { dirname, join } = require('node:path');
const vm = require('node:vm');

// Exercise the pinned getByText exact matcher against Jest's DOM. Unlike
// Testing Library's direct text, it includes a history entry's nested time.
// Only existing injected helpers are evaluated; no browser or server starts.
module.exports = function playwrightExactTextMatcher() {
  const testRequire = createRequire(require.resolve('@playwright/test/package.json'));
  const playwrightRequire = createRequire(testRequire.resolve('playwright/package.json'));
  const core = playwrightRequire.resolve('playwright-core/package.json');
  const manifest = JSON.parse(readFileSync(core, 'utf8'));
  if (manifest.version !== '1.63.0') throw new Error('Review text probe for changed Playwright pin');
  const bundle = readFileSync(join(dirname(core), 'lib/coreBundle.js'), 'utf8');
  const line = bundle.split('\n').find((entry) => entry.trim().startsWith('source4 = ') && entry.includes('injectedScript.ts'));
  if (!line) throw new Error('Pinned Playwright injected text implementation unavailable');
  const source = vm.runInNewContext(line.trim().replace(/^source4 = /, '').replace(/;$/, ''));
  const helpers = new Function('module', `${source}\nreturn { elementMatchesText, createTextMatcher };`)({ exports: {} });
  return (element, text) => helpers.elementMatchesText(new Map(), element, helpers.createTextMatcher(`${JSON.stringify(text)}s`, true).matcher) === 'self';
};
