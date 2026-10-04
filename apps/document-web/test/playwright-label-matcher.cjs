const { readFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const { dirname, join } = require('node:path');
const vm = require('node:vm');

// Use the exact installed Playwright label algorithm, which intentionally differs
// from Testing Library for a <label> wrapping a <select> with option text.
// This evaluates the existing package's injected source in Jest's DOM only; no
// browser, listener, network request or application runtime is started.
module.exports = function playwrightExactLabelMatcher() {
  const testRequire = createRequire(require.resolve('@playwright/test/package.json'));
  const playwrightRequire = createRequire(testRequire.resolve('playwright/package.json'));
  const core = playwrightRequire.resolve('playwright-core/package.json');
  const manifest = JSON.parse(readFileSync(core, 'utf8'));
  if (manifest.version !== '1.63.0') throw new Error('Review label probe for changed Playwright pin');
  const bundle = readFileSync(join(dirname(core), 'lib/coreBundle.js'), 'utf8');
  const line = bundle.split('\n').find((entry) => entry.trim().startsWith('source4 = ') && entry.includes('injectedScript.ts'));
  if (!line) throw new Error('Pinned Playwright injected label implementation unavailable');
  const source = vm.runInNewContext(line.trim().replace(/^source4 = /, '').replace(/;$/, ''));
  const helpers = new Function('module', `${source}\nreturn { getElementLabels, createTextMatcher };`)({ exports: {} });
  return (element, name) => helpers.getElementLabels(new Map(), element).some(helpers.createTextMatcher(`${JSON.stringify(name)}s`, true).matcher);
};
