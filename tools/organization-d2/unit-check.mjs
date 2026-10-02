// Keep assertion diffs, stack paths and source excerpts out of public Actions logs.
import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
try {
  const files = readdirSync('tools/organization-d2/test').filter(name => name.endsWith('.test.mjs')).sort().map(name => `tools/organization-d2/test/${name}`);
  execFileSync(process.execPath, ['--test', ...files], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 60_000, maxBuffer: 1024 * 1024 });
  console.log('Organization D2 harness unit assertions passed; no browser proof inferred');
} catch { console.error('Organization D2 qualification failed: harness-unit'); process.exitCode = 1; }
