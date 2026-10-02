import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { summarize } from '../ci-summary.mjs';
import { browserDiagnostics } from '../browser-diagnostics.mjs';

test('owned real browser journey includes ordered consistency and its actual-stdio bundle provenance', async () => {
  const config = await readFile(new URL('../../../apps/document-web/playwright.runtime.config.ts', import.meta.url), 'utf8');
  assert.match(config, /human-agent-consistency\.spec\.ts/);
  const runner = await readFile(new URL('../run.mjs', import.meta.url), 'utf8');
  assert.match(runner, /mcpConsistency: await sha256File\(join\(root, 'apps\/document-mcp\/dist\/consistency.cjs'\)\)/);
  assert.equal(summarize({ artifacts: { mcpConsistency: 'a'.repeat(64) } }).artifacts.mcpConsistency, 'a'.repeat(64));
});
test('new acceptance source locations are bounded without revealing raw synthetic evidence', () => {
  const output = browserDiagnostics({ errors: [{ message: 'assertion failed private://payload', location: { file: '/private/human-agent-consistency.spec.ts', line: 42 } }] });
  assert.equal(output.tests[0].source, 'human-agent-consistency.spec.ts');
  assert.equal(output.tests[0].line, 42);
  assert.ok(!JSON.stringify(output).includes('private'));
});
test('owned journey includes real worker failure operations bound to runtime-copy artifact hashes', async () => {
  const config = await readFile(new URL('../../../apps/document-web/playwright.runtime.config.ts', import.meta.url), 'utf8');
  assert.match(config, /worker-failure\.spec\.ts/);
  const runner = await readFile(new URL('../run.mjs', import.meta.url), 'utf8');
  assert.match(runner, /workerHashes: \{ dsi: report\.data\.artifacts\.dsi, diff: report\.data\.artifacts\.diff \}/);
  assert.match(runner, /withUnavailableWorker\(directory, worker, report\.data\.artifacts\[worker\]/);
  const output = browserDiagnostics({ errors: [{ message: 'assertion failed private://input', location: { file: '/private/worker-failure.spec.ts', line: 23 } }] });
  assert.equal(output.tests[0].source, 'worker-failure.spec.ts'); assert.ok(!JSON.stringify(output).includes('private'));
});
