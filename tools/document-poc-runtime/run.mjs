#!/usr/bin/env node
// Real processes only. Never a fixture router, dev server, sandbox fallback, or CI skip.
import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { access, chmod, copyFile, cp, mkdir, mkdtemp, readFile, readdir, rename, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { Blocked, EvidenceReport, RUNTIME_STAGES, STARTUP_OBSERVATION_MS, assertSafeDiagnostics, binaryDirectory, command, databaseProxy, delayedJsonRequest, pausedDownload, externalDatabase, freePort, postgresArguments, serverEnvironment, sha256File, startProcess, startupBlocked, stopProcess, waitForDrain, waitForListenerRefusal, waitReady, withUnavailableWorker } from './harness.mjs';

import { readBrowserDiagnostics, sanitizeBrowserPhases } from './browser-diagnostics.mjs';
import { assertOwnedVisualDatabaseInput, exportVisualEvidence } from './visual-evidence.mjs';
import { DatabaseDiagnostics } from './database-diagnostics.mjs';
import { assertSameRuntime, observeOwnedRuntime, privateProvenanceProbe } from './runtime-provenance.mjs';
import { postgresReadyArgs, postgresVersionArgs, parsePostgresReadyStatus, waitForPostgresTcp } from './postgres-readiness.mjs';

import { loadEnabled, runDocumentLoad } from '../document-load-qualification/hosted.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
if (args.some(arg => arg !== '--prebuilt')) throw Error('Usage: node tools/document-poc-runtime/run.mjs [--prebuilt]');
const documentLoadEnabled = loadEnabled(process.env, args.includes('--prebuilt'));
const visualEnabled = process.env.KP_POC_CAPTURE_VISUAL === 'true';
if (process.env.KP_POC_CAPTURE_VISUAL !== undefined && !visualEnabled) throw Error('KP_POC_CAPTURE_VISUAL must be absent or true');
if (visualEnabled && args.includes('--prebuilt')) throw Error('Visual evidence requires built-in-this-run source provenance');
if (visualEnabled) assertOwnedVisualDatabaseInput(process.env);
const base = resolve(process.env.KP_POC_EVIDENCE_DIR ?? join(root, 'tools/document-poc-runtime/.state'));
await mkdir(base, { recursive: true, mode: 0o700 });
const directory = await mkdtemp(join(base, 'run-'));
const runId = randomUUID();
const stages = documentLoadEnabled ? [...RUNTIME_STAGES.slice(0, -1), 'document-load-qualification', RUNTIME_STAGES.at(-1)] : RUNTIME_STAGES;
const report = new EvidenceReport(directory, stages);
const databaseDiagnostics = new DatabaseDiagnostics(report, Boolean(process.env.TEST_DATABASE_URL));
report.data.runId = runId;
report.data.platform = { os: process.platform, arch: process.arch, node: process.version };
report.data.buildMode = args.includes('--prebuilt') ? 'prebuilt-unverified-source-correspondence' : 'built-in-this-run';
report.data.processes = [];
report.data.observationWindows = { startupMs: STARTUP_OBSERVATION_MS, rationale: 'Exceeds DB acquire5s + DSI preflight10s + Diff preflight30s; not a production SLO', drainStillAliveMs: 500 };
const processes = [];
let cid, database, password, proxy;
const log = name => join(directory, `${name}.log`);
const options = (name, env = process.env) => ({ cwd: root, env, log: log(name), secrets: [database, password] });
const run = (name, executable, args, env, timeoutMs) => command(executable, args, { ...options(name, env), ...(timeoutMs === undefined ? {} : { timeoutMs }) });
let visualContext;
let failed = false;
let interrupted = false;
const onSignal = () => { interrupted = true; for (const owned of processes) if (owned.child.exitCode === null) owned.child.kill('SIGTERM'); };
process.on('SIGINT', onSignal); process.on('SIGTERM', onSignal);
try {
  report.data.gitHead = await run('git', 'git', ['rev-parse', 'HEAD']);
  report.data.gitDirty = Boolean(await run('git', 'git', ['status', '--porcelain']));
  await report.stage('toolchain', async () => {
    report.data.tools = { rustc: await run('rustc-version', 'rustc', ['--version']), pnpm: await run('pnpm-version', 'pnpm', ['--version']) };
    report.data.sourceLocks = { cargo: await sha256File(join(root, 'Cargo.lock')), pnpm: await sha256File(join(root, 'pnpm-lock.yaml')) };
  });
  const binaryDir = binaryDirectory({ root, env: process.env, prebuilt: args.includes('--prebuilt') });
  if (args.includes('--prebuilt')) {
    report.data.stages.find(stage => stage.name === 'build').reason = 'Explicit prebuilt mode: build not run and source correspondence unverified';
    await report.save();
  } else await report.stage('build', async () => {
    await run('build-rust', 'cargo', ['build', '--locked', '-p', 'document-server', '-p', 'document-semantic-inspection-worker', '-p', 'document-diff-worker']);
    await run('build-web', 'pnpm', ['--filter', '@knowledge-platform/document-web', 'build']);
    await run('build-mcp', 'pnpm', ['--filter', '@knowledge-platform/document-mcp', 'build']);
    if (documentLoadEnabled) {
      await run('document-load-inspection-tests', 'cargo', ['test', '--locked', '-p', 'document-semantic-inspection-runner', '--example', 'document-load-inspection']);
      await run('document-load-inspection-build', 'cargo', ['build', '--locked', '-p', 'document-semantic-inspection-runner', '--example', 'document-load-inspection']);
    }
  });
  const binary = join(binaryDir, 'document-server');
  const storage = join(directory, 'storage');
  const dsi = join(directory, 'document-semantic-inspection-worker');
  const diff = join(directory, 'document-diff-worker');
  const web = join(directory, 'web');
  const pdfium = process.env.KP_DSI_PDFIUM_RUNTIME_DIR;
  await report.stage('artifacts', async () => {
    if (process.platform !== 'linux') throw new Blocked('Production worker acceptance requires qualified Linux sandboxing');
    if (!pdfium) throw new Blocked('Qualified KP_DSI_PDFIUM_RUNTIME_DIR is required; PDF acceptance cannot be skipped');
    for (const file of [binary, join(binaryDir, 'document-semantic-inspection-worker'), join(binaryDir, 'document-diff-worker')]) {
      try { await access(file, constants.X_OK); } catch { throw new Blocked('A required production executable is missing'); }
    }
    try { await access(join(pdfium, 'libpdfium.so'), constants.R_OK); } catch { throw new Blocked('Qualified PDFium shared library is unavailable'); }
    await mkdir(storage, { mode: 0o700 });
    await copyFile(join(binaryDir, 'document-semantic-inspection-worker'), dsi); await chmod(dsi, 0o700);
    await copyFile(join(binaryDir, 'document-diff-worker'), diff); await chmod(diff, 0o700);
    await cp(join(root, 'apps/document-web/dist'), web, { recursive: true });
    await access(join(web, 'index.html'), constants.R_OK);
    report.data.artifacts = { server: await sha256File(binary), dsi: await sha256File(dsi), diff: await sha256File(diff),
      pdfium: await sha256File(join(pdfium, 'libpdfium.so')),
      mcp: await sha256File(join(root, 'apps/document-mcp/dist/main.cjs')),
      mcpConsistency: await sha256File(join(root, 'apps/document-mcp/dist/consistency.cjs')),
      mcpRuntime: await sha256File(join(root, 'apps/document-mcp/dist/runtime.cjs')), web: {} };
    if (documentLoadEnabled) report.data.artifacts.inspectionProbe = await sha256File(join(binaryDir, 'examples', 'document-load-inspection'));
    async function recordAssets(path, prefix = '') {
      for (const entry of await readdir(path, { withFileTypes: true })) {
        if (entry.isDirectory()) await recordAssets(join(path, entry.name), `${prefix}${entry.name}/`);
        else if (entry.isFile()) report.data.artifacts.web[`${prefix}${entry.name}`] = await sha256File(join(path, entry.name));
        else throw Error('Built GUI contains a non-regular artifact');
      }
    }
    await recordAssets(web);
  });
  await report.stage('database', async () => {
    database = await databaseDiagnostics.step('external-validation', async () => {
      const value = externalDatabase(process.env);
      if (value) password = decodeURIComponent(new URL(value).password) || undefined;
      return value;
    });
    if (database) { report.data.database = { ownership: 'caller-asserted-disposable', cleanup: 'caller-owned; never dropped by harness' }; return; }
    password = randomBytes(24).toString('hex');
    const cidfile = join(directory, 'postgres.cid');
    try {
      await databaseDiagnostics.step('docker-run', () => run('postgres-start', 'docker', postgresArguments(runId, cidfile), { ...process.env, POSTGRES_PASSWORD: password }));
    } catch (error) {
      try { cid = (await readFile(cidfile, 'utf8')).trim(); } catch { /* Docker might not have created the owned container. */ }
      throw new Blocked(`Disposable Docker PostgreSQL could not start: ${error.message}`, { cause: error });
    }
    cid = await databaseDiagnostics.step('cid-read', async () => (await readFile(cidfile, 'utf8')).trim());
    await databaseDiagnostics.step('cid-validation', async () => assert.match(cid, /^[a-f0-9]{64}$/));
    const binding = await databaseDiagnostics.step('port-query', () => run('postgres-port', 'docker', ['port', cid, '5432/tcp']));
    await databaseDiagnostics.step('port-validation', async () => assert.match(binding, /^127\.0\.0\.1:\d+$/));
    database = `postgres://postgres:${password}@${binding}/kp_document_poc`;
    const imageId = await databaseDiagnostics.step('image-inspect', () => run('postgres-image', 'docker', ['inspect', '--format', '{{.Image}}', cid]));
    const digestResult = await databaseDiagnostics.step('repo-digest-query', () => run('postgres-image', 'docker', ['image', 'inspect', '--format', '{{json .RepoDigests}}', imageId]));
    const repoDigests = await databaseDiagnostics.step('repo-digest-parse', async () => JSON.parse(digestResult));
    report.data.database = { ownership: 'harness-owned', image: 'postgres:18.6-bookworm', imageId, repoDigests };
    await databaseDiagnostics.step('readiness', () => waitForPostgresTcp(
      async budget => parsePostgresReadyStatus(await run('postgres-ready', 'docker', postgresReadyArgs(cid), process.env, budget)),
    ));
    // pg_isready alone does not prove this database/credential tuple exists.
    // Do not retry psql exit 2 (connection/auth/config) or exit 3 (SQL error).
    report.data.database.version = await databaseDiagnostics.step('sql-version-query', () =>
      run('postgres-version', 'docker', postgresVersionArgs(cid), { ...process.env, PGPASSWORD: password }, 10_000));
  });
  const upstreamDatabase = database;
  proxy = await databaseDiagnostics.step('proxy-start', () => databaseProxy(database));
  database = proxy.url;
  report.data.database.transport = 'owned transparent loopback TCP proxy for outage/recovery';
  const humanPort = await freePort();
  let agentPort = await freePort(); while (agentPort === humanPort) agentPort = await freePort();
  const human = `http://127.0.0.1:${humanPort}`, agent = `http://127.0.0.1:${agentPort}`;
  const env = profile => serverEnvironment({ inherited: process.env, database, profile, port: profile === 'poc-human' ? humanPort : agentPort, storage, dsi, diff, web, pdfium });
  await report.stage('migrate', () => run('migrate', binary, ['migrate'], env('poc-human')));
  await report.stage('bootstrap', async () => {
    await run('bootstrap', binary, ['bootstrap-poc'], env('poc-human'));
    await run('bootstrap-replay', binary, ['bootstrap-poc'], env('poc-human'));
  });
  async function start(profile, generation) {
    if (interrupted) throw Error('Harness interrupted');
    const owned = startProcess(binary, ['serve'], options(`${profile}-${generation}`, env(profile))); processes.push(owned);
    report.data.processes.push({ profile, generation, pid: owned.child.pid, origin: profile === 'poc-human' ? human : agent, database: 'shared', storage: 'shared' });
    try { await waitReady(profile === 'poc-human' ? human : agent, owned.child); }
    catch (error) {
      // Wait for closed pipes after exit before classifying the actual startup log.
      if (owned.child.exitCode !== null || owned.child.signalCode || owned.child.spawnFailure) await owned.done;
      if (startupBlocked(owned.output())) throw new Blocked('Production DSI/Diff startup preflight failed closed; no sandbox fallback attempted');
      throw error;
    }
    return owned;
  }
  let humanProcess = await report.stage('human-start', () => start('poc-human', 1));
  let agentProcess = await report.stage('agent-start', () => start('poc-agent', 1));
  const traceIds = { human: '11111111111111111111111111111111', agent: '33333333333333333333333333333333' };
  await report.stage('trace-request', async () => {
    for (const [origin, traceId] of [[human, traceIds.human], [agent, traceIds.agent]]) {
      const response = await fetch(`${origin}/v1/session?probe=synthetic-do-not-log`, { headers: {
        traceparent: `00-${traceId}-2222222222222222-01`, 'x-private-probe': 'synthetic-do-not-log',
      } });
      assert.equal(response.status, 200); assert.equal(response.headers.get('trace-id'), traceId);
      await response.arrayBuffer();
    }
  });
  const manifestPath = join(directory, 'seed-manifest.json');
  const seedEnv = { ...process.env, KP_RUNTIME_MODE: 'poc', KP_DOCUMENT_API_BASE_URL: human, KP_POC_SEED_MANIFEST: manifestPath };
  const seed = stage => run(stage, 'pnpm', ['--dir', 'tools/document-poc-seed', 'seed'], seedEnv);
  await report.stage('seed', () => seed('seed'));
  async function recordRuntime(checkpoint) {
    // External disposable databases retain their nonvisual diagnostic path, but
    // cannot establish harness-owned identity and are explicitly unverified.
    if (report.data.database.ownership !== 'harness-owned') return;
    const sourceHead = await run('provenance-head', 'git', ['rev-parse', 'HEAD']);
    assert.equal(sourceHead, report.data.gitHead, 'Owned runtime provenance source changed');
    assert.equal(await run('provenance-dirty', 'git', ['status', '--porcelain']), '', 'Owned runtime provenance source is dirty');
    const container = await privateProvenanceProbe('docker', ['inspect', '--format', '{{.Id}} {{index .Config.Labels "kp.document-poc.run"}}', cid], process.env);
    const binding = await privateProvenanceProbe('docker', ['port', cid, '5432/tcp'], process.env);
    const query = postgresVersionArgs(cid);
    query[query.length - 1] = "SELECT current_database() || ':' || oid::text FROM pg_database WHERE datname = current_database()";
    const databaseIdentity = await privateProvenanceProbe('docker', query, { ...process.env, PGPASSWORD: password });
    const observed = await observeOwnedRuntime({ runId, sourceHead, human, agent,
      database: upstreamDatabase, proxy: proxy.url, storage, manifestPath, cid, container, binding, databaseIdentity });
    if (checkpoint !== 'initial') assertSameRuntime(report.data.runtimeProvenance.initial, observed);
    report.data.runtimeProvenance ??= {};
    report.data.runtimeProvenance[checkpoint] = observed;
  }
  await report.stage('seed-replay', async () => { await seed('seed-replay'); await recordRuntime('initial'); });
  const contextPath = join(directory, 'runtime-context.json');
  const drainFixturePath = join(directory, 'drain-fixture.json');
  visualContext = { runId, human, agent, manifestPath, drainFixturePath, statePath: join(directory, 'persisted-state.json'), workerHashes: { dsi: report.data.artifacts.dsi, diff: report.data.artifacts.diff } };
  if (visualEnabled) {
    const capture = join(directory, 'visual-checkpoints');
    await mkdir(capture, { mode: 0o700 });
    visualContext.visualCapture = { directory: capture, ownership: 'synthetic-owned-runtime', database: 'harness-owned-disposable-loopback' };
  }
  await writeFile(contextPath, JSON.stringify(visualContext), { mode: 0o600 });
  async function browser(phase) {
    const require = createRequire(join(root, 'apps/document-web/package.json'));
    const playwright = require('@playwright/test');
    try { await access(playwright.chromium.executablePath(), constants.X_OK); }
    catch { throw new Blocked('Pinned Playwright Chromium is unavailable; no system-browser substitute accepted'); }
    report.data.browser = { packageVersion: require('@playwright/test/package.json').version, engine: 'bundled-chromium', overrides: false };
    try { await run(`browser-${phase}`, 'pnpm', ['--filter', '@knowledge-platform/document-web', 'exec', 'playwright', 'test', '--config', 'playwright.runtime.config.ts'],
      { ...process.env, KP_POC_RUNTIME_CONTEXT: contextPath, KP_POC_RUNTIME_PHASE: phase, KP_POC_BROWSER_OUTPUT: join(directory, `browser-${phase}`) }); }
    catch (error) {
      const output = await readFile(log(`browser-${phase}`), 'utf8');
      if (/Executable doesn't exist|Host system is missing dependencies|error while loading shared libraries/.test(output)) {
        throw new Blocked('Pinned Playwright browser prerequisite unavailable; inspect browser log and do not substitute a browser or silently install packages');
      }
      throw error;
    } finally {
      report.data.browserDiagnostics = sanitizeBrowserPhases({ ...report.data.browserDiagnostics,
        [phase]: await readBrowserDiagnostics(directory, phase) });
      await report.save();
    }
  }
  await report.stage('browser-journey', () => browser('journey'));
  await report.stage('agent-acceptance', async () => {
    try { await run('agent-acceptance', process.execPath,
      [join(root, 'apps/document-mcp/dist/runtime.cjs')], { ...process.env, KP_POC_RUNTIME_CONTEXT: contextPath }); }
    finally {
      try { report.data.agentAcceptance = JSON.parse(await readFile(join(directory, 'agent-acceptance.json'), 'utf8')); }
      catch { report.data.agentAcceptance = { status: 'UNAVAILABLE' }; }
    }
    assert.equal(report.data.agentAcceptance.status, 'PASS');
    assert.equal(report.data.agentAcceptance.phase, 'complete');
    assert.equal(report.data.agentAcceptance.runId, runId);
    assert.equal(report.data.agentAcceptance.sourceHead, report.data.gitHead);
    assert.equal(report.data.agentAcceptance.mainSha256, report.data.artifacts.mcp);
    assert.equal(report.data.agentAcceptance.runtimeSha256, report.data.artifacts.mcpRuntime);
    assert.equal(report.data.agentAcceptance.workspaceLockSha256, report.data.sourceLocks.pnpm);
  });
  await report.stage('health-recovery', async () => {
    async function health(origin, readyStatus) {
      for (const [path, status, value] of [['live', 200, 'ok'], ['ready', readyStatus, readyStatus === 200 ? 'ok' : 'unavailable']]) {
        const response = await fetch(`${origin}/health/${path}`, { signal: AbortSignal.timeout(35_000) });
        assert.equal(response.status, status); assert.deepEqual(await response.json(), { status: value });
      }
    }
    proxy.pause();
    try { await health(human, 503); await health(agent, 503); }
    finally { proxy.resume(); }
    await waitReady(human, humanProcess.child); await waitReady(agent, agentProcess.child);
    await health(human, 200); await health(agent, 200);
    // Mutate only copies owned by this run; restore each before proceeding.
    for (const worker of ['dsi', 'diff']) {
      await withUnavailableWorker(directory, worker, report.data.artifacts[worker], async () => {
        await health(human, 503); await health(agent, 503);
      });
      await health(human, 200); await health(agent, 200);
    }
    for (const path of [storage, join(web, 'index.html')]) {
      await rename(path, `${path}.unavailable`);
      try { await health(human, 503); if (path === storage) await health(agent, 503); }
      finally { await rename(`${path}.unavailable`, path); }
      await health(human, 200); await health(agent, 200);
    }
  });
  const drain = JSON.parse(await readFile(drainFixturePath, 'utf8'));
  report.data.drainObservations = {};
  await report.stage('ordinary-request-sigterm', async () => {
    const started = Date.now();
    const pending = await delayedJsonRequest(human, `/v1/documents/${drain.documentId}/metadata`, drain.mutation);
    try {
      assert.equal(pending.accepted, true); assert.equal(pending.responseStarted(), false);
      humanProcess.child.kill('SIGTERM');
      await waitForListenerRefusal(human);
      await delay(500);
      assert.equal(humanProcess.child.exitCode, null, 'Server exited while an accepted ordinary request body was incomplete');
      assert.equal(humanProcess.child.signalCode, null);
      const result = await pending.finish();
      assert.equal(result.status, 200, 'Ordinary in-flight mutation must report its actual successful result');
      const mutation = JSON.parse(result.body);
      assert.equal(mutation.operationId, drain.mutation.operationId);
      await waitForDrain(humanProcess);
      report.data.drainObservations.ordinary = { acceptedBy100Continue: true, refusedNewConnections: true,
        remainedDrainingUntilRelease: true, completedStatus: result.status, operationId: mutation.operationId,
        resultingRevision: mutation.resultingRevision, elapsedMs: Date.now() - started };
    } finally { pending.cancel(); }
  });
  await report.stage('stalled-download-sigterm', async () => {
    humanProcess = await start('poc-human', 2);
    const originalPath = `/v1/documents/${drain.documentId}/versions/${drain.versionId}/files/${drain.contentItemId}/${drain.representationId}?purpose=authoring`;
    const download = await pausedDownload(human, originalPath, drain.sizeBytes);
    const started = Date.now();
    try {
      assert.ok(download.receivedBytes() < drain.sizeBytes, 'Download must still be incomplete before SIGTERM');
      humanProcess.child.kill('SIGTERM');
      await waitForListenerRefusal(human);
      await delay(500);
      assert.equal(humanProcess.child.exitCode, null, 'Server exited with an incomplete paused original stream');
      assert.equal(humanProcess.child.signalCode, null);
      let timer;
      const result = await Promise.race([download.resume(), new Promise((_, reject) => {
        timer = setTimeout(() => reject(Error('Original did not finish after client release within the test observation window')), 60_000);
      })]).finally(() => clearTimeout(timer));
      assert.equal(result.bytes, drain.sizeBytes); assert.equal(result.sha256, drain.sha256);
      await waitForDrain(humanProcess);
      report.data.drainObservations.download = { refusedNewConnections: true, remainedDrainingUntilRelease: true,
        verifiedBytes: result.bytes, sha256: result.sha256, elapsedMs: Date.now() - started };
    } finally { download.cancel(); }
  });
  await report.stage('shutdown', async () => { await stopProcess(agentProcess); });
  await report.stage('agent-outage', () => run('agent-outage', process.execPath,
    [join(root, 'apps/document-mcp/test/outage.mjs')], { ...process.env, KP_POC_RUNTIME_CONTEXT: contextPath }));
  await report.stage('diagnostics', async () => {
    const forbidden = [database, password, directory, storage, dsi, diff, web, pdfium, binary];
    assertSafeDiagnostics(await readFile(log('poc-human-1'), 'utf8'), traceIds.human,
      ['Database', 'DsiWorker', 'DiffWorker', 'Storage', 'Web'], forbidden);
    assertSafeDiagnostics(await readFile(log('poc-agent-1'), 'utf8'), traceIds.agent,
      ['Database', 'DsiWorker', 'DiffWorker', 'Storage'], forbidden);
  });
  await report.stage('restart', async () => {
    await recordRuntime('beforeRestart');
    humanProcess = await start('poc-human', 3); agentProcess = await start('poc-agent', 2);
  });
  await report.stage('browser-persistence', async () => { await browser('persistence'); await recordRuntime('afterRestart'); });
  if (documentLoadEnabled) await report.stage('document-load-qualification', async () => {
    report.data.documentLoadQualification = await runDocumentLoad({ root, directory, runId,
      sourceHead: report.data.gitHead, artifacts: report.data.artifacts, storage, cid, password, human, agent, run, worker: dsi, pdfium,
      getPids: () => [humanProcess.child.pid, agentProcess.child.pid],
      identity: async () => { await recordRuntime('documentLoadCheckpoint'); return {
        ...report.data.runtimeProvenance.documentLoadCheckpoint,
        humanPid: humanProcess.child.pid, agentPid: agentProcess.child.pid,
      }; },
      restart: async () => {
        await stopProcess(humanProcess); await stopProcess(agentProcess);
        humanProcess = await start('poc-human', 4); agentProcess = await start('poc-agent', 3);
      },
    });
  });
  await report.stage('final-shutdown', async () => { await stopProcess(humanProcess); await stopProcess(agentProcess); });
} catch (error) {
  failed = true;
  if (report.data.status === 'running') { report.data.status = error instanceof Blocked ? 'blocked' : 'failed'; report.data.failure = error.message; }
  console.error(`Document runtime acceptance ${report.data.status}; local evidence retained`);
} finally {
  const cleanup = [];
  for (const owned of processes) {
    if (owned.child.exitCode !== null || owned.child.signalCode || owned.child.spawnFailure) continue;
    try { await stopProcess(owned); cleanup.push({ pid: owned.child.pid, result: 'gracefully-stopped' }); }
    catch {
      // Test-resource cleanup only; never count a force-cleaned process as graceful acceptance.
      owned.child.kill('SIGKILL'); await owned.done;
      cleanup.push({ pid: owned.child.pid, result: 'forced-test-cleanup' }); failed = true; report.data.status = 'failed';
    }
  }
  if (proxy) await proxy.close();
  if (cid) {
    try {
      assert.match(cid, /^[a-f0-9]{64}$/);
      const owner = await run('postgres-owner', 'docker', ['inspect', '--format', '{{index .Config.Labels "kp.document-poc.run"}}', cid]);
      assert.equal(owner, runId);
      await run('postgres-cleanup', 'docker', ['rm', '--force', cid]);
      cleanup.push({ resource: 'owned-postgres', result: 'removed' });
    } catch { cleanup.push({ resource: 'owned-postgres', result: 'cleanup-unconfirmed' }); failed = true; report.data.status = 'failed'; }
  }
  report.data.cleanup = cleanup;
  if (interrupted) { failed = true; report.data.status = 'failed'; report.data.failure = 'Harness interrupted'; }
  // finish must not override cleanup failures with passing acceptance.
  if (failed && report.data.stages.every(stage => stage.status === 'passed')) report.data.stages.push({ name: 'cleanup', status: 'failed' });
  await report.finish();
  if (visualEnabled && report.data.acceptanceQualified) {
    try {
      const currentSource = { gitHead: await run('visual-git-head', 'git', ['rev-parse', 'HEAD']),
        gitDirty: Boolean(await run('visual-git-dirty', 'git', ['status', '--porcelain'])) };
      await exportVisualEvidence({ runDirectory: directory, report: report.data, context: visualContext, currentSource });
      report.data.visualEvidence = { status: 'validated-local-export', count: 13, visualReview: 'NOT RUN' };
    } catch {
      report.data.visualEvidence = { status: 'validation-failed', visualReview: 'NOT RUN' };
      report.data.acceptanceQualified = false; report.data.status = 'failed';
    }
    await report.save();
  }
  console.log(`Document runtime evidence ${report.data.status}; use the bounded CI summary command`);
  process.exitCode = report.data.acceptanceQualified ? 0 : 1;
}
