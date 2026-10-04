import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { DatabaseDiagnostics, sanitizeDatabaseDiagnostics } from '../database-diagnostics.mjs';
import { command, EvidenceReport } from '../harness.mjs';

const create = () => new DatabaseDiagnostics({ data: {}, save: async () => {} }, true);

test('database operations record exact bounded operation/category and rethrow original errors', async () => {
  const diagnostics = create();
  await diagnostics.step('external-validation', async () => undefined);
  const cause = Object.assign(Error('postgres://user:secret@host/private'), { code: 'ENOENT' });
  await assert.rejects(diagnostics.step('cid-read', async () => { throw cause; }), error => error === cause);
  assert.deepEqual(diagnostics.snapshot(), { externalDatabaseSupplied: true, steps: [
    { operation: 'external-validation', status: 'passed', category: 'ok', available: true },
    { operation: 'cid-read', status: 'failed', category: 'filesystem-unavailable', available: false },
  ] });
  assert.ok(!JSON.stringify(diagnostics.snapshot()).includes('secret'));
});

test('real nonzero command failures retain safe exit metadata and cause without raw output', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'database-diagnostic-test-'));
  try {
    const diagnostics = create();
    let error;
    try { await diagnostics.step('sql-version-query', () => command(process.execPath, ['-e', 'process.exit(23)'], { cwd: directory, env: process.env, log: join(directory, 'command.log') })); }
    catch (caught) { error = caught; }
    assert.equal(error.commandFailure.exitCode, 23);
    assert.equal(error.cause.exitCode, 23);
    assert.deepEqual(diagnostics.snapshot().steps[0], { operation: 'sql-version-query', status: 'failed', category: 'command-exit', available: false, commandAvailable: true, exitCode: 23 });
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('database validation, parse, readiness and proxy failure categories remain distinct', async () => {
  const diagnostics = create();
  for (const [operation, expected] of [['cid-validation', 'invalid-container-id'], ['port-validation', 'invalid-port-binding'],
    ['repo-digest-parse', 'invalid-json'], ['readiness', 'readiness-exhausted'], ['external-validation', 'external-database-rejected'], ['proxy-start', 'proxy-unavailable']]) {
    await assert.rejects(diagnostics.step(operation, async () => { throw Error('private'); }));
    assert.equal(diagnostics.snapshot().steps.at(-1).category, expected);
  }
});

test('summary sanitizer rejects raw fields, malformed operation/category/exit codes and bounds records', () => {
  const secret = 'https://user:credential@secret.example/private';
  const result = sanitizeDatabaseDiagnostics({ externalDatabaseSupplied: secret, url: secret, steps: [
    { operation: 'docker-run', status: 'failed', category: 'command-exit', available: false, commandAvailable: false, exitCode: 125, message: secret, sql: secret, path: secret },
    { operation: secret, status: secret, category: secret, exitCode: secret },
    { operation: 'repo-digest-parse', status: 'failed', category: secret, available: secret, exitCode: 10000 },
    ...Array(1000).fill({ operation: 'readiness', status: 'failed', category: 'readiness-exhausted', available: false }),
  ] });
  assert.ok(!JSON.stringify(result).includes('credential')); assert.ok(!JSON.stringify(result).includes('10000'));
  assert.equal(result.steps[0].exitCode, 125); assert.ok(result.steps.length <= 20);
  assert.equal(result.externalDatabaseSupplied, undefined);
});
