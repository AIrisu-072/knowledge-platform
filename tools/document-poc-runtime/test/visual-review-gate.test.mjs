import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const moduleUrl = new URL('../visual-review-gate.mjs', import.meta.url);
const head = 'a'.repeat(40);
const repository = 'AIrisu-072/knowledge-platform';
const fixture = () => ({
  eventName: 'pull_request', repository, runAttempt: '1', runId: '123456',
  localHead: head, gitDirty: false, now: Date.parse('2026-10-02T08:00:00Z'),
  event: { action: 'labeled', number: 43, repository: { full_name: repository },
    label: { name: `c3-visual-${head}` }, pull_request: { number: 43, state: 'open',
      head: { sha: head, ref: 'feat/document-poc-acceptance-v0-e1', repo: { full_name: repository, fork: false } },
      base: { repo: { full_name: repository } },
    } },
});

test('only the deliberate current-review label enables one exact clean source head', async () => {
  assert.ok(existsSync(moduleUrl), 'A default-off reviewed event gate must exist before capture is activated');
  const { visualReviewGate } = await import(moduleUrl);
  assert.deepEqual(visualReviewGate(fixture()), { enabled: true, head, runId: '123456' });
});

test('ordinary events, wrong identities, malformed values, expiry and reruns never enable capture', async () => {
  assert.ok(existsSync(moduleUrl), 'A default-off reviewed event gate must exist before capture is activated');
  const { visualReviewGate } = await import(moduleUrl);
  const cases = [
    x => { x.event.action = 'synchronize'; }, x => { x.event.action = 'reopened'; },
    x => { x.event.action = 'opened'; }, x => { x.eventName = 'push'; },
    x => { x.eventName = 'pull_request_target'; }, x => { x.eventName = 'workflow_dispatch'; },
    x => { x.event.action = 'unlabeled'; },
    x => { delete x.event.label; x.event.pull_request.labels = [{ name: `c3-visual-${head}` }]; },
    x => { x.event.number = 42; }, x => { x.event.number = '43'; },
    x => { x.event.pull_request.number = 42; }, x => { x.event.pull_request.state = 'closed'; },
    x => { x.repository = 'other/repo'; }, x => { x.event.repository.full_name = 'other/repo'; },
    x => { x.event.pull_request.base.repo.full_name = 'other/repo'; },
    x => { x.event.pull_request.head.repo.full_name = 'other/repo'; },
    x => { x.event.pull_request.head.repo.fork = true; },
    x => { x.event.pull_request.head.ref = 'main'; },
    x => { x.event.pull_request.head.sha = 'b'.repeat(40); },
    x => { x.event.label.name = `c3-visual-${'b'.repeat(40)}`; },
    x => { x.event.label.name += '\n'; }, x => { x.event.pull_request.head.sha = 'A'.repeat(40); },
    x => { x.localHead = 'b'.repeat(40); }, x => { x.gitDirty = true; },
    x => { x.runAttempt = '2'; }, x => { x.runAttempt = 1; },
    x => { x.runId = '0'; }, x => { x.runId = '123\nenabled=true'; },
    x => { x.now = Date.parse('2026-10-03T04:31:00Z'); },
    x => { x.now = Date.parse('2026-10-02T04:30:59Z'); },
    x => { x.now = NaN; }, x => { x.now = '2026-10-02T08:00:00Z'; },
    x => { x.event = null; }, x => { x.event.pull_request.head = null; },
  ];
  for (const [index, change] of cases.entries()) {
    const input = fixture(); change(input);
    assert.deepEqual(visualReviewGate(input), { enabled: false }, `negative case ${index}`);
  }
  for (const value of [undefined, null, {}, [], 'true']) assert.deepEqual(visualReviewGate(value), { enabled: false });
});

test('GitHub wrapper fails closed on malformed event files and never logs their contents', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'visual-gate-'));
  const eventPath = join(directory, 'event.json'), output = join(directory, 'output');
  try {
    for (const payload of ['invalid-json-SYNTHETIC_PRIVATE_PROBE', ' '.repeat(2 * 1024 * 1024 + 1)]) {
      await writeFile(eventPath, payload); await writeFile(output, '');
      const result = spawnSync(process.execPath, [fileURLToPath(moduleUrl)], {
        cwd: directory, env: { PATH: '', GITHUB_EVENT_PATH: eventPath, GITHUB_OUTPUT: output,
          GITHUB_EVENT_NAME: 'pull_request', GITHUB_REPOSITORY: repository,
          GITHUB_RUN_ID: '123456', GITHUB_RUN_ATTEMPT: '1' }, encoding: 'utf8', timeout: 5000,
      });
      assert.equal(result.status, 0); assert.equal(await readFile(output, 'utf8'), 'enabled=false\n');
      assert.equal(result.stdout, 'Current PR43 visual review gate: disabled\n'); assert.equal(result.stderr, '');
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});
