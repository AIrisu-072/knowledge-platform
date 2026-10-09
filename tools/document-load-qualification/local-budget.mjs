const MINUTE = 60_000;
const HARD_DURATION_MS = 80*60*MINUTE;
const SHUTDOWN_RESERVE_MS = 15*MINUTE;
const timestamp = value => Number.isSafeInteger(value) && Number.isFinite(new Date(value).getTime());
const fail = code => { throw Error(`local-budget-${code}`); };

/** Capture before preparation; later calls validate that same start, never extend it. */
export function createLocalRunBudget({startedAt=Date.now(),now=Date.now()}={}) {
  if (!timestamp(now)) fail('clock-invalid');
  if (!timestamp(startedAt) || startedAt > now || !timestamp(startedAt+HARD_DURATION_MS)) fail('start-invalid');
  const hardDeadline = startedAt+HARD_DURATION_MS;
  const workDeadline = hardDeadline-SHUTDOWN_RESERVE_MS;
  if (now >= workDeadline) fail('deadline-exhausted');
  return Object.freeze({runStartedAt:new Date(startedAt).toISOString(),
    hardDeadlineAt:new Date(hardDeadline).toISOString(),workDeadlineAt:new Date(workDeadline).toISOString()});
}
