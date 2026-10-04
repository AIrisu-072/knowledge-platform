// This finite, owner-operated D2 review is separate from the frozen PR43 gate.
import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

export const REVIEW = Object.freeze({
  repository: 'AIrisu-072/knowledge-platform', repositoryId: 1369120817, owner: 'AIrisu-072',
  branch: 'design/organization-client-v0-ui',
  baseBranch: 'design/organization-client-v0', baseSha: '13c1292c806c9179be0a444ef2b4be8234e00bf4',
  // Source-only D2 Draft allocated by the parent; capture still requires every exact-head gate.
  pr: 48,
  start: Date.parse('2026-10-02T13:19:00Z'), expires: Date.parse('2026-10-03T13:19:00Z'),
});
const disabled = () => ({ enabled: false });
function repositoryMatches(repo, policy, minimal = false) {
  return repo?.id === policy.repositoryId && repo.url === `https://api.github.com/repos/${policy.repository}`
    && (minimal ? repo.name === 'knowledge-platform' : repo.full_name === policy.repository && repo.fork === false);
}
export function currentReviewSubject(pr, head, policy = REVIEW) {
  return pr?.number === policy.pr && pr.state === 'open' && pr.head?.sha === head
    && pr.head.ref === policy.branch && repositoryMatches(pr.head.repo, policy)
    && pr.base?.ref === policy.baseBranch && pr.base.sha === policy.baseSha && repositoryMatches(pr.base.repo, policy);
}
function requireRunAssociation(run, gate, policy) {
  // Verified against actual PR48 CI run37016358837. These association repos are
  // minimal {id,url,name}, NOT the full repository objects used by the PR API.
  assert.ok(Array.isArray(run.pull_requests) && run.pull_requests.length === 1, 'prerequisites');
  const pr = run.pull_requests[0];
  assert.ok(pr?.number === policy.pr && pr.url === `https://api.github.com/repos/${policy.repository}/pulls/${policy.pr}`
    && pr.head?.sha === gate.head && pr.head.ref === policy.branch && repositoryMatches(pr.head.repo, policy, true)
    && pr.base?.sha === policy.baseSha && pr.base.ref === policy.baseBranch && repositoryMatches(pr.base.repo, policy, true)
    && repositoryMatches(run.repository, policy) && repositoryMatches(run.head_repository, policy), 'prerequisites');
}

export function reviewGate(input, policy = REVIEW) {
  if (!input || typeof input !== 'object' || !Number.isSafeInteger(policy.pr) || policy.pr < 1
    || !Number.isSafeInteger(policy.start) || !Number.isSafeInteger(policy.expires) || policy.expires <= policy.start) return disabled();
  const { eventName, event, repository, runAttempt, runId, localHead, gitDirty, now } = input;
  if (eventName !== 'pull_request' || event?.action !== 'labeled' || repository !== policy.repository
    || event.repository?.full_name !== policy.repository || event.number !== policy.pr
    || event.sender?.login !== policy.owner || runAttempt !== '1' || typeof runId !== 'string' || !/^[1-9][0-9]{0,19}$/.test(runId)
    || !Number.isSafeInteger(now) || now < policy.start || now >= policy.expires || gitDirty !== false) return disabled();
  const pr = event.pull_request, head = pr?.head?.sha;
  if (!currentReviewSubject(pr, head, policy)
    || typeof head !== 'string' || !/^[a-f0-9]{40}$/.test(head) || localHead !== head
    || event.label?.name !== `d2-visual-${head}`) return disabled();
  return { enabled: true, head, runId, pr: policy.pr, branch: policy.branch };
}
export function readGitHubInput() {
  const file = process.env.GITHUB_EVENT_PATH;
  assert.ok(file && statSync(file).isFile() && statSync(file).size <= 2 * 1024 * 1024, 'event');
  const git = args => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 1024 * 1024 }).trim();
  return { eventName: process.env.GITHUB_EVENT_NAME, event: JSON.parse(readFileSync(file, 'utf8')),
    repository: process.env.GITHUB_REPOSITORY, runAttempt: process.env.GITHUB_RUN_ATTEMPT, runId: process.env.GITHUB_RUN_ID,
    localHead: git(['rev-parse', 'HEAD']), gitDirty: Boolean(git(['status', '--porcelain'])), now: Date.now() };
}
function matchingRun(runs, gate, path, policy) {
  assert.ok(Array.isArray(runs) && runs.length <= 100, 'prerequisites');
  // CI also starts on labeled events; its concurrent run is a different subject.
  // A completed successful exact-head normal CI is required, never the current run.
  // The parent must inspect its freshness before applying the exact-head label.
  // GitHub's documented path may include @ref; immutable head/repository/branch
  // checks below remain the authority, not that display suffix.
  let candidates = runs.filter(run => typeof run.path === 'string' && run.path.split('@')[0] === path && run.head_sha === gate.head && run.event === 'pull_request'
    && run.head_branch === gate.branch && String(run.id) !== gate.runId);
  // Count every same-head/path/branch source run BEFORE association validation.
  // A wrong-PR run or prior capture must not disappear to permit replay.
  if (path === '.github/workflows/organization-d2.yml') assert.equal(candidates.length, 1, 'prerequisites');
  else candidates = candidates.filter(run => run.status === 'completed' && run.conclusion === 'success' && Number.isSafeInteger(run.id) && BigInt(run.id) < BigInt(gate.runId));
  candidates.sort((a, b) => b.id - a.id);
  const run = candidates[0];
  assert.ok(run && Number.isSafeInteger(run.id) && run.id > 0 && BigInt(run.id) < BigInt(gate.runId)
    && run.status === 'completed' && run.conclusion === 'success' && run.run_attempt === 1
    && run.head_repository?.full_name === policy.repository && run.head_repository.fork === false
    && Date.parse(run.created_at) >= policy.start && Date.parse(run.created_at) < policy.expires, 'prerequisites');
  requireRunAssociation(run, gate, policy);
  return run;
}
export function qualifyingRuns({ gate, ciRuns, sourceRuns, dsiPocRuns, dsiSandboxRuns, ciJobs, sourceJobs, dsiPocJobs, dsiSandboxJobs, policy = REVIEW }) {
  assert.ok(gate.enabled, 'prerequisites');
  const ci = matchingRun(ciRuns, gate, '.github/workflows/ci.yml', policy);
  const normal = matchingRun(sourceRuns, gate, '.github/workflows/organization-d2.yml', policy);
  const dsiPoc = matchingRun(dsiPocRuns, gate, '.github/workflows/dsi-poc.yml', policy);
  const dsiSandbox = matchingRun(dsiSandboxRuns, gate, '.github/workflows/dsi-sandbox-preflight.yml', policy);
  assert.equal(new Set([ci.id, normal.id, dsiPoc.id, dsiSandbox.id]).size, 4, 'prerequisites');
  for (const name of ['policy', 'security', 'required-check']) assert.ok(ciJobs.filter(job => job.name === name && job.conclusion === 'success').length === 1, 'prerequisites');
  assert.ok(sourceJobs.filter(job => job.name === 'qualify-source' && job.conclusion === 'success').length === 1, 'prerequisites');
  assert.ok(sourceJobs.filter(job => job.name === 'capture-review' && job.conclusion === 'skipped').length === 1, 'prerequisites');
  assert.ok(dsiPocJobs.filter(job => job.name === 'qualification' && job.conclusion === 'success').length === 1, 'prerequisites');
  assert.ok(dsiSandboxJobs.filter(job => job.name === 'preflight' && job.conclusion === 'success').length === 1, 'prerequisites');
  return { ci: ci.id, normal: normal.id, dsiPoc: dsiPoc.id, dsiSandbox: dsiSandbox.id };
}
async function api(path) {
  // Public repository read only. No tokens, new permission, arbitrary URL or redirects.
  const response = await fetch(`https://api.github.com/repos/${REVIEW.repository}/${path}`, {
    redirect: 'error', signal: AbortSignal.timeout(20_000), headers: { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
  });
  assert.ok(response.ok && response.body, 'prerequisites');
  let size = 0; const chunks = [];
  for await (const chunk of response.body) { size += chunk.length; assert.ok(size <= 2 * 1024 * 1024, 'prerequisites'); chunks.push(chunk); }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
export async function requireHostedPrerequisites(gate) {
  const query = `runs?event=pull_request&head_sha=${gate.head}&per_page=100`;
  const subjects = [
    ['ci', 'ci.yml'], ['source', 'organization-d2.yml'],
    ['dsiPoc', 'dsi-poc.yml'], ['dsiSandbox', 'dsi-sandbox-preflight.yml'],
  ];
  const inputs = { gate };
  for (const [key, workflow] of subjects) {
    const data = await api(`actions/workflows/${workflow}/${query}`);
    // No unseen page may hide a replay or a different prerequisite subject.
    assert.ok(data.total_count === data.workflow_runs?.length, 'prerequisites');
    const run = matchingRun(data.workflow_runs, gate, `.github/workflows/${workflow}`, REVIEW);
    const jobs = await api(`actions/runs/${run.id}/attempts/1/jobs?per_page=100`);
    assert.ok(jobs.total_count === jobs.jobs?.length, 'prerequisites');
    inputs[`${key}Runs`] = data.workflow_runs; inputs[`${key}Jobs`] = jobs.jobs;
  }
  return qualifyingRuns(inputs);
}

export async function requireLiveReview(gate) {
  assert.ok(gate.enabled && Date.now() >= REVIEW.start && Date.now() < REVIEW.expires, 'gate');
  const pr = await api(`pulls/${REVIEW.pr}`);
  assert.ok(currentReviewSubject(pr, gate.head)
    && pr.labels?.some(label => label.name === `d2-visual-${gate.head}`), 'gate');
}
