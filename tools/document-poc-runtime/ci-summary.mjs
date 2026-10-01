#!/usr/bin/env node
// The report is treated as untrusted input: never print reasons, URLs, paths or logs.
import { readFile, readdir, stat } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { RUNTIME_STAGES } from './harness.mjs';

const statuses = new Set(['passed', 'failed', 'blocked', 'not-run', 'running']);
const sha = (value, length = 64) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(value) ? value : 'unverified';
const version = value => typeof value === 'string' ? value.match(/^v?(\d+\.\d+\.\d+)(?:$|[ (])/u)?.[1] ?? 'unverified' : 'unverified';
const code = (name, status) => status === 'not-run' ? 'not-executed' : status === 'running' ? 'execution-incomplete'
  : status === 'failed' ? 'stage-failed'
    : ['human-start', 'agent-start', 'restart'].includes(name) ? 'worker-preflight-unavailable'
      : name === 'database' ? 'disposable-database-unavailable'
        : name.startsWith('browser-') ? 'pinned-browser-unavailable' : 'prerequisite-unavailable';

export function summarize(report) {
  if (!report || typeof report !== 'object' || Array.isArray(report)) return { status: 'not-available', acceptanceQualified: false, failureCode: 'report-not-available' };
  const inputStages = Array.isArray(report.stages) ? report.stages.slice(0, 100) : [];
  const stages = RUNTIME_STAGES.flatMap(name => {
    const stage = inputStages.find(item => item && item.name === name && statuses.has(item.status));
    return stage ? [{ name, status: stage.status, ...(stage.status === 'passed' ? {} : { failureCode: code(name, stage.status) }) }] : [];
  });
  const gitHead = sha(report.gitHead, 40);
  const artifacts = Object.fromEntries(['server', 'dsi', 'diff', 'pdfium'].map(name => [name, sha(report.artifacts?.[name])]));
  const webAssetHashes = Object.values(report.artifacts?.web ?? {}).map(value => sha(value)).filter(value => value !== 'unverified').sort().slice(0, 32);
  return {
    status: statuses.has(report.status) ? report.status : 'not-available',
    acceptanceQualified: report.status === 'passed' && report.acceptanceQualified === true && gitHead !== 'unverified'
      && report.gitDirty === false && stages.length === RUNTIME_STAGES.length && stages.every(stage => stage.status === 'passed'),
    gitHead, gitDirty: typeof report.gitDirty === 'boolean' ? report.gitDirty : 'unverified',
    platform: { os: ['linux', 'darwin', 'win32'].includes(report.platform?.os) ? report.platform.os : 'unverified',
      arch: ['x64', 'arm64', 'ia32'].includes(report.platform?.arch) ? report.platform.arch : 'unverified' },
    tools: { node: version(report.platform?.node), pnpm: version(report.tools?.pnpm),
      rustc: typeof report.tools?.rustc === 'string' ? version(report.tools.rustc.replace(/^rustc /u, '')) : 'unverified',
      playwright: version(report.browser?.packageVersion) },
    postgresVersion: report.database?.ownership === 'caller-asserted-disposable' ? 'unverified-external'
      : typeof report.database?.version === 'string' ? report.database.version.match(/^(\d+\.\d+(?:\.\d+)?)(?:$|[ (])/u)?.[1] ?? 'unverified' : 'unverified',
    sourceLocks: { cargo: sha(report.sourceLocks?.cargo), pnpm: sha(report.sourceLocks?.pnpm) },
    artifacts, webAssetHashes,
    profiles: ['poc-human', 'poc-agent'].filter(profile => Array.isArray(report.processes) && report.processes.some(item => item?.profile === profile)),
    stages,
  };
}

export async function latestReport(directory) {
  try {
    const entries = (await readdir(directory, { withFileTypes: true })).filter(entry => entry.isDirectory() && /^run-[A-Za-z0-9_-]+$/.test(entry.name)).slice(-100);
    const reports = await Promise.all(entries.map(async entry => {
      const path = join(directory, entry.name, 'report.json');
      try { const info = await stat(path); return info.isFile() && info.size <= 2 * 1024 * 1024 ? { path, modified: info.mtimeMs } : null; }
      catch { return null; }
    }));
    const latest = reports.filter(Boolean).sort((a, b) => b.modified - a.modified)[0];
    return latest ? JSON.parse(await readFile(latest.path, 'utf8')) : null;
  } catch { return null; }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = resolve(process.env.KP_POC_EVIDENCE_DIR ?? join(dirname(fileURLToPath(import.meta.url)), '.state'));
  const summary = summarize(await latestReport(directory));
  process.stdout.write(JSON.stringify(summary, null, 2) + '\n');
  process.exitCode = summary.acceptanceQualified ? 0 : 1;
}
