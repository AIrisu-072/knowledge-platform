import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
async function experiment(failAt) {
  const source = await readFile(new URL('./run.mjs', import.meta.url),'utf8');
  const start = source.indexOf('  async function findingReadExperiment()');
  assert.notEqual(start,-1,'bounded read-only experiment must exist');
  const body = source.slice(start,source.indexOf("  await report.stage('persistence'",start));
  const events = [], logs = [];
  const construct = new Function('browser','stopProcess','start','mkdir','join','directory','contextPath','console',`let salesProcess='sales-2', officeProcess='office-2'; ${body} return findingReadExperiment;`);
  const call = construct(async(phase,context,base)=>{events.push(['read',phase,base]); if(events.filter(x=>x[0]==='read').length===failAt) throw Error('stop');}, async owned=>events.push(['stop',owned]), async(profile,generation)=>{events.push(['start',profile,generation]);return `${profile}-${generation}`;},async(base,options)=>{assert.equal(options.mode,0o700);events.push(['mkdir',base]);},(...parts)=>parts.join('/'),'owned','context',{error:value=>logs.push(value)});
  let failed = false; try {await call();} catch(error) {assert.equal(error.message,'stop');failed=true;}
  return {events,logs,failed};
}
test('two bounded cycles read before and after restart with unique private output bases and existing readiness starts', async () => {
  const {events,logs,failed} = await experiment(); assert.equal(failed,false);
  assert.deepEqual(events.filter(x=>x[0]==='read').map(x=>x[2]),['owned/finding-read-1-before','owned/finding-read-1-after','owned/finding-read-2-before','owned/finding-read-2-after']);
  assert.deepEqual(events.filter(x=>x[0]==='start'),[['start','sales-01',3],['start','office-01',3],['start','sales-01',4],['start','office-01',4]]);
  assert.equal(events.filter(x=>x[0]==='stop').length,4);
  assert.ok(logs.some(line=>line==='Organization Finding read experiment: no_reproduction'));
});
test('the first failing read stops immediately without retry, further restart or NoRepro claim', async () => {
  for (let failAt=1;failAt<=4;failAt++) {
    const {events,logs,failed} = await experiment(failAt); assert.equal(failed,true);
    assert.equal(events.filter(x=>x[0]==='read').length,failAt);
    assert.equal(events.at(-1)[0],'read'); assert.ok(!logs.some(line=>line.includes('no_reproduction')));
  }
});
test('probe source uses only the accepted GET oracles and no write, retry, page navigation, or recording', async () => {
  const probe = await readFile(new URL('../../apps/document-web/e2e-organization/finding-read-diagnostic/persistence.spec.ts',import.meta.url),'utf8');
  for (const oracle of ['assertSessions','captureFinal','assertEvidenceState','assertAgentState']) assert.match(probe,new RegExp(`await ${oracle}\\(`));
  assert.match(probe,/expect\(actual\)\.toEqual\(state\.final\)/u);
  assert.doesNotMatch(probe,/\.(?:post|patch|put|delete|goto|reload)\s*\(|setTimeout|waitForTimeout|retry|seed|saveState/u);
  for (const recording of ["screenshot: 'off'","trace: 'off'","video: 'off'"]) assert.ok(probe.includes(recording));
});
test('browser selects the exact original or probe file and keeps each private JSON/failure reader base separate', async () => {
  const source = await readFile(new URL('./run.mjs',import.meta.url),'utf8');
  const body = source.slice(source.indexOf('  async function browser('),source.indexOf("  await report.stage('journey'"));
  const calls = [], readBases = []; let failure;
  const construct = new Function('run','beginFindingDiagnosticWindow','processes','readBrowserFailureDiagnostics','console','join','directory','contextPath','process',body+'return browser;');
  const browser = construct(async(_name,_command,args,env)=>{calls.push({args,env});if(failure)throw failure;},()=>()=>({correlation:'none',events:[]}),[],async(base,phase)=>{readBases.push({base,phase});return{failure:{httpStatus:503,readEndpoint:'finding'}};},{error:()=>undefined},(...parts)=>parts.join('/'),'owned','context',{env:{}});
  await browser('persistence'); await browser('persistence','context','owned/finding-read-1-before');
  const original='/repo/apps/document-web/e2e-organization/persistence.spec.ts';
  const nested='/repo/apps/document-web/e2e-organization/finding-read-diagnostic/persistence.spec.ts';
  assert.ok(new RegExp(calls[0].args.at(-1)).test(original)); assert.ok(!new RegExp(calls[0].args.at(-1)).test(nested));
  assert.ok(new RegExp(calls[1].args.at(-1)).test(nested)); assert.ok(!new RegExp(calls[1].args.at(-1)).test(original));
  assert.equal(calls[0].env.KP_ORGANIZATION_BROWSER_OUTPUT,'owned/browser-persistence');
  assert.equal(calls[1].env.PLAYWRIGHT_JSON_OUTPUT_FILE,'owned/finding-read-1-before/browser-persistence/results.json');
  failure=Error('original'); await assert.rejects(browser('persistence','context','owned/finding-read-1-after'),value=>value===failure);
  assert.deepEqual(readBases,[{base:'owned/finding-read-1-after',phase:'persistence'}]);
});
test('each reused support oracle is GET-only, with no request replay or state persistence', async () => {
  const source = await readFile(new URL('../../apps/document-web/e2e-organization/support.ts',import.meta.url),'utf8');
  for (const name of ['get','assertSessions','captureFinal','assertEvidenceState','assertAgentState','assertHidden']) {
    const start=source.indexOf(`export async function ${name}`); assert.notEqual(start,-1);
    const end=source.indexOf('\nexport ',start+1); const body=source.slice(start,end<0?source.length:end);
    assert.doesNotMatch(body,/request\.(?:post|patch|put|delete|fetch)\s*\(|saveState\s*\(|writeFile\s*\(|setTimeout|waitForTimeout/u);
  }
});
