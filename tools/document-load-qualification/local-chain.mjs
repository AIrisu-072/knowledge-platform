import {mkdir} from 'node:fs/promises';
import {join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {smallPlan} from './chain.mjs';
import {runQualification} from './controller.mjs';
import {createWorkClock} from './job-budget.mjs';
import {validateLocalResourceBudget} from './local-resource-budget.mjs';

const MINUTE = 60_000;
const WORK_DURATION_MS = (80*60-15)*MINUTE;
const STAGES = [
  ['small',2,5,'small'],
  [1000,1000,120,'thousand'],
  [10000,10000,330,'tenThousand'],
  [100000,100000,72*60,'hundredThousand'],
];

/** A fresh local chain; the caller fixes its cutoff before build and preparation. */
export async function runLocalScaleChain({directory,runId,fingerprint,corpus,runtime,probeFactory,workDeadlineAt,resourceBudget,now=createWorkClock(),qualify=runQualification}) {
  const result = {status:'FAILED'};
  const refuse = failureCode => ({...result,status:'NOT_ADMITTED',failureCode});
  try {
    let approvedResourceBudget;
    try {approvedResourceBudget=validateLocalResourceBudget(resourceBudget);} catch {return refuse('chain-resource-budget-invalid');}
    let current = now();
    if (!Number.isSafeInteger(current)) return refuse('chain-clock-invalid');
    const deadline = typeof workDeadlineAt === 'string' ? Date.parse(workDeadlineAt) : NaN;
    if (!Number.isSafeInteger(deadline) || new Date(deadline).toISOString() !== workDeadlineAt || deadline-current > WORK_DURATION_MS) {
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
    const common = {runId,corpus,runtime,probeFactory};
    let previousReport,baselineDataset;
    for (const [index,[stage,documentCount,minutes,key]] of STAGES.entries()) {
      let failureCode = clockFailure();
      if (failureCode) return refuse(failureCode);
      if (previousReport) {
        let history = previousReport;
        // Recheck each original report, including its origin, even when an older
        // report is still referenced by the newer report's history.
        for (let priorIndex=index-1;priorIndex>=0;priorIndex--) {
          const [priorStage,priorCount,,priorKey] = STAGES[priorIndex];
          const requiredHistory = priorIndex === 0 ? undefined : result[STAGES[priorIndex-1][3]];
          if (history !== result[priorKey] || history.runId !== runId || history.status !== 'SUCCEEDED'
            || history.stage !== priorStage || history.documentCount !== priorCount
            || !isDeepStrictEqual(history.fingerprint,fingerprint)
            || !isDeepStrictEqual(history.previousReport,requiredHistory)) {
            return refuse('interstage-report-identity-mismatch');
          }
          history = history.previousReport;
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
      if (approvedResourceBudget) Object.assign(plan.budgets,approvedResourceBudget);
      plan.budgets.maxWallTimeMs = Math.min(minutes*MINUTE,deadline-current);
      plan.deadlineAt = new Date(Math.min(current+minutes*MINUTE,deadline)).toISOString();
      const stageDirectory = join(directory,String(stage));
      await mkdir(stageDirectory,{mode:0o700});
      // The controller validates the complete fresh history and current capacity
      // before opening this stage's mutation journal. No receipt is imported.
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
