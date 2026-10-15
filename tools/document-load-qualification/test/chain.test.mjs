import test from 'node:test';
import assert from 'node:assert/strict';
import {access, mkdir, mkdtemp, readFile, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {runQualification} from '../controller.mjs';

const chain = await import('../chain.mjs').catch(() => ({}));
const MINUTE = 60_000;
const GiB = 1024 ** 3;
const fingerprint = {code:'a'.repeat(40), corpus:'b'.repeat(64), runtime:'c'.repeat(64)};
const runId = '0198eada-1234-7000-8000-000000000001';
const dataset = {runId, sourceHead:fingerprint.code, databaseIdentitySha256:'d'.repeat(64), storageIdentitySha256:'e'.repeat(64), runtimeSource:'built-in-this-run', containerId:'owned-test-container'};
const corpus = {hash:fingerprint.corpus, assets:[]};

// These are orchestration fixtures, never qualifying real-process evidence.
function report(plan, overrides = {}) {
  return {schemaVersion:1, runId, evidenceClass:'test-double', status:'SUCCEEDED', stage:plan.stage, documentCount:plan.documentCount, fingerprint:plan.fingerprint, plan,
    restart:{identityRetained:true, processesReplaced:true, before:{...dataset}, after:{...dataset}, processes:{before:[100,200],after:[101,201]}},
    evidence:{documentIds:Array.from({length:plan.documentCount}, (_, index) => `test-document-${index}`)},
    metrics:{totalElapsedMs:1, diskGrowthBytes:0, peakRssBytes:1}, productionSloClaim:false, ...overrides};
}
async function setup(t, overrides = {}) {
  const directory = await mkdtemp(join(tmpdir(), 'document-thousand-chain-'));
  t.after(() => rm(directory, {recursive:true, force:true}));
  const calls = [];
  const runtime = {evidenceClass:'test-double', identity:async () => ({...dataset, humanPid:101, agentPid:201})};
  return {directory, runId, fingerprint, corpus, runtime, probeFactory:() => ({}),
    qualify:async options => {calls.push(options); return report(options.plan);}, calls, ...overrides};
}

test('chain runs a fresh two-document stage then 1000 with the same inputs and separate journals', async t => {
  assert.equal(typeof chain.runThousandChain, 'function');
  const options = await setup(t);
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'SUCCEEDED');
  assert.deepEqual(options.calls.map(call => call.plan.stage), ['small',1000]);
  assert.deepEqual(options.calls.map(call => call.plan.documentCount), [2,1000]);
  assert.equal(options.calls[0].directory, join(options.directory, 'small'));
  assert.equal(options.calls[1].directory, join(options.directory, '1000'));
  assert.equal(options.calls[0].previousReport, undefined);
  assert.equal(options.calls[1].previousReport, result.small);
  assert.equal(result.thousand.evidenceClass, 'test-double');
  for (const call of options.calls) {
    assert.equal(call.runId, runId);
    assert.equal(call.plan.fingerprint, fingerprint);
    assert.equal(call.runtime, options.runtime);
    assert.equal(call.corpus, corpus);
    assert.equal(call.probeFactory, options.probeFactory);
    await access(call.directory);
  }
});

test('both plans keep the existing reserves and cap the chain at 125 minutes', async t => {
  assert.equal(typeof chain.smallPlan, 'function');
  const started = Date.now();
  let current = started;
  const options = await setup(t, {now:() => current});
  options.qualify = async options => {const value = report(options.plan); if (options.plan.stage === 'small') current += MINUTE; return value;};
  const result = await chain.runThousandChain(options);
  const small = chain.smallPlan(fingerprint, started);
  assert.equal(small.budgets.maxWallTimeMs, 5 * MINUTE);
  assert.equal(small.deadlineAt, new Date(started + 5 * MINUTE).toISOString());
  for (const stage of [result.small, result.thousand]) {
    const plan = stage.plan;
    assert.equal(plan.safetyFactor, 2);
    assert.equal(plan.budgets.diskReserveBytes, GiB);
    assert.equal(plan.budgets.minAvailableMemoryBytes, 512 * 1024 ** 2);
    assert.equal(plan.budgets.maxRssBytes, 2 * GiB);
  }
  assert.equal(result.thousand.plan.budgets.maxWallTimeMs, 120 * MINUTE);
  assert.equal(Date.parse(result.thousand.plan.deadlineAt), started + 121 * MINUTE);
});

for (const status of ['FAILED','ABORTED','NOT_ADMITTED','NOT_RUN','AWAITING_RESTART']) {
  test(`a ${status} small stage stops the chain without consulting a new runtime identity`, async t => {
    const options = await setup(t);
    options.runtime.identity = async () => {throw Error('must not inspect runtime after failed small stage');};
    options.qualify = async ({plan}) => report(plan, {status});
    const result = await chain.runThousandChain(options);
    assert.equal(result.status, status);
    assert.equal(result.small.status, status);
    assert.equal(result.thousand, undefined);
    await assert.rejects(access(join(options.directory,'1000')));
  });
}

for (const field of ['runId','sourceHead','databaseIdentitySha256','storageIdentitySha256','runtimeSource','containerId']) {
  test(`changed ${field} refuses 1000 before qualification`, async t => {
    const options = await setup(t);
    options.runtime.identity = async () => ({...dataset, [field]:'changed', humanPid:101, agentPid:201});
    const result = await chain.runThousandChain(options);
    assert.equal(result.status, 'NOT_ADMITTED');
    assert.equal(result.failureCode, 'interstage-runtime-identity-mismatch');
    assert.equal(result.small.status, 'SUCCEEDED');
    assert.equal(result.thousand, undefined);
    assert.deepEqual(options.calls.map(call => call.plan.stage), ['small']);
  });
}

test('interstage comparison checks current PIDs separately and retains every dataset field', async t => {
  const options = await setup(t);
  options.runtime.identity = async () => ({...dataset, humanPid:101, agentPid:201});
  assert.equal((await chain.runThousandChain(options)).status, 'SUCCEEDED');
  options.runtime.identity = async () => ({...dataset, unexpectedDatasetField:true, humanPid:101, agentPid:201});
  const second = await setup(t, {runtime:options.runtime});
  assert.equal((await chain.runThousandChain(second)).status, 'NOT_ADMITTED');
});

test('missing small restart proof cannot start 1000', async t => {
  const options = await setup(t, {qualify:async ({plan}) => report(plan, {restart:undefined})});
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'NOT_ADMITTED');
  assert.equal(result.failureCode, 'interstage-runtime-identity-mismatch');
  assert.equal(result.thousand, undefined);
});

test('a slow stage and identity read cannot extend the common deadline', async t => {
  const started = Date.now();
  let current = started;
  const options = await setup(t, {now:() => current});
  options.qualify = async ({plan}) => {if(plan.stage === 'small') current += 4 * MINUTE; return report(plan);};
  options.runtime.identity = async () => {current += 2 * MINUTE; return {...dataset,humanPid:101,agentPid:201};};
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'SUCCEEDED');
  assert.equal(Date.parse(result.thousand.plan.deadlineAt), started + 125 * MINUTE);
  assert.equal(result.thousand.plan.budgets.maxWallTimeMs, 120 * MINUTE);
});

test('an exhausted common deadline refuses 1000 even when small says success', async t => {
  const started = Date.now();
  let current = started;
  const options = await setup(t, {now:() => current});
  options.qualify = async ({plan}) => {current = started + 125 * MINUTE; return report(plan);};
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'NOT_ADMITTED');
  assert.equal(result.failureCode, 'chain-deadline-exhausted');
  assert.equal(result.small.status, 'SUCCEEDED');
  assert.equal(result.thousand, undefined);
});

test('clock regression fails closed instead of expanding a stage time budget', async t => {
  const started = Date.now();
  let current = started;
  const options = await setup(t, {now:() => current});
  options.qualify = async ({plan}) => {current = started - 1; return report(plan);};
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'NOT_ADMITTED');
  assert.equal(result.failureCode, 'chain-clock-invalid');
  assert.equal(result.thousand, undefined);
});

test('existing small evidence is never imported or resumed', async t => {
  const options = await setup(t);
  await mkdir(join(options.directory,'small'));
  await writeFile(join(options.directory,'small','report.json'), 'PRIVATE_SENTINEL');
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'FAILED');
  assert.equal(result.failureCode, 'chain-prerequisite-failed');
  assert.equal(options.calls.length, 0);
  assert.equal(await readFile(join(options.directory,'small','report.json'),'utf8'), 'PRIVATE_SENTINEL');
  assert.ok(!JSON.stringify(result).includes('PRIVATE_SENTINEL'));
});

test('1000 rejection and partial failure are preserved with the fresh small result', async t => {
  for (const status of ['NOT_ADMITTED','ABORTED','FAILED']) {
    const options = await setup(t, {qualify:async ({plan}) => report(plan,{status:plan.stage === 'small' ? 'SUCCEEDED' : status})});
    const result = await chain.runThousandChain(options);
    assert.equal(result.status, status);
    assert.equal(result.small.status, 'SUCCEEDED');
    assert.equal(result.thousand.status, status);
  }
});

test('unexpected runtime errors preserve the completed small result and use a fixed failure code', async t => {
  const options = await setup(t);
  options.runtime.identity = async () => {throw Error('PRIVATE_SENTINEL');};
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'FAILED');
  assert.equal(result.small.status, 'SUCCEEDED');
  assert.equal(result.failureCode, 'chain-prerequisite-failed');
  assert.ok(!JSON.stringify(result).includes('PRIVATE_SENTINEL'));
});

test('real controller admission rejects a test-double small before any 1000 mutation', async t => {
  const options = await setup(t);
  let restarts = 0;
  const mutations = [];
  options.runtime.observe = async () => ({observedAt:new Date().toISOString(), diskFreeBytes:10*GiB, storageDiskFreeBytes:10*GiB, databaseDiskFreeBytes:10*GiB,
    availableMemoryBytes:10*GiB, rssBytes:1, storageBytes:0, databaseBytes:0, rssCoverage:'process-tree',databaseFilesystemVerified:true});
  options.runtime.identity = async () => ({...dataset, humanPid:100+restarts, agentPid:200+restarts});
  options.runtime.restart = async () => {restarts++;};
  options.qualify = args => runQualification({...args, execute:async ({count}) => {mutations.push(count); return {documentIds:['test-a','test-b']};}, verify:async () => {}});
  const result = await chain.runThousandChain(options);
  assert.equal(result.status, 'NOT_ADMITTED');
  assert.equal(result.small.status, 'SUCCEEDED');
  assert.equal(result.small.evidenceClass, 'test-double');
  assert.equal(result.thousand.evidenceClass, 'test-double');
  assert.match(result.thousand.admission.reasons.join(';'), /real owned-process/);
  assert.deepEqual(mutations, [2]);
  assert.equal(restarts, 1);
  assert.equal(JSON.parse(await readFile(join(options.directory,'small','report.json'),'utf8')).stage, 'small');
  assert.equal(JSON.parse(await readFile(join(options.directory,'1000','report.json'),'utf8')).stage, 1000);
  await assert.rejects(access(join(options.directory,'1000','operations.jsonl')));
});
test('an unowned interstage HTTP process replacement is rejected before1000 mutations',async t=>{
 const options=await setup(t);options.runtime.identity=async()=>({...dataset,humanPid:999,agentPid:201});
 const result=await chain.runThousandChain(options);assert.equal(result.status,'NOT_ADMITTED');assert.equal(result.failureCode,'interstage-runtime-identity-mismatch');assert.equal(options.calls.length,1);
});
