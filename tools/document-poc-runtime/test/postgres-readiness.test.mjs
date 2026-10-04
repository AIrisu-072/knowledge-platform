import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { command } from '../harness.mjs';
import { postgresReadyArgs, postgresVersionArgs, parsePostgresReadyStatus, waitForPostgresTcp } from '../postgres-readiness.mjs';

const failure = code => Object.assign(Error('original probe failure'), { commandFailure: { category: 'command-exit', exitCode: code, available: true } });

test('readiness and real SQL both require the final TCP endpoint with bounded clients and no password argv', () => {
  const ready = postgresReadyArgs('owned-container'), sql = postgresVersionArgs('owned-container');
  for (const args of [ready, sql]) {
    assert.equal(args[args.indexOf('-h') + 1], '127.0.0.1');
    assert.equal(args[args.indexOf('-p') + 1], '5432');
    assert.equal(args[args.indexOf('-d') + 1], 'kp_document_poc');
    assert.ok(!args.some(value => value.startsWith('PGPASSWORD=')));
  }
  assert.equal(ready[ready.indexOf('-t') + 1], '2');
  assert.equal(ready[2], 'sh'); assert.equal(ready[3], '-c');
  assert.match(ready[4], /pg_isready \"\$@\"/);
  assert.match(ready[4], /KP_PG_READY_STATUS=/);
  assert.ok(sql.includes('PGPASSWORD')); assert.ok(sql.includes('PGCONNECT_TIMEOUT=2'));
  assert.ok(sql.includes('PGOPTIONS=-c statement_timeout=5000')); assert.ok(sql.includes('-w'));
  assert.ok(sql.includes('ON_ERROR_STOP=1')); assert.equal(sql.at(-1), 'SHOW server_version');
});

test('retries only pg_isready startup/no-response statuses and success requires a real success', async () => {
  let time = 0, attempts = 0;
  await waitForPostgresTcp(async () => { attempts++; return attempts < 3 ? attempts : 0; }, {
    timeoutMs: 3000, now: () => time, wait: async ms => { time += ms; },
  });
  assert.equal(attempts, 3);
  for (const error of [failure(1), failure(2), failure(3), failure(125), Object.assign(Error('missing'), { commandFailure: { category: 'command-unavailable', available: false } }), Error('unexpected')]) {
    let calls = 0;
    await assert.rejects(waitForPostgresTcp(async () => { calls++; throw error; }), caught => caught === error);
    assert.equal(calls, 1);
  }
});

test('deadline bounds probe attempts and preserves the final startup cause instead of sleeping to success', async () => {
  let time = 0, calls = 0;
  await assert.rejects(waitForPostgresTcp(async budget => { assert.ok(budget <= 1200); calls++; return 2; }, {
    timeoutMs: 1200, now: () => time, wait: async ms => { time += ms; },
  }), caught => caught.cause.commandFailure.exitCode === 2);
  assert.equal(time, 1200); assert.equal(calls, 3);
});

test('host-side command deadline stops only its owned hung client and records a non-retryable timeout', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'postgres-client-timeout-'));
  try {
    await assert.rejects(command(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
      cwd: directory, env: process.env, log: join(directory, 'client.log'), timeoutMs: 50,
    }), error => error.commandFailure?.category === 'command-timeout');
  } finally { await rm(directory, { recursive: true, force: true }); }
});


test('only the fixed pg_isready marker identifies startup status; wrapper failures remain failures', async () => {
  assert.equal(parsePostgresReadyStatus('KP_PG_READY_STATUS=0'), 0);
  assert.equal(parsePostgresReadyStatus('KP_PG_READY_STATUS=2'), 2);
  for (const output of ['docker error', 'KP_PG_READY_STATUS=125', 'KP_PG_READY_STATUS=2\nother output']) {
    assert.throws(() => parsePostgresReadyStatus(output));
  }
  let attempts = 0;
  await assert.rejects(waitForPostgresTcp(async () => { attempts++; return 3; }), error => error.commandFailure.exitCode === 3);
  assert.equal(attempts, 1);
});
