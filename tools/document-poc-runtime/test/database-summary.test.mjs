import test from 'node:test';
import assert from 'node:assert/strict';
import { summarize } from '../ci-summary.mjs';

test('CI database evidence is re-sanitized and preserves known prior provenance fields', () => {
  const secret = 'postgres://user:credential@host/private';
  const result = summarize({ gitHead: 'a'.repeat(40), gitDirty: false, status: 'failed', databaseDiagnostics: {
    externalDatabaseSupplied: false, cause: secret, steps: [{ operation: 'sql-version-query', status: 'failed', category: 'command-exit',
      available: false, commandAvailable: true, exitCode: 2, message: secret, command: secret, env: secret, sql: secret, path: secret }],
  } });
  assert.equal(result.gitHead, 'a'.repeat(40));
  assert.deepEqual(result.databaseDiagnostics.steps, [{ operation: 'sql-version-query', status: 'failed', category: 'command-exit', available: false, commandAvailable: true, exitCode: 2 }]);
  assert.ok(!JSON.stringify(result).includes('credential'));
});
