import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const fixture = await readFile(new URL('../../../crates/document-publication-scheduler/tests/support/database.rs', import.meta.url), 'utf8');

test('scheduler container waits once for final PostgreSQL stderr readiness', () => {
  // The official image uses pg_ctl for its socket-only initialization server;
  // pg_ctl redirects that server's stderr to stdout. Only the final, directly
  // executed postgres emits this marker on stderr. Two stderr markers can hang.
  // This is a source/configuration guard, not real-container acceptance evidence.
  assert.match(fixture, /with_wait_for\(WaitFor::message_on_stderr\(\s*"database system is ready to accept connections",?\s*\)\)/);
  assert.doesNotMatch(fixture, /\.with_times\(2\)/);
  assert.doesNotMatch(fixture, /WaitFor::message_on_(?:stdout|either_std)/);
});
