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

/** Fresh three-stage chain; the caller supplies the validated GitHub attempt cutoff. */
export async function runTenThousandChain({directory, runId, fingerprint, corpus, runtime, probeFactory, workDeadlineAt, qualify = runQualification, now = Date.now}) {
  const result = {status:'FAILED'};
  const refuse = failureCode => ({...result,status:'NOT_ADMITTED',failureCode});
  try {
    let current = now();
    if (!Number.isSafeInteger(current)) return refuse('chain-clock-invalid');
    const deadline = typeof workDeadlineAt === 'string' ? Date.parse(workDeadlineAt) : NaN;
    if (!Number.isSafeInteger(deadline) || new Date(deadline).toISOString() !== workDeadlineAt || deadline-current > 345*MINUTE) {
      return refuse('chain-deadline-invalid');
    }
    if (current >= deadline) return refuse('chain-deadline-exhausted');
    let lastClock = current;
    const clockFailure = () => {
      current = now();
      if (!Number.isSafeInteger(current) || current < lastClock) return 'chain-clock-invalid';
      lastClock = current;
      return current >= deadline ? 'chain-deadline-exhausted' : undefined;
    };
    const stages = [['small',2,5,'small'],[1000,1000,120,'thousand'],[10000,10000,330,'tenThousand']];
    const common = {runId,corpus,runtime,probeFactory};
    let previousReport, baselineDataset;
    for (const [index,[stage,documentCount,minutes,key]] of stages.entries()) {
      let failureCode = clockFailure();
      if (failureCode) return refuse(failureCode);
      if (previousReport) {
        const [priorStage,priorCount,,priorKey] = stages[index-1];
        const requiredHistory = index === 1 ? undefined : result[stages[index-2][3]];
        if (previousReport !== result[priorKey] || previousReport.runId !== runId
          || previousReport.stage !== priorStage || previousReport.documentCount !== priorCount
          || !isDeepStrictEqual(previousReport.fingerprint,fingerprint)
          || !isDeepStrictEqual(previousReport.previousReport,requiredHistory)) {
          return refuse('interstage-report-identity-mismatch');
        }
        const {humanPid,agentPid,...currentDataset} = await runtime.identity();
        const after = previousReport.restart?.after;
        if (!after || after.runId !== runId || after.sourceHead !== fingerprint.code
          || !isDeepStrictEqual(currentDataset,after)
          || !isDeepStrictEqual([humanPid,agentPid],previousReport.restart.processes?.after)
          || (baselineDataset && !isDeepStrictEqual(currentDataset,baselineDataset))) {
          return refuse('interstage-runtime-identity-mismatch');
        }
        baselineDataset ??= currentDataset;
        failureCode = clockFailure();
        if (failureCode) return refuse(failureCode);
      }
      const plan = {...smallPlan(fingerprint,current),stage,documentCount};
      plan.budgets.maxWallTimeMs = Math.min(minutes*MINUTE,deadline-current);
      plan.deadlineAt = new Date(Math.min(current+minutes*MINUTE,deadline)).toISOString();
      const stageDirectory = join(directory,String(stage));
      await mkdir(stageDirectory,{mode:0o700});
      // The unmodified controller re-observes capacity and validates the full
      // report chain before opening a mutation journal. No 100k stage is selected.
      result[key] = await qualify({...common,directory:stageDirectory,plan,previousReport});
      result.status = result[key].status;
      if (result.status !== 'SUCCEEDED') return result;
      previousReport = result[key];
    }
    const failureCode = clockFailure();
    return failureCode ? refuse(failureCode) : result;
  } catch {
    return {...result,status:'FAILED',failureCode:'chain-prerequisite-failed'};
  }
}
