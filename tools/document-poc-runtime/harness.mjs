import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { connect, createServer } from 'node:net';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { StringDecoder } from 'node:string_decoder';
import { request as httpRequest } from 'node:http';

export const RUNTIME_STAGES = ['toolchain', 'build', 'artifacts', 'database', 'migrate', 'bootstrap', 'human-start', 'agent-start', 'trace-request', 'seed', 'seed-replay', 'browser-journey', 'agent-acceptance', 'health-recovery', 'ordinary-request-sigterm', 'stalled-download-sigterm', 'shutdown', 'agent-outage', 'diagnostics', 'restart', 'browser-persistence', 'final-shutdown'];

export class Blocked extends Error {}

export function binaryDirectory({ root, env, prebuilt }) {
  if (env.KP_POC_BINARY_DIR && !prebuilt) {
    throw new Blocked('KP_POC_BINARY_DIR requires explicit --prebuilt diagnostics; unrelated executables cannot qualify a source build');
  }
  return resolve(env.KP_POC_BINARY_DIR ?? join(env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug'));
}


export function externalDatabase(env) {
  if (!env.TEST_DATABASE_URL) return undefined;
  if (env.KP_POC_DISPOSABLE_DATABASE !== 'true') throw new Blocked('Explicit KP_POC_DISPOSABLE_DATABASE=true is required for an external disposable database');
  let url;
  try { url = new URL(env.TEST_DATABASE_URL); } catch { throw new Blocked('External database must be a loopback PostgreSQL URL'); }
  if (!['postgres:', 'postgresql:'].includes(url.protocol) || !['127.0.0.1', '[::1]', 'localhost'].includes(url.hostname) || url.search || url.hash || url.pathname === '/') {
    throw new Blocked('External database must be a loopback PostgreSQL URL without query overrides');
  }
  return env.TEST_DATABASE_URL;
}

export function postgresArguments(runId, cidfile) {
  return ['run', '--detach', '--rm', '--name', `document-poc-${runId}`, '--label', `kp.document-poc.run=${runId}`,
    '--cidfile', cidfile, '--publish', '127.0.0.1::5432', '--tmpfs', '/var/lib/postgresql:rw',
    '--env', 'POSTGRES_PASSWORD', '--env', 'POSTGRES_DB=kp_document_poc', 'postgres:18.6-bookworm'];
}

export function serverEnvironment({ inherited, database, profile, port, storage, dsi, diff, web, pdfium }) {
  const env = Object.fromEntries(Object.entries(inherited).filter(([key]) => !key.startsWith('KP_') && key !== 'TEST_DATABASE_URL'));
  return { ...env, KP_RUNTIME_MODE: 'poc', KP_IDENTITY_PROFILE: profile, KP_BIND: `127.0.0.1:${port}`,
    KP_POC_ALLOW_NON_LOOPBACK: 'false', KP_DATABASE_URL: database, KP_STORAGE_ROOT: storage,
    KP_DSI_WORKER: dsi, KP_DIFF_WORKER: diff, KP_DSI_PDFIUM_RUNTIME_DIR: pdfium,
    ...(profile === 'poc-human' ? { KP_WEB_DIST: web } : {}) };
}

export function startupBlocked(log) {
  return /(?:DSI|Diff) worker or required native sandbox is unavailable/.test(log);
}

export class EvidenceReport {
  constructor(directory, stages) {
    this.path = join(directory, 'report.json');
    this.data = { schemaVersion: 1, status: 'running', acceptanceQualified: false, startedAt: new Date().toISOString(),
      scope: 'C1 R6 composition-root acceptance; scheduler R5 excluded and remains STOP',
      stages: stages.map(name => ({ name, status: 'not-run' })) };
  }
  async save() { await writeFile(this.path, JSON.stringify(this.data, null, 2) + '\n', { mode: 0o600 }); }
  async stage(name, action) {
    const stage = this.data.stages.find(item => item.name === name);
    if (!stage) throw Error(`Unknown evidence stage: ${name}`);
    stage.status = 'running'; stage.startedAt = new Date().toISOString(); await this.save();
    try {
      const result = await action(); stage.status = 'passed'; return result;
    } catch (error) {
      stage.status = error instanceof Blocked ? 'blocked' : 'failed';
      stage.reason = error.message; this.data.status = stage.status; throw error;
    } finally { stage.finishedAt = new Date().toISOString(); await this.save(); }
  }
  async finish() {
    this.data.acceptanceQualified = this.data.stages.every(s => s.status === 'passed');
    if (this.data.acceptanceQualified) this.data.status = 'passed';
    else if (this.data.status === 'running') this.data.status = 'not-run';
    this.data.finishedAt = new Date().toISOString(); await this.save();
  }
}

export async function freePort() {
  const server = createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve)); return port;
}

// Startup observation exceeds DB acquire5s + DSI preflight10s + Diff preflight30s.
// This is a test observation budget, not a production SLO.
export const STARTUP_OBSERVATION_MS = 60_000;
export async function waitReady(origin, child, timeout = STARTUP_OBSERVATION_MS) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode || child.spawnFailure) throw Error('Server exited before readiness');
    try {
      const response = await fetch(`${origin}/health/ready`, { signal: AbortSignal.timeout(1_000) });
      if (response.status === 200 && JSON.stringify(await response.json()) === '{"status":"ok"}') return;
    } catch { /* poll the owned process until its startup window ends */ }
    await delay(100);
  }
  throw Error('Server did not become ready within the test startup window');
}

export function startProcess(command, args, { cwd, env, log, secrets = [] }) {
  const stream = createWriteStream(log, { flags: 'a', mode: 0o600 });
  const child = spawn(command, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let rawOutput = '';
  const stdoutDecoder = new StringDecoder('utf8'), stderrDecoder = new StringDecoder('utf8');
  const redact = () => {
    let output = rawOutput;
    for (const value of secrets.filter(Boolean)) output = output.replaceAll(value, '[REDACTED]');
    return output;
  };
  // Buffer until child exit so secrets spanning arbitrary pipe chunks cannot leak.
  // output() is also redacted; raw process output is never persisted or reported.
  child.stdout.on('data', data => { rawOutput += stdoutDecoder.write(data); });
  child.stderr.on('data', data => { rawOutput += stderrDecoder.write(data); });
  child.on('error', error => { child.spawnFailure = true; child.spawnError = error; });
  const done = new Promise(resolve => child.once('close', (code, signal) => {
    rawOutput += stdoutDecoder.end() + stderrDecoder.end();
    const output = redact();
    stream.end(output, () => resolve({ code, signal, output }));
  }));
  return { child, done, log, output: redact };
}

export async function command(command, args, options) {
  const process = startProcess(command, args, options);
  let timedOut = false;
  const timer = options.timeoutMs === undefined ? undefined : setTimeout(() => {
    timedOut = true;
    // Only this owned read-only probe client is stopped; never the database/server.
    process.child.kill('SIGKILL');
  }, options.timeoutMs);
  const result = await process.done.finally(() => clearTimeout(timer));
  if (timedOut) {
    const error = new Error('Owned probe client exceeded its observation deadline', { cause: { signal: result.signal } });
    error.commandFailure = { category: 'command-timeout', available: true }; throw error;
  }
  if (process.child.spawnFailure) {
    const error = new Blocked(`Required command unavailable: ${command.split('/').at(-1)}`, { cause: process.child.spawnError });
    error.commandFailure = { category: 'command-unavailable', available: false }; throw error;
  }
  if (result.code !== 0) {
    const error = new Error(`${command.split('/').at(-1)} exited ${result.code ?? result.signal}; see stage log`,
      { cause: { exitCode: result.code, signal: result.signal } });
    error.commandFailure = { category: result.code === null ? 'command-signal' : 'command-exit', available: true, exitCode: result.code };
    throw error;
  }
  return result.output.trim();
}

export async function stopProcess(process, timeout = 15_000) {
  if (process.child.exitCode !== null || process.child.signalCode) throw Error('Owned server exited before the requested graceful shutdown');
  process.child.kill('SIGTERM');
  await waitForDrain(process, timeout);
}

export async function waitForDrain(process, timeout = 15_000) {
  let timer;
  try {
    const result = await Promise.race([process.done, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('Graceful drain exceeded the test observation window; no bounded application shutdown claim')), timeout); })]);
    if (result.code !== 0 || !result.output.includes('graceful drain complete')) throw Error('Server did not confirm a successful graceful drain');
  } finally { clearTimeout(timer); }
}

export async function sha256File(path) { return createHash('sha256').update(await readFile(path)).digest('hex'); }

// Transparent TCP fault injection, not a database fake. Every query still reaches the
// one disposable PostgreSQL upstream; pause drops sessions without editing its data.
export async function databaseProxy(databaseUrl) {
  const upstream = new URL(databaseUrl);
  const sockets = new Set();
  let paused = false;
  const server = createServer(client => {
    if (paused) { client.destroy(); return; }
    const target = connect(Number(upstream.port || 5432), upstream.hostname.replace(/^\[|\]$/g, ''));
    sockets.add(client); sockets.add(target);
    client.on('error', () => target.destroy()); target.on('error', () => client.destroy());
    client.on('close', () => { sockets.delete(client); target.destroy(); });
    target.on('close', () => { sockets.delete(target); client.destroy(); });
    client.pipe(target); target.pipe(client);
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const url = new URL(databaseUrl); url.hostname = '127.0.0.1'; url.port = String(server.address().port);
  return {
    url: url.href,
    pause() { paused = true; for (const socket of sockets) socket.destroy(); },
    resume() { paused = false; },
    async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); },
  };
}


export async function waitForListenerRefusal(origin, timeout = 5_000) {
  const url = new URL(origin), deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const refused = await new Promise(resolve => {
      const socket = connect(Number(url.port), url.hostname);
      socket.setTimeout(500, () => { socket.destroy(); resolve(false); });
      socket.once('connect', () => { socket.destroy(); resolve(false); });
      socket.once('error', error => { socket.destroy(); resolve(error.code === 'ECONNREFUSED'); });
    });
    if (refused) return;
    await delay(50);
  }
  throw Error('New connections were not refused after SIGTERM within the observation window');
}

export async function delayedJsonRequest(origin, path, body) {
  const bytes = Buffer.from(JSON.stringify(body)), split = Math.max(1, Math.floor(bytes.length / 2));
  let started = false;
  let responseResolve, responseReject;
  const responseDone = new Promise((resolve, reject) => { responseResolve = resolve; responseReject = reject; });
  responseDone.catch(() => {});
  const request = httpRequest(new URL(path, origin), { method: 'PATCH', agent: false, headers: {
    'Content-Type': 'application/json', 'Content-Length': bytes.length, Expect: '100-continue',
  } });
  request.on('response', response => {
    started = true; let result = '';
    response.on('data', chunk => { result += chunk.toString(); });
    response.on('error', responseReject);
    response.on('end', () => responseResolve({ status: response.statusCode, body: result }));
  });
  request.on('error', responseReject);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { request.destroy(); reject(Error('Actual server did not accept in-flight JSON with 100 Continue')); }, 10_000);
    request.once('error', error => { clearTimeout(timer); reject(error); });
    request.once('continue', () => {
      request.write(bytes.subarray(0, split), error => { clearTimeout(timer); if (error) reject(error); else resolve(); });
    });
    request.flushHeaders();
  });
  return { accepted: true, responseStarted: () => started,
    async finish() { request.end(bytes.subarray(split)); return responseDone; },
    cancel() { request.destroy(); },
  };
}

export async function pausedDownload(origin, path, expectedBytes) {
  const url = new URL(origin), socket = connect(Number(url.port), url.hostname);
  let header = Buffer.alloc(0), parsed = false, bytes = 0, status, digest = createHash('sha256');
  let completedResolve, completedReject;
  const completed = new Promise((resolve, reject) => { completedResolve = resolve; completedReject = reject; });
  completed.catch(() => {});
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { socket.destroy(); reject(Error('Original download did not return headers')); }, 10_000);
    socket.on('error', error => { clearTimeout(timer); reject(error); completedReject(error); });
    socket.on('data', data => {
      if (!parsed) {
        header = Buffer.concat([header, data]); const end = header.indexOf('\r\n\r\n');
        if (end < 0) { if (header.length > 64 * 1024) { socket.destroy(); reject(Error('Oversized download headers')); } return; }
        const text = header.subarray(0, end).toString('ascii');
        status = Number(text.match(/^HTTP\/1\.[01] (\d{3})/)?.[1]);
        const size = Number(text.match(/\r\ncontent-length: (\d+)/i)?.[1]);
        if (status !== 200 || size !== expectedBytes) { clearTimeout(timer); socket.destroy(); reject(Error('Download status/length disagrees with synthetic original')); return; }
        parsed = true; socket.pause(); clearTimeout(timer);
        data = header.subarray(end + 4); header = Buffer.alloc(0);
        bytes += data.length; digest.update(data); resolve();
      } else { bytes += data.length; digest.update(data); }
    });
    socket.once('connect', () => socket.write(`GET ${path} HTTP/1.1\r\nHost: ${url.host}\r\nConnection: close\r\n\r\n`));
    socket.once('end', () => {
      if (bytes !== expectedBytes) completedReject(Error('Original download was truncated'));
      else completedResolve({ bytes, sha256: digest.digest('hex') });
    });
    socket.once('close', () => { clearTimeout(timer); if (!parsed) reject(Error('Download closed before headers')); if (bytes !== expectedBytes) completedReject(Error('Original download closed incomplete')); });
  });
  await ready;
  return { status, expectedBytes, receivedBytes: () => bytes,
    async resume() { socket.resume(); return completed; }, cancel() { socket.destroy(); } };
}

export function assertSafeDiagnostics(log, traceId, categories, forbiddenValues) {
  const correlated = log.split('\n').find(line => line.includes(`trace_id=${traceId}`));
  if (!correlated || !correlated.includes('route_template=/v1/session') || !correlated.includes('method=GET') || !correlated.includes('status=200')) {
    throw Error('Missing correlated actual-process API diagnostic');
  }
  for (const category of categories) {
    if (!log.split('\n').some(line => line.includes('readiness unavailable') && line.includes(`error_category=${category}`))) {
      throw Error('Missing safe readiness failure category diagnostic');
    }
  }
  if (/(?:postgres(?:ql)?|https?):\/\/|database_url|password|file_path|storage_root|worker_path|synthetic-do-not-log|\[REDACTED\]/i.test(log)
      || forbiddenValues.filter(Boolean).some(value => log.includes(value))) {
    throw Error('Actual-process diagnostics leaked a private value');
  }
}
