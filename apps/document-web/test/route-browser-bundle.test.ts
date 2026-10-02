import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

test('production route bundle runs without Node globals or runtime code generation', () => {
  execFileSync(process.execPath, [resolve(__dirname, 'route-browser-bundle.cjs')], {
    cwd: resolve(__dirname, '..'), timeout: 60_000, stdio: 'pipe',
  });
}, 65_000);
