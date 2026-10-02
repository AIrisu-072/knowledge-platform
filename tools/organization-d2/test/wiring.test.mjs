import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { PNG_NAMES } from '../pixels.mjs';
const read = file => readFileSync(new URL(`../../../${file}`, import.meta.url), 'utf8');
test('dedicated workflow has exact official uploader and only twenty literal image paths', () => {
  const workflow = read('.github/workflows/organization-d2.yml');
  assert.match(workflow, /actions\/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/);
  assert.match(workflow, /retention-days: 1/); assert.match(workflow, /overwrite: false/); assert.match(workflow, /include-hidden-files: false/);
  const names = [...workflow.matchAll(/visual-export\/([a-z_-]+\.png)/g)].map(match => match[1]); assert.deepEqual(names, PNG_NAMES);
  assert.doesNotMatch(workflow, /pull_request_target|workflow_dispatch|actions: write|id-token: write|contents: write|secrets\.|path:.*\*/);
  assert.match(workflow, /success\(\) && steps\.qualification\.outputs\.exported == 'true'/);
});
test('runner completes all normal assertions before screenshot loop, proves source and live gates before export', () => {
  const runner = read('tools/organization-d2/run.mjs');
  assert.ok(runner.indexOf('await qualifyAll(') < runner.indexOf('await captureAll('));
  assert.ok(runner.indexOf('await requireLiveReview(gate);', runner.indexOf('await captureAll(')) < runner.indexOf('await exportPixels('));
  assert.match(runner, /serviceWorkers: 'block'/); assert.match(runner, /routeWebSocket/);
  assert.doesNotMatch(runner, /recordVideo|tracing\.start|console\.log\(error|console\.error\(error/);
  assert.match(runner, /assert\.deepEqual\(finalSource\.identity, snapshot\.identity/);
});
