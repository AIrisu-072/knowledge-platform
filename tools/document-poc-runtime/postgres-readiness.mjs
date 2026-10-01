import { setTimeout as delay } from 'node:timers/promises';
import { Blocked } from './harness.mjs';

// Docker's initialization server is socket-only. Readiness and SQL deliberately
// target the final TCP listener. A successful fixed wrapper separates the
// pg_isready status from Docker/exec failures that can also use exit1 or2.
export function postgresReadyArgs(cid) {
  return ['exec', cid, 'sh', '-c',
    'pg_isready "$@"; status=$?; printf "KP_PG_READY_STATUS=%s\\n" "$status"', 'kp-pg-ready',
    '-q', '-h', '127.0.0.1', '-p', '5432', '-U', 'postgres', '-d', 'kp_document_poc', '-t', '2'];
}
export function parsePostgresReadyStatus(output) {
  const match = /^KP_PG_READY_STATUS=([0-3])$/.exec(output);
  if (!match) throw new Error('Unexpected PostgreSQL readiness probe output');
  return Number(match[1]);
}
export function postgresVersionArgs(cid) {
  return ['exec', '--env', 'PGPASSWORD', '--env', 'PGCONNECT_TIMEOUT=2', '--env', 'PGOPTIONS=-c statement_timeout=5000', cid,
    'psql', '-X', '-w', '-t', '-A', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-p', '5432', '-U', 'postgres', '-d', 'kp_document_poc',
    '-c', 'SHOW server_version'];
}

// Only explicit pg_isready1 (rejecting/startup) or2 (no response) retry.
// Docker/tool/timeouts throw untouched. SQL/auth/config is checked exactly once
// afterwards by the caller using the same TCP endpoint and fixture credentials.
export async function waitForPostgresTcp(probe, { timeoutMs = 30_000, now = Date.now, wait = delay } = {}) {
  const deadline = now() + timeoutMs;
  let lastError;
  while (now() < deadline) {
    const status = await probe(Math.min(5_000, deadline - now()));
    if (status === 0) return;
    if (![1, 2, 3].includes(status)) throw new Error('Unexpected PostgreSQL readiness probe status');
    const error = new Error('PostgreSQL readiness probe did not report an accepting listener');
    error.commandFailure = { category: 'command-exit', exitCode: status, available: true };
    if (status === 3) throw error;
    lastError = error;
    const remaining = deadline - now();
    if (remaining > 0) await wait(Math.min(500, remaining));
  }
  throw new Blocked('Final PostgreSQL TCP listener did not become ready within the observation deadline', { cause: lastError });
}
