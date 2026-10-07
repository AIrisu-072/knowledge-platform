// TEST-ONLY: owned synthetic backend for the desktop GUI run. Reuses the
// qualified Document/Organization PoC primitives: one disposable loopback
// PostgreSQL container and the real organization-server (Document + Work API
// on one origin) with the fixed synthetic `sales-01` profile. No production
// identity, data or external service is involved.
import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { command, freePort, postgresArguments, startProcess, stopProcess, waitReady } from '../../../tools/document-poc-runtime/harness.mjs';
import { parsePostgresReadyStatus, postgresReadyArgs, postgresVersionArgs, waitForPostgresTcp } from '../../../tools/document-poc-runtime/postgres-readiness.mjs';
import { organizationEnvironment } from '../../../tools/organization-poc-runtime/settings.mjs';

const SHARED_FOLDER = '00000000-0000-7000-8000-000000000001';

function uuidv7() {
  const bytes = randomBytes(16);
  bytes.writeUIntBE(Date.now(), 0, 6);
  bytes[6] = (bytes[6] & 15) | 0x70;
  bytes[8] = (bytes[8] & 63) | 0x80;
  const hex = bytes.toString('hex');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Creates and publishes one synthetic document through the real Document API. */
async function publishDocument(origin, title, text) {
  const form = new FormData();
  form.append('request', new Blob([JSON.stringify({ folderId: SHARED_FOLDER, title, documentMetadata: {}, versionMetadata: {} })], { type: 'application/json' }));
  form.append('file', new Blob([text], { type: 'text/plain' }), 'desktop-reference.txt');
  const created = await fetch(`${origin}/v1/documents`, { method: 'POST', body: form, signal: AbortSignal.timeout(60_000) });
  assert.equal(created.status, 201, 'synthetic Document create must succeed');
  const { documentId, documentVersionId } = await created.json();
  const detail = await (await fetch(`${origin}/v1/documents/${documentId}?view=authoring`, { signal: AbortSignal.timeout(15_000) })).json();
  const published = await fetch(`${origin}/v1/documents/${documentId}/versions/${documentVersionId}:publish`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ operationId: uuidv7(), expectedRevision: detail.revision }),
    signal: AbortSignal.timeout(60_000),
  });
  assert.equal(published.status, 200, 'synthetic Document publication must succeed');
  await published.arrayBuffer();
  return documentId;
}

/**
 * Starts PostgreSQL 18.6 (docker, loopback, tmpfs) and organization-server,
 * migrates, bootstraps and seeds synthetic data. Returns the backend origin
 * and a stop() that removes only the owned container and process (also
 * handed to `register` before anything starts).
 */
export async function startBackend({ root, directory, pdfium, register }) {
  const runId = randomUUID();
  const binaryDir = join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug');
  const binary = join(binaryDir, 'organization-server');
  const password = randomBytes(24).toString('hex');
  const storage = join(directory, 'backend-storage');
  await mkdir(storage, { recursive: true, mode: 0o700 });
  const secrets = [password];
  const options = (name, env = process.env) => ({ cwd: root, env, log: join(directory, `backend-${name}.log`), secrets });
  const cidfile = join(directory, 'postgres.cid');
  let cid;
  let server;
  let starting;
  const stop = async () => {
    if (server && server.child.exitCode === null && !server.child.signalCode) {
      try { await stopProcess(server); } catch { server.child.kill('SIGKILL'); }
    }
    // A stop during `docker run` (Ctrl-C, SIGTERM) waits for it to settle, then
    // finds the container by its cidfile or, failing that, by this run's label.
    await starting?.catch(() => undefined);
    const ids = new Set([cid ?? (await readFile(cidfile, 'utf8').catch(() => '')).trim()].filter(Boolean));
    const labelled = await command('docker', ['ps', '--all', '--quiet', '--no-trunc', '--filter', `label=kp.document-poc.run=${runId}`], options('postgres-find')).catch(() => '');
    for (const id of labelled.split('\n').map((line) => line.trim()).filter(Boolean)) ids.add(id);
    for (const id of ids) {
      // Remove only containers this run created and labeled.
      const label = await command('docker', ['inspect', '--format', '{{index .Config.Labels "kp.document-poc.run"}}', id], options('postgres-label')).catch(() => '');
      if (label === runId) await command('docker', ['rm', '--force', id], options('postgres-stop')).catch(() => undefined);
    }
  };
  // The caller can stop a backend that is still starting (Ctrl-C / SIGTERM).
  register?.(stop);
  try {
    try {
      starting = command('docker', postgresArguments(runId, cidfile), options('postgres-start', { ...process.env, POSTGRES_PASSWORD: password }));
      await starting;
    } finally {
      cid = (await readFile(cidfile, 'utf8').catch(() => '')).trim() || undefined;
    }
    assert.match(cid ?? '', /^[a-f0-9]{64}$/);
    const binding = await command('docker', ['port', cid, '5432/tcp'], options('postgres-port'));
    assert.match(binding, /^127\.0\.0\.1:\d+$/);
    const database = `postgres://postgres:${password}@${binding}/kp_document_poc`;
    secrets.push(database);
    await waitForPostgresTcp(async (budget) => parsePostgresReadyStatus(await command('docker', postgresReadyArgs(cid), { ...options('postgres-ready'), timeoutMs: budget })));
    const version = await command('docker', postgresVersionArgs(cid), { ...options('postgres-version', { ...process.env, PGPASSWORD: password }), timeoutMs: 10_000 });
    assert.match(version, /^18\.6(?:\s|$)/);
    const port = await freePort();
    const origin = `http://127.0.0.1:${port}`;
    const env = organizationEnvironment({
      inherited: process.env, database, profile: 'sales-01', port, storage,
      dsi: join(binaryDir, 'document-semantic-inspection-worker'), diff: join(binaryDir, 'document-diff-worker'),
      web: join(root, 'apps/document-web/dist'), pdfium,
    });
    await command(binary, ['migrate'], options('migrate', env));
    await command(binary, ['bootstrap-poc'], options('bootstrap', env));
    server = startProcess(binary, ['serve'], options('serve', env));
    await waitReady(origin, server.child);
    const documentId = await publishDocument(origin, 'デスクトップ確認用資料', '【合成データ】デスクトップ版の画面確認に使う共有資料です。\n');
    await command(binary, ['seed-work'], options('seed-work', { ...env, KP_ORGANIZATION_DOCUMENT_ID: documentId }));
    return { origin, documentId, postgres: { image: 'postgres:18.6-bookworm', version }, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}
