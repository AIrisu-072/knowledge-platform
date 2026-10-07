#!/usr/bin/env node
// Separate owned synthetic runtime, using the qualified Document process/DB primitives.
import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { access, appendFile, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { Blocked, EvidenceReport, binaryDirectory, command, freePort, postgresArguments, startProcess, stopProcess, waitReady } from '../document-poc-runtime/harness.mjs';
import { postgresReadyArgs, postgresVersionArgs, parsePostgresReadyStatus, waitForPostgresTcp } from '../document-poc-runtime/postgres-readiness.mjs';
import { ORGANIZATION_PROFILES, organizationEnvironment } from './settings.mjs';
import { readBrowserFailureDiagnostics } from './browser-diagnostics.mjs';

if (process.argv.length !== 2) throw Error('Organization runtime accepts no alternate or prebuilt mode');
if (process.env.TEST_DATABASE_URL || process.env.WORK_POC_TEST_DATABASE_URL) throw Error('Organization acceptance creates its own disposable databases');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const base = join(root, 'tools/organization-poc-runtime/.state');
await mkdir(base, { recursive: true, mode: 0o700 });
const directory = await mkdtemp(join(base, 'run-'));
const runId = randomUUID();
const report = new EvidenceReport(directory, ['build', 'database', 'transaction', 'initialize', 'journey', 'restart', 'persistence', 'shutdown',
  'policy-initialize', 'policy-journey', 'policy-restart', 'policy-persistence',
  'context-seed', 'context-journey', 'context-restart', 'context-persistence', 'policy-shutdown']);
report.data.scope = 'Synthetic Organization Browser PoC: actual PostgreSQL transaction, two-principal browser journey and a separate fresh-database six-principal policy journey';
report.data.runId = runId;
const processes = [];
let cid, binding, database, testDatabase, policyDatabase, password, failed = false, interrupted = false;
const options = (name, env = process.env) => ({ cwd: root, env, log: join(directory, `${name}.log`), secrets: [database, testDatabase, policyDatabase, password] });
const run = (name, exe, args, env, timeoutMs) => command(exe, args, { ...options(name, env), ...(timeoutMs ? { timeoutMs } : {}) });
const signal = () => { interrupted = true; for (const owned of processes) if (owned.child.exitCode === null) owned.child.kill('SIGTERM'); };
process.on('SIGTERM', signal); process.on('SIGINT', signal);
try {
  report.data.gitHead = await run('head', 'git', ['rev-parse', 'HEAD']);
  assert.equal(await run('dirty', 'git', ['status', '--porcelain']), '', 'Hosted qualification requires committed source');
  const binaryDir = binaryDirectory({ root, env: process.env, prebuilt: false });
  const binary = join(binaryDir, 'organization-server');
  const dsi = join(binaryDir, 'document-semantic-inspection-worker');
  const diff = join(binaryDir, 'document-diff-worker');
  const web = join(root, 'apps/document-web/dist');
  const storage = join(directory, 'storage');
  const pdfium = process.env.KP_DSI_PDFIUM_RUNTIME_DIR;
  if (process.platform !== 'linux' || !pdfium) throw new Blocked('Qualified Linux Document worker/PDFium prerequisites required');
  await report.stage('build', async () => {
    await run('build-rust', 'cargo', ['build', '--locked', '-p', 'organization-server', '-p', 'document-semantic-inspection-worker', '-p', 'document-diff-worker']);
    await run('build-web', 'pnpm', ['--filter', '@knowledge-platform/document-web', 'build']);
    for (const path of [binary, dsi, diff]) await access(path, constants.X_OK);
    await access(join(pdfium, 'libpdfium.so'), constants.R_OK);
    await access(join(web, 'index.html'), constants.R_OK);
    await mkdir(storage, { mode: 0o700 });
  });
  await report.stage('database', async () => {
    password = randomBytes(24).toString('hex');
    const cidfile = join(directory, 'postgres.cid');
    // Six principal processes each hold two lazily opened pools (Document and Work, up to 12
    // connections each); raise the owned container's limit above that worst case.
    try { await run('postgres-start', 'docker', [...postgresArguments(runId, cidfile), '-c', 'max_connections=200'], { ...process.env, POSTGRES_PASSWORD: password }); }
    finally { try { cid = (await readFile(cidfile, 'utf8')).trim(); } catch { /* No owned container was created. */ } }
    assert.match(cid ?? '', /^[a-f0-9]{64}$/);
    binding = await run('postgres-port', 'docker', ['port', cid, '5432/tcp']);
    assert.match(binding, /^127\.0\.0\.1:\d+$/);
    database = `postgres://postgres:${password}@${binding}/kp_document_poc`;
    testDatabase = `postgres://postgres:${password}@${binding}/organization_work_poc_test`;
    await waitForPostgresTcp(async budget => parsePostgresReadyStatus(await run('postgres-ready', 'docker', postgresReadyArgs(cid), process.env, budget)));
    const pgEnv = { ...process.env, PGPASSWORD: password };
    const version = await run('postgres-version', 'docker', postgresVersionArgs(cid), pgEnv, 10_000);
    assert.match(version, /^18\.6(?:\s|$)/);
    const create = postgresVersionArgs(cid);
    create[create.length - 1] = 'CREATE DATABASE organization_work_poc_test';
    await run('create-work-test-database', 'docker', create, pgEnv, 10_000);
    report.data.database = { ownership: 'harness-owned-disposable', image: 'postgres:18.6-bookworm', version, transport: 'owned-loopback-tcp', isolatedTransactionDatabase: true };
  });
  await report.stage('transaction', () => run('transaction', 'cargo', ['test', '--locked', '-p', 'work-repository-postgres', '--test', 'postgres_transaction', '--', '--ignored', '--test-threads=1'], { ...process.env, WORK_POC_TEST_DATABASE_URL: testDatabase }, 180_000));
  const salesPort = await freePort();
  let officePort = await freePort(); while (officePort === salesPort) officePort = await freePort();
  const sales = `http://127.0.0.1:${salesPort}`, office = `http://127.0.0.1:${officePort}`;
  const env = profile => organizationEnvironment({ inherited: process.env, database, profile, port: profile === 'sales-01' ? salesPort : officePort, storage, dsi, diff, web, pdfium });
  async function start(profile, generation) {
    if (interrupted) throw Error('Runtime interrupted');
    const owned = startProcess(binary, ['serve'], options(`${profile}-${generation}`, env(profile)));
    processes.push(owned);
    await waitReady(profile === 'sales-01' ? sales : office, owned.child);
    return owned;
  }
  // Create and publish the synthetic shared input through the existing Document API only.
  async function publishSharedDocument(origin) {
    const form = new FormData();
    form.append('request', new Blob([JSON.stringify({ folderId: '00000000-0000-7000-8000-000000000001', title: 'PoC共有参照資料', documentMetadata: {}, versionMetadata: {} })], { type: 'application/json' }));
    form.append('file', new Blob(['【合成データ】2名の提出確認に使う共有資料です。\n'], { type: 'text/plain' }), 'organization-reference.txt');
    const createdResponse = await fetch(`${origin}/v1/documents`, { method: 'POST', body: form, signal: AbortSignal.timeout(60_000) });
    assert.equal(createdResponse.status, 201, 'Synthetic Document create must succeed; do not retry unknown outcomes');
    const created = await createdResponse.json(); const createdId = created.documentId;
    assert.match(createdId, /^[0-9a-f-]{36}$/);
    const detailResponse = await fetch(`${origin}/v1/documents/${createdId}?view=authoring`, { signal: AbortSignal.timeout(15_000) });
    assert.equal(detailResponse.status, 200);
    const detail = await detailResponse.json();
    const bytes = randomBytes(16); bytes.writeUIntBE(Date.now(), 0, 6); bytes[6] = (bytes[6] & 15) | 0x70; bytes[8] = (bytes[8] & 63) | 0x80;
    const hex = bytes.toString('hex'); const operationId = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
    const published = await fetch(`${origin}/v1/documents/${createdId}/versions/${created.documentVersionId}:publish`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ operationId, expectedRevision: detail.revision }), signal: AbortSignal.timeout(60_000) });
    assert.equal(published.status, 200, 'Synthetic Document publication must succeed');
    await published.arrayBuffer();
    return createdId;
  }
  let salesProcess, officeProcess, documentId;
  await report.stage('initialize', async () => {
    await run('migrate', binary, ['migrate'], env('sales-01'));
    await run('bootstrap', binary, ['bootstrap-poc'], env('sales-01'));
    await run('bootstrap-replay', binary, ['bootstrap-poc'], env('sales-01'));
    salesProcess = await start('sales-01', 1);
    officeProcess = await start('office-01', 1);
    documentId = await publishSharedDocument(sales);
    await run('seed-work', binary, ['seed-work'], { ...env('sales-01'), KP_ORGANIZATION_DOCUMENT_ID: documentId });
    await run('seed-replay', binary, ['seed-work'], { ...env('sales-01'), KP_ORGANIZATION_DOCUMENT_ID: documentId });
  });
  const contextPath = join(directory, 'context.json');
  await writeFile(contextPath, JSON.stringify({ sales, office, documentId, statePath: join(directory, 'state.json') }), { mode: 0o600 });
  const require = createRequire(join(root, 'apps/document-web/package.json'));
  await access(require('@playwright/test').chromium.executablePath(), constants.X_OK);
  async function browser(phase, runtimeContext = contextPath) {
    try {
      await run(`browser-${phase}`, 'pnpm', ['--filter', '@knowledge-platform/document-web', 'exec', 'playwright', 'test', '--config', 'playwright.organization.config.ts'], { ...process.env, KP_ORGANIZATION_RUNTIME_CONTEXT: runtimeContext, KP_ORGANIZATION_RUNTIME_PHASE: phase, KP_ORGANIZATION_BROWSER_OUTPUT: join(directory, `browser-${phase}`), PLAYWRIGHT_JSON_OUTPUT_FILE: join(directory, `browser-${phase}`, 'results.json') });
    } catch (error) {
      console.error(`Organization browser failure: ${JSON.stringify(await readBrowserFailureDiagnostics(directory, phase))}`);
      throw error;
    }
  }
  await report.stage('journey', () => browser('journey'));
  await report.stage('restart', async () => {
    await stopProcess(salesProcess); await stopProcess(officeProcess);
    salesProcess = await start('sales-01', 2); officeProcess = await start('office-01', 2);
  });
  await report.stage('persistence', () => browser('persistence'));
  await report.stage('shutdown', async () => { await stopProcess(salesProcess); await stopProcess(officeProcess); });

  // Six fixed-profile processes on a separate fresh database, so the accepted
  // two-principal journey above stays unchanged. Identity is per process only.
  const roles = { 'sales-01': 'sales', 'office-01': 'office', 'review-01': 'review', 'approver-01': 'approver', 'multi-role-01': 'multiRole', 'delegate-01': 'delegate' };
  const policyPorts = {}, policyProcesses = {};
  const policyStorage = join(directory, 'policy-storage');
  const policyContextPath = join(directory, 'policy-context.json');
  const policyOrigin = profile => `http://127.0.0.1:${policyPorts[profile]}`;
  const policyEnv = profile => organizationEnvironment({ inherited: process.env, database: policyDatabase, profile, port: policyPorts[profile], storage: policyStorage, dsi, diff, web, pdfium });
  async function startPolicy(generation) {
    if (interrupted) throw Error('Runtime interrupted');
    await Promise.all(ORGANIZATION_PROFILES.map(async profile => {
      const owned = startProcess(binary, ['serve'], options(`policy-${profile}-${generation}`, policyEnv(profile)));
      processes.push(owned); policyProcesses[profile] = owned;
      await waitReady(policyOrigin(profile), owned.child);
    }));
  }
  const stopPolicy = () => Promise.all(ORGANIZATION_PROFILES.map(profile => stopProcess(policyProcesses[profile])));
  let policyDocumentId;
  await report.stage('policy-initialize', async () => {
    const create = postgresVersionArgs(cid);
    create[create.length - 1] = 'CREATE DATABASE kp_organization_policy_poc';
    await run('create-policy-database', 'docker', create, { ...process.env, PGPASSWORD: password }, 10_000);
    policyDatabase = `postgres://postgres:${password}@${binding}/kp_organization_policy_poc`;
    await mkdir(policyStorage, { mode: 0o700 });
    const used = new Set([salesPort, officePort]);
    for (const profile of ORGANIZATION_PROFILES) {
      let port = await freePort(); while (used.has(port)) port = await freePort();
      used.add(port); policyPorts[profile] = port;
    }
    await run('policy-migrate', binary, ['migrate'], policyEnv('sales-01'));
    await run('policy-bootstrap', binary, ['bootstrap-poc'], policyEnv('sales-01'));
    await startPolicy(1);
    policyDocumentId = await publishSharedDocument(policyOrigin('sales-01'));
    await run('policy-seed', binary, ['seed-work'], { ...policyEnv('sales-01'), KP_ORGANIZATION_DOCUMENT_ID: policyDocumentId });
    const origins = Object.fromEntries(ORGANIZATION_PROFILES.map(profile => [roles[profile], policyOrigin(profile)]));
    await writeFile(policyContextPath, JSON.stringify({ ...origins, documentId: policyDocumentId, statePath: join(directory, 'policy-state.json'), contextStatePath: join(directory, 'context-state.json') }), { mode: 0o600 });
    report.data.policy = { profiles: ORGANIZATION_PROFILES.length, database: 'separate-fresh-owned' };
  });
  await report.stage('policy-journey', () => browser('policy-journey', policyContextPath));
  await report.stage('policy-restart', async () => { await stopPolicy(); await startPolicy(2); });
  await report.stage('policy-persistence', () => browser('policy-persistence', policyContextPath));
  // Additional synthetic WorkContexts through the explicit command on the same (now
  // existing) database: the upgrade path, never a reset of earlier progress.
  await report.stage('context-seed', async () => {
    await run('context-seed', binary, ['seed-contexts'], { ...policyEnv('sales-01'), KP_ORGANIZATION_DOCUMENT_ID: policyDocumentId });
    await run('context-seed-replay', binary, ['seed-contexts'], { ...policyEnv('sales-01'), KP_ORGANIZATION_DOCUMENT_ID: policyDocumentId });
    report.data.contexts = { seeded: 'explicit-command', replay: 'idempotent' };
  });
  await report.stage('context-journey', () => browser('context-journey', policyContextPath));
  await report.stage('context-restart', async () => { await stopPolicy(); await startPolicy(3); });
  await report.stage('context-persistence', () => browser('context-persistence', policyContextPath));
  await report.stage('policy-shutdown', stopPolicy);
} catch (error) {
  failed = true;
  if (report.data.status === 'running') report.data.status = error instanceof Blocked ? 'blocked' : 'failed';
  console.error('Organization runtime acceptance did not pass; inspect the private stage log');
} finally {
  for (const owned of processes) {
    if (owned.child.exitCode !== null || owned.child.signalCode || owned.child.spawnFailure) continue;
    try { await stopProcess(owned); } catch { owned.child.kill('SIGKILL'); await owned.done; failed = true; }
  }
  if (cid) {
    try {
      assert.match(cid, /^[a-f0-9]{64}$/);
      assert.equal(await run('postgres-owner', 'docker', ['inspect', '--format', '{{index .Config.Labels "kp.document-poc.run"}}', cid]), runId);
      await run('postgres-cleanup', 'docker', ['rm', '--force', cid]);
      report.data.cleanup = 'owned-container-removed';
    } catch { failed = true; report.data.cleanup = 'unconfirmed'; }
  }
  if (interrupted) failed = true;
  if (failed && report.data.stages.every(stage => stage.status === 'passed')) report.data.stages.push({ name: 'cleanup', status: 'failed' });
  await report.finish();
  const cleanup = ['owned-container-removed', 'unconfirmed'].includes(report.data.cleanup) ? `cleanup: ${report.data.cleanup}\n` : '';
  const summary = `Organization Browser PoC: ${report.data.status}\n${report.data.stages.map(stage => `${stage.name}: ${stage.status}`).join('\n')}\n${cleanup}`;
  console.log(summary);
  if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, `## Organization Browser PoC\n\nSource: ${report.data.gitHead ?? 'unknown'}\n\n${report.data.stages.map(stage => `- ${stage.name}: ${stage.status}`).join('\n')}\n\nSynthetic data only; no screenshots, traces, videos or runtime files uploaded.\n`);
  process.exitCode = report.data.acceptanceQualified ? 0 : 1;
}
