import test from 'node:test';
import assert from 'node:assert/strict';
import { reviewGate, qualifyingRuns, requireHostedPrerequisites, requireLiveReview, REVIEW } from '../review-gate.mjs';
const head = 'a'.repeat(40), repository = 'AIrisu-072/knowledge-platform';
const baseBranch = 'design/organization-client-v0', baseSha = '13c1292c806c9179be0a444ef2b4be8234e00bf4';
const repo = () => ({ id: 1369120817, name: 'knowledge-platform', url: `https://api.github.com/repos/${repository}`, full_name: repository, fork: false });
function association() { const minimalRepo = { id: 1369120817, name: 'knowledge-platform', url: `https://api.github.com/repos/${repository}` }; return { number: 48, id: 4717301798, url: `https://api.github.com/repos/${repository}/pulls/48`, head: { ref: policy.branch, sha: head, repo: { ...minimalRepo } }, base: { ref: baseBranch, sha: baseSha, repo: { ...minimalRepo } } }; }
function dsi() { return { dsiPocRuns: [run(110, '.github/workflows/dsi-poc.yml')], dsiSandboxRuns: [run(120, '.github/workflows/dsi-sandbox-preflight.yml')], dsiPocJobs: [{ name: 'qualification', conclusion: 'success' }, { name: 'qualification-macos (macos-15)', conclusion: 'skipped' }], dsiSandboxJobs: [{ name: 'preflight', conclusion: 'success' }] }; }
const policy = { ...REVIEW, pr: 48, branch: 'design/organization-client-v0-ui' };
function input() { return { eventName: 'pull_request', repository, runAttempt: '1', runId: '300', localHead: head, gitDirty: false, now: Date.parse('2026-10-02T14:00:00Z'), event: { action: 'labeled', number: 48, repository: { full_name: repository }, sender: { login: 'AIrisu-072' }, label: { name: `d2-visual-${head}` }, pull_request: { number: 48, state: 'open', head: { sha: head, ref: policy.branch, repo: repo() }, base: { ref: baseBranch, sha: baseSha, repo: repo() } } } }; }
test('review gate accepts only current owned exact-head first-attempt label', () => { assert.equal(reviewGate(input(), policy).enabled, true); });
for (const [name, mutate] of [
  ['push', x => x.eventName = 'push'], ['synchronize', x => x.event.action = 'synchronize'], ['rerun', x => x.runAttempt = '2'],
  ['fork', x => x.event.pull_request.head.repo.fork = true], ['other repository', x => x.repository = 'other/repo'],
  ['wrong PR', x => x.event.number++], ['untrusted actor', x => x.event.sender.login = 'someone'],
  ['wrong label', x => x.event.label.name = `d2-visual-${'b'.repeat(40)}`], ['dirty checkout', x => x.gitDirty = true],
  ['wrong local head', x => x.localHead = 'b'.repeat(40)], ['expired', x => x.now = Date.parse('2026-10-03T13:19:00Z')],
  ['before approval', x => x.now = Date.parse('2026-10-02T13:18:59Z')], ['invalid run', x => x.runId = '../upload'],
  ['other branch', x => x.event.pull_request.head.ref = 'main'], ['closed', x => x.event.pull_request.state = 'closed'],
]) test(`review gate rejects ${name}`, () => { const x = input(); mutate(x); assert.equal(reviewGate(x, policy).enabled, false); });
test('unset review PR cannot activate; malformed inputs fail closed', () => { assert.equal(reviewGate(input(), { ...policy, pr: 0 }).enabled, false); for (const x of [null, {}, [], 'true']) assert.equal(reviewGate(x, policy).enabled, false); });
function run(id, path) { return { id, name: path === '.github/workflows/ci.yml' ? 'CI' : 'Organization D2 Source Qualification', path, event: 'pull_request', status: 'completed', conclusion: 'success', head_sha: head, head_branch: policy.branch, run_attempt: 1, repository: repo(), head_repository: repo(), pull_requests: [association()], created_at: '2026-10-02T13:30:00Z' }; }
test('hosted prerequisites bind successful CI and distinct normal source subject to exact head', () => {
  const ci = run(100, '.github/workflows/ci.yml'), normal = run(200, '.github/workflows/organization-d2.yml');
  const args = { ...dsi(), gate: reviewGate(input(), policy), ciRuns: [ci], sourceRuns: [normal], ciJobs: [{ name: 'policy', conclusion: 'success' }, { name: 'security', conclusion: 'success' }, { name: 'required-check', conclusion: 'success' }], sourceJobs: [{ name: 'qualify-source', conclusion: 'success' }, { name: 'capture-review', conclusion: 'skipped' }], policy };
  assert.deepEqual(qualifyingRuns(args), { ci: 100, normal: 200, dsiPoc: 110, dsiSandbox: 120 });
  for (const changed of [ { ciRuns: [{ ...ci, head_sha: 'b'.repeat(40) }] }, { sourceRuns: [{ ...normal, conclusion: 'failure' }] }, { sourceRuns: [{ ...normal, run_attempt: 2 }] }, { ciJobs: [{ name: 'policy', conclusion: 'success' }] }, { sourceJobs: [{ name: 'qualify-source', conclusion: 'skipped' }, { name: 'capture-review', conclusion: 'success' }] } ]) assert.throws(() => qualifyingRuns({ ...args, ...changed }));
});
test('a second prior source run blocks replay even after another ordinary same-head run', () => {
  const gate = reviewGate(input(), policy), ci = run(100, '.github/workflows/ci.yml'), normal = run(200, '.github/workflows/organization-d2.yml');
  assert.throws(() => qualifyingRuns({ ...dsi(), gate, policy, ciRuns: [ci], sourceRuns: [normal, { ...normal, id: 199 }], ciJobs: [{ name: 'policy', conclusion: 'success' }, { name: 'security', conclusion: 'success' }, { name: 'required-check', conclusion: 'success' }], sourceJobs: [{ name: 'qualify-source', conclusion: 'success' }, { name: 'capture-review', conclusion: 'skipped' }] }));
});
test('a concurrent labeled CI cannot replace or hide the prior completed exact-head CI', () => {
  const ci = run(100, '.github/workflows/ci.yml'), normal = run(200, '.github/workflows/organization-d2.yml');
  const result = qualifyingRuns({ ...dsi(), gate: reviewGate(input(), policy), policy, ciRuns: [{ ...ci, id: 301, status: 'in_progress', conclusion: null }, ci], sourceRuns: [normal], ciJobs: [{ name: 'policy', conclusion: 'success' }, { name: 'security', conclusion: 'success' }, { name: 'required-check', conclusion: 'success' }], sourceJobs: [{ name: 'qualify-source', conclusion: 'success' }, { name: 'capture-review', conclusion: 'skipped' }] });
  assert.equal(result.ci, 100);
});
test('non-string run identities and malformed bounds cannot turn on capture', () => {
  for (const runId of [300, null, undefined, ['300']]) assert.equal(reviewGate({ ...input(), runId }, policy).enabled, false);
  for (const expires of [NaN, Infinity, undefined]) assert.equal(reviewGate(input(), { ...policy, expires }).enabled, false);
});
test('GitHub workflow path@ref metadata retains the exact workflow file and source binding', () => {
  const ci = run(100, '.github/workflows/ci.yml'), normal = run(200, '.github/workflows/organization-d2.yml');
  const args = { ...dsi(), gate: reviewGate(input(), policy), policy, ciRuns: [{ ...ci, path: `${ci.path}@refs/pull/48/merge` }], sourceRuns: [{ ...normal, path: `${normal.path}@${policy.branch}` }], ciJobs: [{ name: 'policy', conclusion: 'success' }, { name: 'security', conclusion: 'success' }, { name: 'required-check', conclusion: 'success' }], sourceJobs: [{ name: 'qualify-source', conclusion: 'success' }, { name: 'capture-review', conclusion: 'skipped' }] };
  assert.deepEqual(qualifyingRuns(args), { ci: 100, normal: 200, dsiPoc: 110, dsiSandbox: 120 });
  assert.throws(() => qualifyingRuns({ ...args, ciRuns: [{ ...ci, path: '.github/workflows/other.yml@refs/pull/48/merge' }] }));
});
test('deployed review policy binds allocated D2 PR48 without widening current-review guards', () => {
  assert.equal(REVIEW.pr, 48); assert.equal(REVIEW.branch, 'design/organization-client-v0-ui');
  assert.equal(REVIEW.owner, 'AIrisu-072'); assert.equal(REVIEW.start, Date.parse('2026-10-02T13:19:00Z'));
  assert.equal(REVIEW.expires, Date.parse('2026-10-03T13:19:00Z'));
  assert.equal(reviewGate(input()).enabled, true);
  const other = input(); other.event.number = 47; other.event.pull_request.number = 47;
  assert.equal(reviewGate(other).enabled, false);
  const normal = input(); normal.event.action = 'synchronize'; assert.equal(reviewGate(normal).enabled, false);
});
function prerequisiteInput() { return { ...dsi(), gate: reviewGate(input(), policy), policy, ciRuns: [run(100, '.github/workflows/ci.yml')], sourceRuns: [run(200, '.github/workflows/organization-d2.yml')], ciJobs: [{ name: 'policy', conclusion: 'success' }, { name: 'security', conclusion: 'success' }, { name: 'required-check', conclusion: 'success' }], sourceJobs: [{ name: 'qualify-source', conclusion: 'success' }, { name: 'capture-review', conclusion: 'skipped' }] }; }
const badAssociation = [
  ['wrong PR', r => { r.pull_requests[0].number = 49; }],
  ['missing association', r => { delete r.pull_requests; }],
  ['empty association', r => { r.pull_requests = []; }],
  ['ambiguous association', r => { r.pull_requests.push(structuredClone(r.pull_requests[0])); }],
  ['wrong associated head', r => { r.pull_requests[0].head.sha = 'b'.repeat(40); }],
  ['wrong associated branch', r => { r.pull_requests[0].head.ref = 'other'; }],
  ['wrong head repository', r => { r.pull_requests[0].head.repo.url = 'https://api.github.com/repos/other/repository'; }],
  ['wrong repository ID', r => { r.pull_requests[0].head.repo.id++; }],
  ['wrong base repository', r => { r.pull_requests[0].base.repo.id++; }],
  ['wrong base branch', r => { r.pull_requests[0].base.ref = 'main'; }],
  ['wrong base SHA', r => { r.pull_requests[0].base.sha = 'b'.repeat(40); }],
  ['wrong PR URL', r => { r.pull_requests[0].url = `https://api.github.com/repos/${repository}/pulls/49`; }],
];
for (const key of ['ciRuns', 'sourceRuns', 'dsiPocRuns', 'dsiSandboxRuns']) for (const [name, mutate] of badAssociation) test(`${key} rejects ${name}`, () => {
  const args = prerequisiteInput(); mutate(args[key][0]); assert.throws(() => qualifyingRuns(args));
});
test('wrong-PR prior source run cannot be filtered out to permit capture replay', () => {
  const args = prerequisiteInput(), wrong = run(199, '.github/workflows/organization-d2.yml'); wrong.pull_requests[0].number = 49;
  args.sourceRuns.push(wrong); assert.throws(() => qualifyingRuns(args));
});
for (const key of ['dsiPocRuns', 'dsiSandboxRuns']) for (const status of ['missing', 'failure', 'pending']) test(`${key} ${status} blocks all capture qualification`, () => {
  const args = prerequisiteInput(); if (status === 'missing') args[key] = []; else Object.assign(args[key][0], status === 'failure' ? { conclusion: 'failure' } : { status: 'in_progress', conclusion: null });
  assert.throws(() => qualifyingRuns(args));
});
for (const key of ['dsiPocJobs', 'dsiSandboxJobs']) for (const conclusion of ['failure', 'skipped', null]) test(`${key} mandatory job ${conclusion} cannot be waived by a successful run`, () => {
  const args = prerequisiteInput(); args[key][0].conclusion = conclusion; assert.throws(() => qualifyingRuns(args));
});
for (const [name, mutate] of [['retargeted branch', p => { p.base.ref = 'main'; }], ['changed base commit', p => { p.base.sha = 'b'.repeat(40); }], ['changed base repo', p => { p.base.repo.id++; }]]) test(`event denies ${name}`, () => {
  const event = input(); mutate(event.event.pull_request); assert.equal(reviewGate(event, policy).enabled, false);
});
for (const [name, mutate] of [['retargeted branch', p => { p.base.ref = 'main'; }], ['changed base commit', p => { p.base.sha = 'b'.repeat(40); }], ['changed base repo', p => { p.base.repo.id++; }]]) test(`live PR denies ${name}`, async () => {
  const originalFetch = globalThis.fetch, originalNow = Date.now, pr = input().event.pull_request;
  pr.labels = [{ name: `d2-visual-${head}` }]; mutate(pr);
  globalThis.fetch = async url => { assert.equal(url, `https://api.github.com/repos/${repository}/pulls/48`); return new Response(JSON.stringify(pr)); };
  Date.now = () => Date.parse('2026-10-02T14:00:00Z');
  try { await assert.rejects(requireLiveReview(reviewGate(input(), policy))); } finally { globalThis.fetch = originalFetch; Date.now = originalNow; }
});
test('verified real GitHub run uses minimal association repos without invented full_name fields', async () => {
  const { readFile } = await import('node:fs/promises');
  const fixture = JSON.parse(await readFile(new URL('./fixtures/pr48-run-subject.json', import.meta.url), 'utf8'));
  assert.equal(fixture.pull_requests[0].head.repo.full_name, undefined);
  const args = prerequisiteInput(); args.gate.head = fixture.head_sha; args.gate.runId = '40000000000';
  for (const key of ['ciRuns', 'sourceRuns', 'dsiPocRuns', 'dsiSandboxRuns']) {
    args[key][0].head_sha = fixture.head_sha; args[key][0].pull_requests = structuredClone(fixture.pull_requests);
  }
  args.ciRuns[0] = { ...args.ciRuns[0], ...fixture };
  assert.deepEqual(qualifyingRuns(args), { ci: 37016358837, normal: 200, dsiPoc: 110, dsiSandbox: 120 });
});
for (const key of ['dsiPocJobs', 'dsiSandboxJobs']) test(`${key} requires the mandatory job to exist`, () => {
  const args = prerequisiteInput(); args[key] = []; assert.throws(() => qualifyingRuns(args));
});
test('hosted API orchestration obtains all four exact subjects and rejects incomplete pagination', async () => {
  const originalFetch = globalThis.fetch, args = prerequisiteInput(), calls = [];
  let incomplete = false;
  const workflows = { 'ci.yml': 'ci', 'organization-d2.yml': 'source', 'dsi-poc.yml': 'dsiPoc', 'dsi-sandbox-preflight.yml': 'dsiSandbox' };
  const ids = { 100: 'ci', 200: 'source', 110: 'dsiPoc', 120: 'dsiSandbox' };
  globalThis.fetch = async input => {
    const url = new URL(input); assert.equal(url.origin, 'https://api.github.com'); calls.push(url.pathname);
    const workflow = url.pathname.match(/\/actions\/workflows\/([^/]+)\/runs$/);
    if (workflow) { const runs = args[`${workflows[workflow[1]]}Runs`]; assert.equal(url.searchParams.get('head_sha'), head); return new Response(JSON.stringify({ total_count: runs.length + (incomplete ? 1 : 0), workflow_runs: runs })); }
    const id = url.pathname.match(/\/actions\/runs\/(\d+)\/attempts\/1\/jobs$/)?.[1];
    const jobs = args[`${ids[id]}Jobs`]; assert.ok(jobs); return new Response(JSON.stringify({ total_count: jobs.length, jobs }));
  };
  try {
    assert.deepEqual(await requireHostedPrerequisites(args.gate), { ci: 100, normal: 200, dsiPoc: 110, dsiSandbox: 120 });
    assert.equal(calls.length, 8); assert.ok(calls.some(path => path.includes('dsi-poc.yml')) && calls.some(path => path.includes('dsi-sandbox-preflight.yml')));
    incomplete = true; await assert.rejects(requireHostedPrerequisites(args.gate));
  } finally { globalThis.fetch = originalFetch; }
});
