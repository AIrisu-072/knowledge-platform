import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createServer as tcpServer, connect } from 'node:net';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as harness from '../harness.mjs';

test('external database requires explicit disposable acknowledgement and loopback', () => {
  assert.throws(() => harness.externalDatabase({ TEST_DATABASE_URL: 'postgres://localhost/poc' }), /disposable/);
  for (const url of ['postgres://prod.example/poc', 'https://localhost/poc', 'postgres://localhost/poc?host=prod.example']) {
    assert.throws(() => harness.externalDatabase({ TEST_DATABASE_URL: url, KP_POC_DISPOSABLE_DATABASE: 'true' }), /loopback/);
  }
  assert.equal(harness.externalDatabase({}), undefined);
  assert.equal(harness.externalDatabase({ TEST_DATABASE_URL: 'postgres://u:p@127.0.0.1:49155/poc', KP_POC_DISPOSABLE_DATABASE: 'true' }), 'postgres://u:p@127.0.0.1:49155/poc');
});

test('owned Docker PostgreSQL uses pinned image, random loopback port and no host mounts', () => {
  const args = harness.postgresArguments('run-123', '/tmp/run-123/cid');
  assert.equal(args.at(-1), 'postgres:18.6-bookworm');
  assert.ok(args.includes('127.0.0.1::5432'));
  assert.ok(args.includes('kp.document-poc.run=run-123'));
  assert.ok(!args.includes('--privileged'));
  assert.ok(!args.includes('-v'));
  assert.ok(!args.join(' ').includes('PASSWORD='));
});

test('profile process environments cannot inherit arbitrary identity or non-loopback override', () => {
  const env = harness.serverEnvironment({ inherited: { PATH: '/bin', KP_POC_ALLOW_NON_LOOPBACK: 'true', KP_IDENTITY_PROFILE: 'bad', KP_DATABASE_URL: 'wrong' }, database: 'postgres://localhost/disposable', profile: 'poc-agent', port: 43210, storage: '/tmp/storage', dsi: '/tmp/dsi', diff: '/tmp/diff', web: '/tmp/web', pdfium: '/tmp/pdfium' });
  assert.equal(env.KP_RUNTIME_MODE, 'poc'); assert.equal(env.KP_POC_ALLOW_NON_LOOPBACK, 'false');
  assert.equal(env.KP_BIND, '127.0.0.1:43210'); assert.equal(env.KP_IDENTITY_PROFILE, 'poc-agent');
  assert.equal(env.KP_DSI_PDFIUM_RUNTIME_DIR, '/tmp/pdfium'); assert.equal(env.KP_WEB_DIST, undefined);
  assert.equal(env.KP_DATABASE_URL, 'postgres://localhost/disposable');
});

test('blocked stage is never pass and later stages remain not-run; evidence persists', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-report-test-'));
  try {
    const report = new harness.EvidenceReport(dir, ['migrate', 'human-start', 'browser']);
    await report.stage('migrate', async () => {});
    await assert.rejects(report.stage('human-start', async () => { throw new harness.Blocked('production DSI preflight unavailable'); }), /preflight/);
    const saved = JSON.parse(await readFile(join(dir, 'report.json'), 'utf8'));
    assert.equal(saved.status, 'blocked');
    assert.deepEqual(saved.stages.map(x => x.status), ['passed', 'blocked', 'not-run']);
    assert.equal(saved.acceptanceQualified, false);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('failed assertion is failed, rather than an infrastructure skip', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-report-test-'));
  try {
    const report = new harness.EvidenceReport(dir, ['browser']);
    await assert.rejects(report.stage('browser', async () => { throw Error('assertion failed'); }));
    assert.equal(report.data.status, 'failed'); assert.equal(report.data.acceptanceQualified, false);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('HTTP readiness checks real response status and fails if child exits', async () => {
  const server = createServer((_req, res) => { res.writeHead(200, { 'content-type': 'application/json' }); res.end('{"status":"ok"}'); });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    await harness.waitReady(`http://127.0.0.1:${server.address().port}`, { exitCode: null }, 500);
    await assert.rejects(harness.waitReady('http://127.0.0.1:1', { exitCode: 1 }, 500), /before readiness/);
  } finally { await new Promise(resolve => server.close(resolve)); }
});

test('classification names only known production sandbox startup blockers', () => {
  assert.equal(harness.startupBlocked('document-server: DSI worker or required native sandbox is unavailable'), true);
  assert.equal(harness.startupBlocked('document-server: Diff worker or required native sandbox is unavailable'), true);
  assert.equal(harness.startupBlocked('document-server: document schema is incompatible'), false);
});

test('command runner executes a real child and propagates nonzero exit', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-process-test-'));
  try {
    const options = { cwd: dir, env: process.env, log: join(dir, 'command.log') };
    assert.equal(await harness.command(process.execPath, ['-e', 'process.stdout.write("real child")'], options), 'real child');
    await assert.rejects(harness.command(process.execPath, ['-e', 'process.exit(17)'], options), /exited 17/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('graceful stop requires an actual clean exit and explicit drain confirmation', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-process-test-'));
  try {
    const owned = harness.startProcess(process.execPath, ['-e', 'process.on("SIGTERM", () => { console.log("graceful drain complete"); process.exit(0); }); console.log("ready"); setInterval(() => {}, 1000);'], { cwd: dir, env: process.env, log: join(dir, 'server.log') });
    while (!owned.output().includes('ready')) await new Promise(resolve => setTimeout(resolve, 5));
    await harness.stopProcess(owned, 1000);
    assert.equal(owned.child.exitCode, 0);
  } finally { await rm(dir, { recursive: true, force: true }); }
});


test('owned database proxy drops existing sessions during outage and resumes the same upstream', async () => {
  const upstream = tcpServer(socket => socket.pipe(socket));
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  let proxy;
  try {
    proxy = await harness.databaseProxy(`postgres://u:p@127.0.0.1:${upstream.address().port}/disposable`);
    const url = new URL(proxy.url);
    assert.equal(url.username, 'u'); assert.equal(url.password, 'p'); assert.equal(url.pathname, '/disposable');
    const echo = () => new Promise((resolve, reject) => {
      const socket = connect(Number(url.port), '127.0.0.1', () => socket.write('same state'));
      socket.on('data', bytes => { socket.end(); resolve(bytes.toString()); }); socket.on('error', reject);
    });
    assert.equal(await echo(), 'same state');
    proxy.pause();
    await new Promise(resolve => {
      const socket = connect(Number(url.port), '127.0.0.1'); socket.on('close', resolve); socket.on('error', () => {});
    });
    proxy.resume(); assert.equal(await echo(), 'same state');
  } finally { if (proxy) await proxy.close(); await new Promise(resolve => upstream.close(resolve)); }
});

test('unexecuted prerequisite keeps final evidence not-run, never qualified', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-report-test-'));
  try {
    const report = new harness.EvidenceReport(dir, ['build', 'browser']);
    await report.stage('browser', async () => {}); await report.finish();
    assert.equal(report.data.status, 'not-run'); assert.equal(report.data.acceptanceQualified, false);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('subprocess evidence redacts credentials even across output chunks', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'poc-process-test-'));
  try {
    const secret = 'postgres://user:do-not-log@localhost/private';
    const result = await harness.command(process.execPath,
      ['-e', 'process.stdout.write("postgres://user:"); setTimeout(() => process.stdout.write("do-not-log@localhost/private"), 30)'],
      { cwd: dir, env: process.env, log: join(dir, 'command.log'), secrets: [secret] });
    assert.equal(result, '[REDACTED]');
    assert.equal(await readFile(join(dir, 'command.log'), 'utf8'), '[REDACTED]');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('paused download observes real headers then resumes exact bytes', async () => {
  const body = Buffer.alloc(1024 * 1024, 'x');
  const server = createServer((_request, response) => {
    response.writeHead(200, { 'content-length': body.length }); response.end(body);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let download;
  try {
    download = await harness.pausedDownload(`http://127.0.0.1:${server.address().port}`, '/original', body.length);
    assert.equal(download.status, 200); assert.equal(download.expectedBytes, body.length);
    const result = await download.resume();
    assert.equal(result.bytes, body.length); assert.equal(result.sha256, await import('node:crypto').then(({ createHash }) => createHash('sha256').update(body).digest('hex')));
  } finally { download?.cancel(); await new Promise(resolve => server.close(resolve)); }
});

test('ordinary JSON request is accepted with 100-continue before body completion', async () => {
  const server = createServer(async (request, response) => {
    let body = ''; for await (const chunk of request) body += chunk;
    response.writeHead(200, { 'content-type': 'application/json' }); response.end(body);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let pending;
  try {
    pending = await harness.delayedJsonRequest(`http://127.0.0.1:${server.address().port}`, '/metadata', { synthetic: true });
    assert.equal(pending.accepted, true); assert.equal(pending.responseStarted(), false);
    const result = await pending.finish(); assert.equal(result.status, 200); assert.deepEqual(JSON.parse(result.body), { synthetic: true });
  } finally { pending?.cancel(); await new Promise(resolve => server.close(resolve)); }
});

test('actual-process diagnostic assertions require correlation/categories and reject private paths', () => {
  const lines = 'INFO document_api_http: document HTTP request trace_id=11111111111111111111111111111111 route_template=/v1/session method=GET status=200 duration_micros=5 invocation_kind=Some("human_interactive") error_code=None\nWARN document_server: readiness unavailable error_category=Storage\n';
  harness.assertSafeDiagnostics(lines, '11111111111111111111111111111111', ['Storage'], ['/private/storage']);
  assert.throws(() => harness.assertSafeDiagnostics(lines + '/private/storage', '11111111111111111111111111111111', ['Storage'], ['/private/storage']), /private/);
  assert.throws(() => harness.assertSafeDiagnostics(lines, '33333333333333333333333333333333', ['Storage'], []), /correlated/);
  assert.throws(() => harness.assertSafeDiagnostics(lines, '11111111111111111111111111111111', ['Database'], []), /category/);
});

test('prepared binary override is diagnostic-only and cannot qualify current source', () => {
  assert.throws(() => harness.binaryDirectory({ root: '/repo', env: { KP_POC_BINARY_DIR: '/prepared-old' }, prebuilt: false }), /prebuilt/);
  assert.equal(harness.binaryDirectory({ root: '/repo', env: { KP_POC_BINARY_DIR: '/prepared-old' }, prebuilt: true }), '/prepared-old');
  assert.equal(harness.binaryDirectory({ root: '/repo', env: { CARGO_TARGET_DIR: '/owned-build' }, prebuilt: false }), '/owned-build/debug');
});
