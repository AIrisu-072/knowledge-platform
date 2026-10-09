import test from 'node:test';
import assert from 'node:assert/strict';
const budget=await import('../job-budget.mjs').catch(()=>({}));
const MINUTE=60_000,started=Date.parse('2026-10-09T00:00:00.000Z');
const repository='AIrisu-072/knowledge-platform',workflowPath='.github/workflows/document-load-ten-thousand.yml';
const env={GITHUB_REPOSITORY:repository,GITHUB_REPOSITORY_ID:'1369120817',GITHUB_RUN_ID:'37700000001',GITHUB_RUN_ATTEMPT:'2',GITHUB_SHA:'a'.repeat(40),GITHUB_REF:'refs/heads/main',GITHUB_EVENT_NAME:'workflow_dispatch',GITHUB_WORKFLOW_REF:`${repository}/${workflowPath}@refs/heads/main`};
const repo=()=>({id:1369120817,full_name:repository,private:false});
const response=()=>({id:37700000001,run_attempt:2,head_sha:env.GITHUB_SHA,path:workflowPath,event:'workflow_dispatch',head_branch:'main',status:'in_progress',run_started_at:new Date(started).toISOString(),repository:repo(),head_repository:repo()});
const validate=(body=response(),changes={})=>{
  assert.equal(typeof budget.validateRunBudget,'function');
  return budget.validateRunBudget(body,{env,now:started+30*MINUTE,...changes});
};
test('public current-attempt metadata anchors fixed 360-minute hard and 345-minute work cutoffs',()=>{
  assert.deepEqual(validate(),{runStartedAt:new Date(started).toISOString(),hardDeadlineAt:new Date(started+360*MINUTE).toISOString(),workDeadlineAt:new Date(started+345*MINUTE).toISOString()});
  assert.deepEqual(validate(response(),{now:started+300*MINUTE}),validate());
});
test('documented main-qualified workflow path binds to the same exact workflow',()=>{
 assert.deepEqual(validate({...response(),path:`${workflowPath}@main`}),validate());
 for(const path of [`${workflowPath}@other`,`${workflowPath}@refs/heads/other`,`${workflowPath}@${env.GITHUB_SHA}`,`${workflowPath}@main/extra`])assert.throws(()=>validate({...response(),path}),/job-budget-origin-invalid/);
});
for(const [key,value]of Object.entries({GITHUB_REPOSITORY:'other/repository',GITHUB_REPOSITORY_ID:'1',GITHUB_RUN_ID:'bad',GITHUB_RUN_ATTEMPT:'0',GITHUB_SHA:'x'.repeat(40),GITHUB_REF:'refs/heads/feature',GITHUB_EVENT_NAME:'pull_request',GITHUB_WORKFLOW_REF:`${repository}/.github/workflows/other.yml@refs/heads/main`}))test(`invalid or missing expected ${key} is rejected`,()=>{
  for(const replacement of [value,undefined])assert.throws(()=>validate(response(),{env:{...env,[key]:replacement}}),/job-budget-origin-invalid/);
});
for(const [key,value]of Object.entries({id:37700000002,run_attempt:1,head_sha:'b'.repeat(40),path:'.github/workflows/document-load-thousand.yml',event:'push',head_branch:'other',status:'completed'}))test(`mismatched or missing response ${key} is rejected`,()=>{
  for(const replacement of [value,undefined])assert.throws(()=>validate({...response(),[key]:replacement}),/job-budget-origin-invalid/);
});
for(const key of ['repository','head_repository'])for(const change of [{id:1},{full_name:'other/repo'},{private:true},{private:undefined}])test(`public source origin ${key} ${Object.keys(change)[0]} must match`,()=>{
  assert.throws(()=>validate({...response(),[key]:{...repo(),...change}}),/job-budget-origin-invalid/);
});
for(const value of [undefined,null,'tomorrow','2026-10-09T00:00:00','2026-02-30T00:00:00Z',new Date(started+31*MINUTE).toISOString()])test(`invalid, ambiguous or future start ${value} is rejected`,()=>{
  assert.throws(()=>validate({...response(),run_started_at:value}),/job-budget-start-invalid/);
});
for(const now of [NaN,Infinity,started-1,started+345*MINUTE,started+360*MINUTE])test(`invalid clock or exhausted work cutoff ${now} is rejected`,()=>{
  assert.throws(()=>validate(response(),{now}),/job-budget-(clock-invalid|start-invalid|deadline-exhausted)/);
});
test('malformed bodies fail closed without reflecting private response content',()=>{
  for(const body of [undefined,null,[],{},'PRIVATE_SENTINEL'])assert.throws(()=>budget.validateRunBudget(body,{env,now:started+30*MINUTE}),error=>/job-budget-origin-invalid/.test(error.message)&&!error.message.includes('PRIVATE_SENTINEL'));
});
async function fetchBudget(changes={}){
  assert.equal(typeof budget.fetchCurrentRunBudget,'function');
  return budget.fetchCurrentRunBudget({env,now:()=>started+30*MINUTE,fetchImpl:async()=>new Response(JSON.stringify(response()),{headers:{'content-type':'application/json'}}),...changes});
}
test('fetch uses only the exact public current attempt endpoint with no auth and returns only approved timestamps',async()=>{
  const calls=[];const result=await fetchBudget({env:{...env,GITHUB_TOKEN:'PRIVATE_SENTINEL',GH_TOKEN:'PRIVATE_SENTINEL',GITHUB_API_URL:'https://attacker.invalid'},fetchImpl:async(url,options)=>{calls.push({url,options});return new Response(JSON.stringify({...response(),secret:'PRIVATE_SENTINEL'}));}});
  assert.equal(calls[0].url,`https://api.github.com/repos/${repository}/actions/runs/37700000001/attempts/2`);
  assert.equal(calls[0].options.redirect,'error');assert.equal(calls[0].options.credentials,'omit');assert.ok(calls[0].options.signal instanceof AbortSignal);
  assert.ok(!JSON.stringify(calls).includes('PRIVATE_SENTINEL'));assert.deepEqual(result,validate());
});
test('bad environment is rejected before fetching',async()=>{
  let calls=0;await assert.rejects(fetchBudget({env:{...env,GITHUB_REPOSITORY:'other/repo'},fetchImpl:async()=>{calls++;throw Error('unexpected fetch');}}),/job-budget-origin-invalid/);assert.equal(calls,0);
});
for(const status of [403,404,429,500])test(`HTTP ${status} fails closed without a local-clock fallback`,async()=>{
  await assert.rejects(fetchBudget({fetchImpl:async()=>new Response('PRIVATE_SENTINEL',{status})}),/job-budget-fetch-failed/);
});
test('network, malformed JSON, redirect and oversized responses are sanitized failures',async()=>{
  for(const fetchImpl of [async()=>{throw Error('PRIVATE_SENTINEL');},async()=>new Response('PRIVATE_SENTINEL'),async()=>new Response('{}',{status:302,headers:{location:'https://attacker.invalid'}}),async()=>new Response('x'.repeat(256*1024+1)),async()=>new Response('{}',{headers:{'content-length':String(256*1024+1)}})]){
    await assert.rejects(fetchBudget({fetchImpl}),error=>error.message==='job-budget-fetch-failed');
  }
});
test('an unknown-length stream is stopped at the byte cap',async()=>{
  let cancelled=false;
  const stream=new ReadableStream({pull(controller){controller.enqueue(new Uint8Array(64*1024));},cancel(){cancelled=true;}});
  await assert.rejects(fetchBudget({fetchImpl:async()=>new Response(stream)}),/job-budget-fetch-failed/);assert.equal(cancelled,true);
});
test('fetch timeout also covers a stalled response body',async()=>{
  let cancelled=false;
  const body=new ReadableStream({start(){},cancel(){cancelled=true;}});
  await assert.rejects(fetchBudget({timeoutMs:20,fetchImpl:async()=>new Response(body)}),/job-budget-fetch-failed/);assert.equal(cancelled,true);
});
test('fetch timeout covers a fetch implementation that never settles',async()=>{
  await assert.rejects(fetchBudget({timeoutMs:20,fetchImpl:()=>new Promise(()=>{})}),/job-budget-fetch-failed/);
});
test('timeout cannot be disabled or expanded past ten seconds',async()=>{
  for(const timeoutMs of [0,-1,10001,Infinity,'20'])await assert.rejects(fetchBudget({timeoutMs}),/job-budget-fetch-failed/);
});
test('clock rollback during metadata fetch cannot enlarge the budget',async()=>{
  let clock=started+30*MINUTE;await assert.rejects(fetchBudget({now:()=>clock,fetchImpl:async()=>{clock--;return new Response(JSON.stringify(response()));}}),/job-budget-clock-invalid/);
});
test('preparation that consumes the remaining cutoff while metadata is fetched fails closed',async()=>{
  let clock=started+344*MINUTE;await assert.rejects(fetchBudget({now:()=>clock,fetchImpl:async()=>{clock=started+345*MINUTE;return new Response(JSON.stringify(response()));}}),/job-budget-deadline-exhausted/);
});
