const { readFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const { dirname, join } = require('node:path');
const vm = require('node:vm');

// Exercise the pinned getByRole exact-name engine against the rendered Jest DOM.
// Only installed injected helpers are evaluated; no browser or server starts.
module.exports = function playwrightExactRoleMatcher() {
  const testRequire = createRequire(require.resolve('@playwright/test/package.json'));
  const playwrightRequire = createRequire(testRequire.resolve('playwright/package.json'));
  const core = playwrightRequire.resolve('playwright-core/package.json');
  const manifest = JSON.parse(readFileSync(core, 'utf8'));
  if (manifest.version !== '1.63.0') throw new Error('Review role probe for changed Playwright pin');
  const bundle = readFileSync(join(dirname(core), 'lib/coreBundle.js'), 'utf8');
  const line = bundle.split('\n').find((entry) => entry.trim().startsWith('source4 = ') && entry.includes('injectedScript.ts'));
  if (!line) throw new Error('Pinned Playwright injected role implementation unavailable');
  const source = vm.runInNewContext(line.trim().replace(/^source4 = /, '').replace(/;$/, ''));
  const helpers = new Function('module', `${source}\nreturn { createRoleEngine };`)({ exports: {} });
  return (root, role, name) => helpers.createRoleEngine(true).queryAll(root, `${role}[name=${JSON.stringify(name)}s]`);
};
