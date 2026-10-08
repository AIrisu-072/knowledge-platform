import assert from 'node:assert/strict';
import test from 'node:test';
import { findingFailureDiagnostics, beginFindingDiagnosticWindow } from './finding-diagnostics.mjs';
const event = { operation: 'finding', phase: 'evidence', dependency: 'document', sql_class: 'none', failure: 'dependency_unavailable', elapsed_ms: 5100 };
const records = raw => findingFailureDiagnostics(raw).events;
const line = value => `KP_FINDING_DIAGNOSTIC ${JSON.stringify(value)}\n`;
test('exact fixed-prefix JSON projects only the closed failure schema', () => {
  assert.deepEqual(records(`noise\n${line(event)}${line({...event, operation:'finding_list'})}`), [event, {...event, operation:'finding_list'}]);
});
test('raw text, extra keys, malformed JSON and unknown enum values never leak', () => {
  const bad = [ {...event, error:'PRIVATE_SECRET'}, {...event, operation:'PRIVATE_SECRET'}, {...event, phase:'PRIVATE_SECRET'}, {...event, dependency:'PRIVATE_SECRET'}, {...event, sql_class:'PRIVATE_SECRET'}, {...event, failure:'PRIVATE_SECRET'}, {...event, elapsed_ms:'PRIVATE_SECRET'}, {...event, elapsed_ms:-1}, {...event, elapsed_ms:1.5}, {...event, elapsed_ms:4294967296} ];
  const raw = bad.map(line).join('') + 'KP_FINDING_DIAGNOSTIC {"PRIVATE_SECRET"\n' + 'other KP_FINDING_DIAGNOSTIC '+JSON.stringify(event)+'\n';
  assert.deepEqual(records(raw), []);
});
test('missing keys and nested structures are rejected, zero and uint32 maximum are accepted', () => {
  const missing = {...event}; delete missing.phase;
  assert.deepEqual(records(line(missing)+line({...event, phase:{secret:'PRIVATE_SECRET'}})), []);
  assert.deepEqual(records(line({...event, elapsed_ms:0})+line({...event, elapsed_ms:4294967295})).map(x=>x.elapsed_ms), [0,4294967295]);
});
test('multiple records are ambiguous and excess records are refused', () => {
  const raw = Array.from({length:5},(_,elapsed_ms)=>line({...event,elapsed_ms})).join('');
  assert.deepEqual(findingFailureDiagnostics(raw), { correlation:'overflow',events:[] });
  assert.equal(findingFailureDiagnostics(line(event)+line(event)).correlation,'ambiguous');
});
test('huge input, long lines, ANSI prefixes and incomplete lines do not expose arbitrary bytes', () => {
  const long = line({...event,error:'PRIVATE_SECRET'.repeat(10000)});
  assert.deepEqual(records(long+'\u001b[31m'+line(event)), []);
  assert.deepEqual(findingFailureDiagnostics('PRIVATE_SECRET'.repeat(10000)+'\n'+line(event)), {correlation:'overflow',events:[]});
  assert.deepEqual(records(line(event).trimEnd()), []);
  assert.deepEqual(records(null), []);
});
test('only current owned processes and phase-new output are considered', () => {
  let output = line({...event, elapsed_ms:1});
  const live = {child:{exitCode:null,signalCode:null},output:()=>output};
  const dead = {child:{exitCode:0},output:()=>line({...event,elapsed_ms:2})};
  const read = beginFindingDiagnosticWindow([dead,live]);
  assert.deepEqual(read(), {correlation:'none',events:[]});
  output += line({...event,elapsed_ms:3}); assert.deepEqual(read(), {correlation:'single',events:[{...event,elapsed_ms:3}]});
});
test('process output failures cannot replace the original runtime failure', () => {
  const read = beginFindingDiagnosticWindow([{child:{exitCode:null},output:()=>{throw Error('PRIVATE_SECRET');}}]);
  assert.deepEqual(read(), {correlation:'none',events:[]});
});

test('expected404 and non503 failures are excluded', () => {
  assert.deepEqual(records(line({...event,failure:'not_found'})+line({...event,failure:'forbidden'})+line({...event,failure:'validation'})),[]);
});

import { readFile } from 'node:fs/promises';
async function isolatedBrowser(run, observation, logs, counts) {
  const source = await readFile(new URL('./run.mjs', import.meta.url), 'utf8');
  const body = source.slice(source.indexOf('  async function browser('), source.indexOf("  await report.stage('journey'"));
  const construct = new Function('run','beginFindingDiagnosticWindow','processes','readBrowserFailureDiagnostics','console','join','directory','contextPath','process',body+'return browser;');
  return construct(run, () => { counts.window++; return () => {counts.read++; return {correlation:'single',events:[event]};}; }, [], async () => observation, {error:value=>logs.push(value)}, (...parts)=>parts.join('/'), 'PRIVATE_DIRECTORY','PRIVATE_CONTEXT',{env:{}});
}
test('runtime emits finding diagnostics only for the existing503 Finding failure, never for success or unrelated failure', async () => {
  for (const observation of [{failure:{httpStatus:503,readEndpoint:'finding'}},{failure:{httpStatus:404,readEndpoint:'finding'}},{failure:{httpStatus:503,readEndpoint:'evidence'}},{availability:'unavailable'}]) {
    const logs = [], counts = {window:0,read:0}; const original = Error('PRIVATE_RAW_ERROR');
    const browser = await isolatedBrowser(async()=>{throw original;},observation,logs,counts);
    await assert.rejects(browser('persistence'),error=>error===original);
    assert.equal(counts.window,1); assert.equal(counts.read,observation.failure?.httpStatus===503 && observation.failure.readEndpoint==='finding' ? 1 : 0);
    assert.ok(!logs.join('').includes('PRIVATE_RAW_ERROR')); assert.ok(!logs.join('').includes('PRIVATE_DIRECTORY'));
  }
  const logs = [], counts = {window:0,read:0}; const browser = await isolatedBrowser(async()=>undefined,{},logs,counts);
  await browser('journey'); assert.deepEqual(logs,[]); assert.equal(counts.read,0);
});

test('excess active processes and duplicate keys are refused rather than silently selecting a cause', () => {
  const live = {child:{exitCode:null},output:()=>line(event)};
  assert.deepEqual(beginFindingDiagnosticWindow(Array(7).fill(live))(),{correlation:'overflow',events:[]});
  const duplicate = line(event).replace('"operation":"finding"','"operation":"finding_list","operation":"finding"');
  assert.deepEqual(records(duplicate),[]);
});
