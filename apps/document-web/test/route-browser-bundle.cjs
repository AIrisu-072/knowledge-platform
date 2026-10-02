const assert = require('node:assert/strict');
const { mkdtempSync, readFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { resolve, join } = require('node:path');
const vm = require('node:vm');
const webpack = require('webpack');
const configure = require('../webpack.config.cjs');

// Exercise the real production loader/parser configuration, rather than Jest's
// CommonJS transform, which can hide unresolved browser require() calls.
const directory = mkdtempSync(join(tmpdir(), 'document-route-browser-'));
const config = configure({}, { mode: 'production' });
config.entry = {
  RouteState: resolve(__dirname, '../src/application/search-state.ts'),
  ApiClient: resolve(__dirname, '../../../packages/document-api-client/src/index.ts'),
};
config.output = { ...config.output, path: directory, filename: '[name].js', library: { name: '[name]', type: 'var' } };
const compiler = webpack(config);
compiler.run((error, stats) => {
  compiler.close(closeError => {
    try {
      if (error || closeError) throw error || closeError;
      assert.ok(stats && !stats.hasErrors(), stats?.toString({ all: false, errors: true }));
      // Headers is a browser Web API used by generated-client initialization.
      // Supply no Node globals or network function.
      const context = vm.createContext({ Headers }, { codeGeneration: { strings: false, wasm: false } });
      assert.equal(vm.runInContext('typeof require', context), 'undefined');
      assert.equal(vm.runInContext('typeof process', context), 'undefined');
      assert.throws(() => vm.runInContext('Function("return 1")()', context), /code generation|Code generation/);
      vm.runInContext(readFileSync(join(directory, 'RouteState.js'), 'utf8'), context, { timeout: 5000 });
      const result = vm.runInContext('RouteState.validateListSearch({ titleContains: "😀", pageSize: "25", extra: "remove" })', context);
      assert.deepEqual(JSON.parse(JSON.stringify(result)), {
        titleContains: '😀', pageSize: 25, view: 'published', includeDescendants: false,
        sort: 'published_at_desc', panel: 'open',
      });
      assert.equal(vm.runInContext('RouteState.validateDetailSearch({ tab: "versions" }).tab', context), 'versions');
      vm.runInContext(readFileSync(join(directory, 'ApiClient.js'), 'utf8'), context, { timeout: 5000 });
      assert.equal(vm.runInContext('typeof ApiClient.getSession', context), 'function');
      assert.equal(vm.runInContext('typeof ApiClient.BinaryTransportBridge', context), 'function');
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
