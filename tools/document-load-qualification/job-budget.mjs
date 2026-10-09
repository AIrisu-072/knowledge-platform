const MINUTE = 60_000;
const REPOSITORY = 'AIrisu-072/knowledge-platform';
const REPOSITORY_ID = 1369120817;
const WORKFLOW_PATH = '.github/workflows/document-load-ten-thousand.yml';
const MAX_BYTES = 256*1024;
const fail = code => { throw Error(`job-budget-${code}`); };
const positiveInteger = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));

/** Preparation elapsed time cannot be refunded by a backward wall-clock correction. */
export function createWorkClock({wallNow=Date.now,monotonicNow=()=>performance.now()}={}){
 const wallStart=wallNow(),monoStart=monotonicNow();let lastMono=monoStart,last=wallStart;
 if(!Number.isSafeInteger(wallStart)||!Number.isFinite(monoStart))fail('clock-invalid');
 return()=>{
  const wall=wallNow(),mono=monotonicNow();
  if(!Number.isSafeInteger(wall)||!Number.isFinite(mono)||mono<lastMono)fail('clock-invalid');
  const current=Math.max(last,wall,wallStart+Math.ceil(mono-monoStart));
  if(!Number.isSafeInteger(current))fail('clock-invalid');
  lastMono=mono;last=current;return current;
 };
}

function expectedOrigin(env) {
  if (!env || env.GITHUB_REPOSITORY !== REPOSITORY || env.GITHUB_REPOSITORY_ID !== String(REPOSITORY_ID)
    || !positiveInteger(env.GITHUB_RUN_ID) || !positiveInteger(env.GITHUB_RUN_ATTEMPT)
    || !/^[a-f0-9]{40}$/.test(env.GITHUB_SHA ?? '') || env.GITHUB_REF !== 'refs/heads/main'
    || env.GITHUB_EVENT_NAME !== 'workflow_dispatch'
    || env.GITHUB_WORKFLOW_REF !== `${REPOSITORY}/${WORKFLOW_PATH}@refs/heads/main`) fail('origin-invalid');
  return {runId:Number(env.GITHUB_RUN_ID),runAttempt:Number(env.GITHUB_RUN_ATTEMPT),head:env.GITHUB_SHA};
}

/** Validate only the current public run attempt; no local-start fallback exists. */
export function validateRunBudget(response, {env=process.env,now=Date.now()}={}) {
  const expected = expectedOrigin(env);
  const matchingRepository = value => value?.id === REPOSITORY_ID && value.full_name === REPOSITORY && value.private === false;
  if (!response || Array.isArray(response) || response.id !== expected.runId || response.run_attempt !== expected.runAttempt
    || response.head_sha !== expected.head || ![WORKFLOW_PATH,`${WORKFLOW_PATH}@main`].includes(response.path) || response.event !== 'workflow_dispatch'
    || response.head_branch !== 'main' || response.status !== 'in_progress'
    || !matchingRepository(response.repository) || !matchingRepository(response.head_repository)) fail('origin-invalid');
  if (!Number.isSafeInteger(now)) fail('clock-invalid');
  const start = response.run_started_at;
  const startedAt = typeof start === 'string' ? Date.parse(start) : NaN;
  if (!/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{3})?Z$/.test(start ?? '')
    || !Number.isSafeInteger(startedAt) || new Date(startedAt).toISOString().replace('.000Z','Z') !== start.replace('.000Z','Z')
    || startedAt > now) fail('start-invalid');
  const hardDeadline = startedAt+360*MINUTE, workDeadline = hardDeadline-15*MINUTE;
  if (now >= workDeadline) fail('deadline-exhausted');
  return Object.freeze({runStartedAt:new Date(startedAt).toISOString(),hardDeadlineAt:new Date(hardDeadline).toISOString(),workDeadlineAt:new Date(workDeadline).toISOString()});
}

/** Unauthenticated fixed-origin fetch, bounded across both headers and body. */
export async function fetchCurrentRunBudget({env=process.env,fetchImpl=globalThis.fetch,now=Date.now,timeoutMs=10_000}={}) {
  const expected = expectedOrigin(env), before = now();
  if (!Number.isSafeInteger(before)) fail('clock-invalid');
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10_000) fail('fetch-failed');
  const abort = new AbortController();
  let reader, timer, body;
  try {
    const timeout = new Promise((_,reject) => {
      timer = setTimeout(() => {abort.abort();reader?.cancel().catch(()=>{});reject(Error('job-budget-fetch-failed'));},timeoutMs);
    });
    body = await Promise.race([timeout,(async () => {
      const response = await fetchImpl(`https://api.github.com/repos/${REPOSITORY}/actions/runs/${expected.runId}/attempts/${expected.runAttempt}`,{
        headers:{Accept:'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28'},redirect:'error',credentials:'omit',signal:abort.signal,
      });
      if (response.status !== 200 || !response.body || Number(response.headers.get('content-length')) > MAX_BYTES) fail('fetch-failed');
      reader = response.body.getReader();
      const chunks=[];let length=0;
      for (;;) {
        const {done,value} = await reader.read();
        if (done) break;
        length += value.byteLength;
        if (length > MAX_BYTES) fail('fetch-failed');
        chunks.push(value);
      }
      return JSON.parse(Buffer.concat(chunks,length).toString('utf8'));
    })()]);
  } catch {
    fail('fetch-failed');
  } finally {
    clearTimeout(timer);abort.abort();reader?.cancel().catch(()=>{});
  }
  const after = now();
  if (!Number.isSafeInteger(after) || after < before) fail('clock-invalid');
  return validateRunBudget(body,{env,now:after});
}
