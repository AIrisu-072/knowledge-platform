import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';

const execute = promisify(execFile);

// Unlike stage logs, these bounded probe results contain raw owned identity.
// Keep them only in memory and never propagate child diagnostics into reports.
export async function privateProvenanceProbe(executable, args, env) {
  try {
    const { stdout } = await execute(executable, args, { env, encoding: 'utf8', timeout: 10_000, killSignal: 'SIGKILL', maxBuffer: 4096 });
    return stdout.trim();
  } catch { throw Error('Owned runtime provenance probe failed'); }
}

const uuid = value => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(value);
const sha = (value, length = 64) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(value);
const port = value => Number.isInteger(value) && value > 0 && value <= 65535;
const portNames = ['human', 'agent', 'postgres', 'proxy'];
const hashNames = ['databaseIdentitySha256', 'storageIdentitySha256', 'fixtureHash'];
const identityHash = values => createHash('sha256').update(JSON.stringify(values)).digest('hex');
const fail = () => { throw Error('Owned runtime provenance could not be verified'); };
const requireProof = condition => { if (!condition) fail(); };
function receipt(value) {
  if (!value || !uuid(value.runId) || !sha(value.sourceHead, 40)
    || !hashNames.every(name => sha(value[name])) || !portNames.every(name => port(value.ports?.[name]))
    || new Set(portNames.map(name => value.ports[name])).size !== portNames.length) return undefined;
  return { runId: value.runId, sourceHead: value.sourceHead,
    ports: Object.fromEntries(portNames.map(name => [name, value.ports[name]])),
    ...Object.fromEntries(hashNames.map(name => [name, value[name]])) };
}
function endpoint(value, protocol) {
  const url = new URL(value);
  requireProof(url.protocol === protocol && url.hostname === '127.0.0.1' && port(Number(url.port)) && !url.search && !url.hash);
  if (protocol === 'http:') requireProof(!url.username && !url.password && url.pathname === '/');
  else requireProof(url.username === 'postgres' && url.password && url.pathname === '/kp_document_poc');
  return url;
}

// Inputs are private observations from this run's owned resources. Only this
// bounded receipt is persisted in the report; raw IDs, paths and URLs stay out.
export async function observeOwnedRuntime(input) {
  try {
    requireProof(uuid(input.runId) && sha(input.sourceHead, 40) && sha(input.cid));
    requireProof(input.container === `${input.cid} ${input.runId}`);
    const human = endpoint(input.human, 'http:'), agent = endpoint(input.agent, 'http:');
    const database = endpoint(input.database, 'postgres:'), proxy = endpoint(input.proxy, 'postgres:');
    requireProof(database.username === proxy.username && database.password === proxy.password && database.pathname === proxy.pathname);
    requireProof(input.binding === `127.0.0.1:${database.port}`);
    const databaseId = /^kp_document_poc:([1-9][0-9]{0,9})$/.exec(input.databaseIdentity);
    requireProof(databaseId && Number(databaseId[1]) <= 4294967295);
    const storage = await lstat(input.storage, { bigint: true });
    requireProof(storage.isDirectory() && !storage.isSymbolicLink() && storage.dev >= 0n && storage.ino > 0n);
    const handle = await open(input.manifestPath, constants.O_RDONLY | constants.O_NOFOLLOW);
    let manifest;
    try {
      const info = await handle.stat();
      requireProof(info.isFile() && info.nlink === 1 && info.size > 0 && info.size <= 2 * 1024 * 1024);
      manifest = JSON.parse(await handle.readFile('utf8'));
    } finally { await handle.close(); }
    requireProof(manifest?.schemaVersion === 1 && manifest.baseUrl === human.origin && sha(manifest.fixtureHash));
    const result = receipt({ runId: input.runId, sourceHead: input.sourceHead,
      ports: { human: Number(human.port), agent: Number(agent.port), postgres: Number(database.port), proxy: Number(proxy.port) },
      databaseIdentitySha256: identityHash(['document-poc-runtime/database/v1', input.runId, input.sourceHead, input.cid, 'kp_document_poc', databaseId[1]]),
      storageIdentitySha256: identityHash(['document-poc-runtime/storage/v1', input.runId, input.sourceHead, String(storage.dev), String(storage.ino)]),
      fixtureHash: manifest.fixtureHash });
    requireProof(result); return result;
  } catch { fail(); }
}

export function assertSameRuntime(initial, observed) {
  const left = receipt(initial), right = receipt(observed);
  requireProof(left && right && JSON.stringify(left) === JSON.stringify(right));
}

// Reports are untrusted. Reconstruct fixed keys and check every receipt against
// the enclosing run/head, rather than echoing observations or caller flags.
export function summarizeRuntimeProvenance(report) {
  const external = report.database?.ownership === 'caller-asserted-disposable';
  const unavailable = { ownership: external ? 'unverified-external' : 'unverified',
    runId: uuid(report.runId) ? report.runId : 'unverified', ports: 'unverified',
    databaseIdentitySha256: 'unverified', storageIdentitySha256: 'unverified', fixtureHash: 'unverified', restartIdentityVerified: false };
  if (external || report.database?.ownership !== 'harness-owned') return unavailable;
  const proofs = report.runtimeProvenance;
  const initial = receipt(proofs?.initial);
  if (!initial || initial.runId !== report.runId || initial.sourceHead !== report.gitHead) return unavailable;
  for (const name of ['beforeRestart', 'afterRestart']) {
    if (proofs[name] === undefined) continue;
    try { assertSameRuntime(initial, proofs[name]); } catch { return unavailable; }
  }
  const stages = Array.isArray(report.stages) ? report.stages.slice(0, 100) : [];
  const verified = ['beforeRestart', 'afterRestart'].every(name => receipt(proofs[name]))
    && ['seed-replay', 'restart', 'browser-persistence'].every(name => stages.some(stage => stage?.name === name && stage.status === 'passed'));
  const { sourceHead: _sourceHead, ...bounded } = initial;
  return { ownership: 'harness-owned', ...bounded, restartIdentityVerified: verified };
}
