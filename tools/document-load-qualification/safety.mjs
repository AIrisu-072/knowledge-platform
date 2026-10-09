import { lstat, opendir, readFile, statfs } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const STAGES = ['small', 1000, 10000, 100000];
const FINGERPRINT_KEYS = ['corpus', 'code', 'runtime'];
const finiteNonnegative = value => typeof value === 'number' && Number.isFinite(value) && value >= 0;
const byteCount = value => Number.isSafeInteger(value) && value >= 0;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const timestamp = value => typeof value === 'string' && /^\d{4}-\d\d-\d\dT.*(?:Z|[+-]\d\d:\d\d)$/.test(value) && Number.isFinite(Date.parse(value));
const fingerprintValid = value => record(value) && FINGERPRINT_KEYS.every(key => typeof value[key] === 'string' && value[key].trim().length > 0);
const sameFingerprint = (a, b) => fingerprintValid(a) && fingerprintValid(b) && FINGERPRINT_KEYS.every(key => a[key] === b[key]);
const countValid = (stage, count) => Number.isSafeInteger(count) && (stage === 'small' ? count >= 1 && count <= 20 : STAGES.includes(stage) && count === stage);

/** No operational budgets or performance SLOs are inferred by this helper. */
export function validatePlan(plan) {
  const errors = [];
  if (!record(plan)) return { valid: false, errors: ['plan must be an object'] };
  if (!STAGES.includes(plan.stage)) errors.push('stage must be small, 1000, 10000 or 100000');
  if (!countValid(plan.stage, plan.documentCount)) errors.push('documentCount must match the stage; small must be 1..20');
  if (!fingerprintValid(plan.fingerprint)) errors.push('fingerprint must contain nonempty corpus, code and runtime strings');
  if (!finiteNonnegative(plan.safetyFactor) || plan.safetyFactor < 1) errors.push('safetyFactor must be finite and at least 1');
  for (const field of ['diskReserveBytes', 'minAvailableMemoryBytes', 'maxRssBytes']) {
    if (!byteCount(plan.budgets?.[field])) errors.push(`budgets.${field} must be explicit nonnegative safe integer bytes`);
  }
  if (plan.budgets?.maxRssBytes === 0) errors.push('budgets.maxRssBytes must be positive');
  if (!finiteNonnegative(plan.budgets?.maxWallTimeMs) || plan.budgets.maxWallTimeMs <= 0) errors.push('budgets.maxWallTimeMs must be explicit finite positive milliseconds');
  if (!timestamp(plan.deadlineAt)) errors.push('deadlineAt must be an explicit ISO timestamp with a timezone');
  return { valid: errors.length === 0, errors };
}

function validateHistory(report, expectedIndex, fingerprint, reasons) {
  // Depth is at most three; checking the exact expected stage also rejects cycles.
  if (!record(report) || report.stage !== STAGES[expectedIndex] || report.status !== 'SUCCEEDED') {
    reasons.push(`a successful ${STAGES[expectedIndex]} report is required; stages cannot be skipped`);
    return;
  }
  const proof=report.restart;
  const sha=value=>typeof value==='string' && /^[a-f0-9]{64}$/.test(value);
  const validPids=value=>Array.isArray(value)&&value.length===2&&value.every(pid=>Number.isSafeInteger(pid)&&pid>0)&&value[0]!==value[1];
  if(report.schemaVersion!==1 || report.evidenceClass!=='owned-real-process'
    || proof?.identityRetained!==true || proof?.processesReplaced!==true
    || !proof.before || JSON.stringify(proof.before)!==JSON.stringify(proof.after)
    || proof.before.sourceHead!==fingerprint.code || !/^[a-f0-9]{40}$/.test(proof.before.sourceHead??'')
    || !sha(proof.before.databaseIdentitySha256) || !sha(proof.before.storageIdentitySha256)
    || !validPids(proof.processes?.before) || !validPids(proof.processes?.after)
    || proof.processes.before.some((pid,index)=>pid===proof.processes.after[index])
    || !Array.isArray(report.evidence?.documentIds) || report.evidence.documentIds.length!==report.documentCount
    || new Set(report.evidence.documentIds).size!==report.documentCount) reasons.push('previous report must carry real owned-process, restart and complete document identity evidence');
  if (!countValid(report.stage, report.documentCount)) reasons.push('previous report has an invalid documentCount');
  if (!sameFingerprint(report.fingerprint, fingerprint)) reasons.push('previous report corpus, code or runtime fingerprint differs');
  for (const field of ['totalElapsedMs', 'diskGrowthBytes', 'peakRssBytes']) {
    const value = report.metrics?.[field];
    if (!finiteNonnegative(value) || (field !== 'diskGrowthBytes' && value === 0)) reasons.push(`previous report is missing measured positive ${field} (disk growth may be zero)`);
  }
  if (expectedIndex > 0) validateHistory(report.previousReport, expectedIndex - 1, fingerprint, reasons);
}

/** Admission is capacity screening, never a product performance qualification. */
export function admitStage(plan, previousReport, observation) {
  const validation = validatePlan(plan);
  const reasons = [...validation.errors];
  let projection = null;
  if (!validation.valid) return { status: 'NOT_ADMITTED', reasons, projection };
  const index = STAGES.indexOf(plan.stage);
  if (index > 0) validateHistory(previousReport, index - 1, plan.fingerprint, reasons);
  if (!record(observation)) reasons.push('resource observation is required');
  for (const field of ['diskFreeBytes', 'storageDiskFreeBytes', 'databaseDiskFreeBytes', 'availableMemoryBytes', 'rssBytes', 'storageBytes', 'databaseBytes']) {
    if (!byteCount(observation?.[field])) reasons.push(`missing or invalid measured ${field}`);
  }
  if (!timestamp(observation?.observedAt)) reasons.push('resource observation requires observedAt timestamp');
  if (observation?.rssCoverage !== 'process-tree') reasons.push('complete process-tree RSS coverage is required');
  if (observation?.databaseFilesystemVerified !== true) reasons.push('database filesystem capacity must be measured or explicitly verified to share storage filesystem');
  const remainingMs = Date.parse(plan.deadlineAt) - Date.now();
  if (remainingMs <= 0) reasons.push('per-run deadline has elapsed');
  if (reasons.length > 0) return { status: 'NOT_ADMITTED', reasons, projection };

  const budgets = plan.budgets;
  if (observation.diskFreeBytes < budgets.diskReserveBytes) reasons.push('current free disk is below disk reserve');
  if (observation.availableMemoryBytes < budgets.minAvailableMemoryBytes) reasons.push('current available memory is below reserve');
  if (observation.rssBytes > budgets.maxRssBytes) reasons.push('current process-tree RSS exceeds budget');
  if (index > 0) {
    const multiplier = plan.documentCount / previousReport.documentCount * plan.safetyFactor;
    projection = {
      basis: 'serial-workload: linear time/disk; prior peak RSS with safety factor; runtime abort guards required', multiplier,
      totalElapsedMs: Math.ceil(previousReport.metrics.totalElapsedMs * multiplier),
      diskGrowthBytes: Math.ceil(previousReport.metrics.diskGrowthBytes * multiplier),
      peakRssBytes: Math.ceil(previousReport.metrics.peakRssBytes * plan.safetyFactor),
    };
    for (const field of ['totalElapsedMs', 'diskGrowthBytes', 'peakRssBytes']) {
      if (!Number.isSafeInteger(projection[field])) reasons.push(`projected ${field} exceeds safe numeric range`);
    }
    if (projection.diskGrowthBytes + budgets.diskReserveBytes > observation.diskFreeBytes) reasons.push('projected disk growth would consume disk reserve');
    // Require the full projected process-tree peak in available memory, conservatively
    // avoiding assumptions about reclaimed memory or overlap with the current process.
    if (projection.peakRssBytes + budgets.minAvailableMemoryBytes > observation.availableMemoryBytes) reasons.push('projected process-tree RSS would consume available-memory reserve');
    if (projection.peakRssBytes > budgets.maxRssBytes) reasons.push('projected process-tree RSS exceeds budget');
    if (projection.totalElapsedMs > budgets.maxWallTimeMs) reasons.push('projected total elapsed time exceeds wall-time budget');
    if (projection.totalElapsedMs > remainingMs) reasons.push('projected total elapsed time exceeds remaining per-run deadline');
  }
  return { status: reasons.length === 0 ? 'ADMITTED' : 'NOT_ADMITTED', reasons, projection };
}

/** Nearest-rank percentiles retain every sample; invalid samples are never dropped. */
export function summarizeTimings(samples) {
  if (!Array.isArray(samples)) throw new TypeError('timings must be an array of finite nonnegative milliseconds');
  const sorted = [...samples];
  if (sorted.some(value => !finiteNonnegative(value))) throw new TypeError('timings must be an array of finite nonnegative milliseconds');
  sorted.sort((a, b) => a - b);
  const count = sorted.length;
  const percentile = p => count === 0 ? null : sorted[Math.ceil(p * count) - 1];
  return {
    count, minMs: count ? sorted[0] : null,
    p50Ms: percentile(0.5), p95Ms: percentile(0.95), p99Ms: percentile(0.99),
    maxMs: count ? sorted[count - 1] : null,
    meanMs: count ? sorted.reduce((sum, value) => sum + value / count, 0) : null,
  };
}

async function storageSize(root, { maxStorageEntries, maxStorageObservationMs }, limitations, diagnostics) {
  if (typeof root !== 'string' || !root) throw new Error('storageRoot is required');
  if (!Number.isSafeInteger(maxStorageEntries) || maxStorageEntries < 1 || !finiteNonnegative(maxStorageObservationMs) || maxStorageObservationMs <= 0) throw new Error('invalid storage observation bounds');
  const scanStarted = performance.now();
  const end = scanStarted + maxStorageObservationMs;
  let entries = 0;
  let total = 0;
  let skippedSymlinks = false;
  const queue = [root];
  while (queue.length > 0) {
    if (++entries > maxStorageEntries || performance.now() > end) throw new Error('storage observation entry/time limit exceeded');
    const current = queue.pop();
    const stat = await lstat(current);
    if (stat.isSymbolicLink()) {
      if (current === root) throw new Error('storage root is a symlink; traversal refused');
      skippedSymlinks = true;
      continue;
    }
    if (stat.isDirectory()) {
      const directory = await opendir(current);
      for await (const entry of directory) {
        if (entries + queue.length >= maxStorageEntries || performance.now() > end) throw new Error('storage observation entry/time limit exceeded');
        queue.push(path.join(current, entry.name));
      }
    } else if (stat.isFile()) {
      total += stat.size;
      if (!byteCount(total)) throw new Error('storage size exceeds safe numeric range');
    } else {
      throw new Error('storage contains a non-regular filesystem entry');
    }
  }
  if (skippedSymlinks) limitations.push('Storage symlinks were excluded without traversal; storageBytes covers regular files only.');
  if(diagnostics)Object.assign(diagnostics,{complete:true,elapsedMs:performance.now()-scanStarted,entries});
  return total;
}

async function diskAvailable(root) {
  if (typeof root !== 'string' || !root) throw new Error('filesystem root is missing');
  const stat = await lstat(root);
  if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error('filesystem root must be a real directory');
  const fs = await statfs(root, { bigint: true });
  const available = Number(fs.bavail * fs.bsize);
  if (!byteCount(available)) throw new Error('filesystem free bytes exceed safe numeric range');
  return available;
}

async function processTreeRss(pids) {
  if (!Array.isArray(pids) || pids.length === 0 || pids.some(pid => !Number.isSafeInteger(pid) || pid < 1)) throw new Error('live process root PIDs are required');
  const { stdout } = await execFileAsync('ps', ['-eo', 'pid=,ppid=,rss='], { timeout: 5000, maxBuffer: 4 * 1024 * 1024, encoding: 'utf8' });
  const processes = new Map();
  const children = new Map();
  for (const line of stdout.trim().split('\n')) {
    const parts = line.trim().split(/\s+/);
    if (parts.length !== 3 || parts.some(value => !/^\d+$/.test(value))) throw new Error('ps returned an invalid process observation');
    const [pid, ppid, kib] = parts.map(Number);
    if (!Number.isSafeInteger(pid) || pid < 1 || !byteCount(ppid) || !byteCount(kib * 1024)) throw new Error('ps returned invalid numeric RSS');
    processes.set(pid, { ppid, rssBytes: kib * 1024 });
    if (!children.has(ppid)) children.set(ppid, []);
    children.get(ppid).push(pid);
  }
  const included = new Set(pids);
  if ([...included].some(pid => !processes.has(pid))) throw new Error('one or more required process roots are absent from ps');
  const queue = [...included];
  while (queue.length > 0) {
    for (const pid of children.get(queue.pop()) ?? []) {
      if (!included.has(pid)) { included.add(pid); queue.push(pid); }
    }
  }
  const rssBytes = [...included].reduce((total, pid) => total + processes.get(pid).rssBytes, 0);
  if (!byteCount(rssBytes) || rssBytes === 0) throw new Error('process-tree RSS is unavailable');
  return { rssBytes, measuredPids: [...included].sort((a, b) => a - b), rssCoverage: 'process-tree' };
}

/**
 * Point-in-time resource observation. databaseBytes must be obtained from the
 * database by the orchestrator; this module never estimates a database size.
 * databaseSharesStorageFilesystem is a deployment assertion, not autodetection.
 * Prefer databaseRoot so both filesystem capacities are actually measured.
 */
export async function observeResources({
  storageRoot, databaseRoot, databaseSharesStorageFilesystem = false, pids, databaseBytes,
  maxStorageEntries = 1_000_000, maxStorageObservationMs = 10_000, includeScanDiagnostics = false,
} = {}) {
  const limitations = ['RSS and storage are point-in-time samples; short-lived processes and between-sample peaks may be missed.'];
  const observation = {
    observedAt: null, diskFreeBytes: null, storageDiskFreeBytes: null, databaseDiskFreeBytes: null, availableMemoryBytes: null,
    rssBytes: null, rssCoverage: 'unavailable', measuredPids: [],
    storageBytes: null, databaseBytes: byteCount(databaseBytes) ? databaseBytes : null,
    databaseFilesystemVerified: false, limitations,
  };
  if(includeScanDiagnostics)observation.storageScan={complete:false,elapsedMs:null,entries:null};
  if (observation.databaseBytes === null) limitations.push('Measured databaseBytes was not supplied by the orchestrator.');
  await Promise.all([
    (async () => {
      try {
        const storageFreeBytes = await diskAvailable(storageRoot);
        observation.storageDiskFreeBytes = storageFreeBytes;
        if (databaseRoot !== undefined) {
          const databaseFreeBytes = await diskAvailable(databaseRoot);
          observation.databaseDiskFreeBytes = databaseFreeBytes;
          observation.diskFreeBytes = Math.min(storageFreeBytes, databaseFreeBytes);
          observation.databaseFilesystemVerified = true;
        } else {
          observation.diskFreeBytes = storageFreeBytes;
          observation.databaseFilesystemVerified = databaseSharesStorageFilesystem === true;
          observation.databaseDiskFreeBytes = databaseSharesStorageFilesystem === true ? storageFreeBytes : null;
          limitations.push(databaseSharesStorageFilesystem === true
            ? 'Database sharing the storage filesystem is an explicit orchestrator assertion; no separate database root was measured.'
            : 'Database filesystem capacity is unverified; supply databaseRoot or an explicit same-filesystem deployment assertion.');
        }
      } catch (error) { limitations.push(`Disk observation unavailable: ${error.message}`); }
    })(),
    (async () => {
      try {
        let available = os.freemem();
        // os.freemem() is conservative about cache but may exceed a container's
        // memory limit. Apply cgroup-v2 headroom when the standard files exist.
        if (process.platform === 'linux') {
          try {
            const [maximum, current] = await Promise.all([
              readFile('/sys/fs/cgroup/memory.max', 'utf8'), readFile('/sys/fs/cgroup/memory.current', 'utf8'),
            ]);
            if (maximum.trim() !== 'max') {
              const limit = Number(maximum.trim()); const used = Number(current.trim());
              if (!byteCount(limit) || !byteCount(used)) throw new Error('invalid cgroup memory counters');
              available = Math.min(available, Math.max(0, limit - used));
            }
          } catch (error) { limitations.push(`Cgroup memory limit was not established: ${error.message}`); }
        }
        if (!byteCount(available)) throw new Error('OS memory observation is invalid');
        observation.availableMemoryBytes = available;
      } catch (error) { limitations.push(`Memory observation unavailable: ${error.message}`); }
    })(),
    (async () => {
      try { observation.storageBytes = await storageSize(storageRoot, { maxStorageEntries, maxStorageObservationMs }, limitations, observation.storageScan); }
      catch (error) { limitations.push(`Storage observation unavailable: ${error.message}`); }
    })(),
    (async () => {
      try { Object.assign(observation, await processTreeRss(pids)); }
      catch (error) { limitations.push(`Process-tree RSS observation unavailable: ${error.message}`); }
    })(),
  ]);
  observation.observedAt = new Date().toISOString();
  return observation;
}
