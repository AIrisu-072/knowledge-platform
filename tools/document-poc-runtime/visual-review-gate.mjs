// One deliberate PR43 review event only. No network, tokens or standing capture.
import { appendFileSync, readFileSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY = 'AIrisu-072/knowledge-platform';
const BRANCH = 'feat/document-poc-acceptance-v0-e1';
const START = Date.parse('2026-10-02T04:31:00Z');
const EXPIRES = Date.parse('2026-10-03T04:31:00Z');
const disabled = () => ({ enabled: false });

export function visualReviewGate(input) {
  if (!input || typeof input !== 'object') return disabled();
  const { eventName, event, repository, runAttempt, runId, localHead, gitDirty, now } = input;
  if (eventName !== 'pull_request' || event?.action !== 'labeled' || repository !== REPOSITORY
    || event.repository?.full_name !== REPOSITORY || event.number !== 43
    || runAttempt !== '1' || typeof runId !== 'string' || !/^[1-9][0-9]{0,19}$/.test(runId)
    || !Number.isSafeInteger(now) || now < START || now >= EXPIRES || gitDirty !== false) return disabled();
  const pr = event.pull_request, head = pr?.head?.sha;
  if (pr?.number !== 43 || pr.state !== 'open' || pr.base?.repo?.full_name !== REPOSITORY
    || pr.head?.repo?.full_name !== REPOSITORY || pr.head.repo.fork !== false || pr.head.ref !== BRANCH
    || typeof head !== 'string' || !/^[a-f0-9]{40}$/.test(head) || localHead !== head
    || event.label?.name !== `c3-visual-${head}`) return disabled();
  return { enabled: true, head, runId };
}

function fromGitHub() {
  try {
    // Reject malformed/oversized event files without copying their contents to logs.
    const path = process.env.GITHUB_EVENT_PATH;
    if (!path || statSync(path).size > 2 * 1024 * 1024) return disabled();
    const event = JSON.parse(readFileSync(path, 'utf8'));
    const git = args => execFileSync('git', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 }).trim();
    return visualReviewGate({ eventName: process.env.GITHUB_EVENT_NAME, event,
      repository: process.env.GITHUB_REPOSITORY, runAttempt: process.env.GITHUB_RUN_ATTEMPT,
      runId: process.env.GITHUB_RUN_ID, localHead: git(['rev-parse', 'HEAD']),
      gitDirty: Boolean(git(['status', '--porcelain'])), now: Date.now() });
  } catch { return disabled(); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 2 || !process.env.GITHUB_OUTPUT) throw Error('Use the owned GitHub gate step');
  const gate = fromGitHub();
  appendFileSync(process.env.GITHUB_OUTPUT, `enabled=${gate.enabled}\n${gate.enabled ? `head=${gate.head}\nrun-id=${gate.runId}\n` : ''}`);
  console.log(`Current PR43 visual review gate: ${gate.enabled ? 'enabled' : 'disabled'}`);
}
