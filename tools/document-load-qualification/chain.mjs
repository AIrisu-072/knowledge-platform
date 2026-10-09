import {mkdir} from 'node:fs/promises';
import {join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {runQualification} from './controller.mjs';

const MINUTE = 60_000;
const GiB = 1024 ** 3;

export function smallPlan(fingerprint, startedAt = Date.now()) {
  return {stage:'small', documentCount:2, fingerprint, safetyFactor:2,
    budgets:{diskReserveBytes:GiB, minAvailableMemoryBytes:512*1024**2, maxRssBytes:2*GiB, maxWallTimeMs:5*MINUTE},
    deadlineAt:new Date(startedAt + 5*MINUTE).toISOString()};
}

/** One fresh owned-runtime chain. The controller owns admission and both restarts. */
export async function runThousandChain({directory, runId, fingerprint, corpus, runtime, probeFactory, qualify = runQualification, now = Date.now}) {
  const result = {status:'FAILED'};
  try {
    const startedAt = now();
    const deadline = startedAt + 125*MINUTE;
    const common = {runId, corpus, runtime, probeFactory};
    const smallDirectory = join(directory, 'small');
    // Exclusive stage directories prevent importing or resuming a previous report.
    await mkdir(smallDirectory, {mode:0o700});
    result.small = await qualify({...common, directory:smallDirectory, plan:smallPlan(fingerprint, startedAt)});
    result.status = result.small.status;
    if (result.status !== 'SUCCEEDED') return result;

    const clockFailure = value => !Number.isSafeInteger(value) || value < startedAt
      ? 'chain-clock-invalid' : value >= deadline ? 'chain-deadline-exhausted' : undefined;
    const refuse = failureCode => ({...result, status:'NOT_ADMITTED', failureCode});
    let current = now();
    let failureCode = clockFailure(current);
    if (failureCode) return refuse(failureCode);

    const {humanPid, agentPid, ...currentDataset} = await runtime.identity();
    if (!result.small.restart?.after || !isDeepStrictEqual(currentDataset, result.small.restart.after)
      || !isDeepStrictEqual([humanPid,agentPid],result.small.restart.processes?.after)) {
      return refuse('interstage-runtime-identity-mismatch');
    }
    current = now();
    failureCode = clockFailure(current);
    if (failureCode) return refuse(failureCode);
    const plan = {...smallPlan(fingerprint, current), stage:1000, documentCount:1000};
    plan.budgets.maxWallTimeMs = 120*MINUTE;
    plan.deadlineAt = new Date(Math.min(current + 120*MINUTE, deadline)).toISOString();
    const thousandDirectory = join(directory, '1000');
    await mkdir(thousandDirectory, {mode:0o700});
    // Preserve the whole fresh report: unchanged admission validates its proof and
    // a new resource observation before the controller opens any mutation journal.
    result.thousand = await qualify({...common, directory:thousandDirectory, plan, previousReport:result.small});
    result.status = result.thousand.status;
    return result;
  } catch {
    return {...result, status:'FAILED', failureCode:'chain-prerequisite-failed'};
  }
}
