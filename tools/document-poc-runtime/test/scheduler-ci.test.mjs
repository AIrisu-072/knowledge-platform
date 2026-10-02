import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const workflow = await readFile(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8');
const tasks = await readFile(new URL('../../../mise.toml', import.meta.url), 'utf8');

function job(name) {
  const section = workflow.split(`\n  ${name}:\n`)[1];
  assert.ok(section, `required job ${name} must exist`);
  return section.split(/\n  [a-z][a-z0-9-]*:\n/)[0];
}

test('separate scheduler acceptance is an exact-head hosted gate with existing bounded tooling', () => {
  const scheduler = job('document-scheduler');
  assert.match(scheduler, /runs-on: ubuntu-24\.04/);
  assert.match(scheduler, /timeout-minutes: 45/);
  for (const [key, value] of [['CARGO_BUILD_JOBS', '2'], ['CARGO_PROFILE_DEV_DEBUG', '0'], ['CARGO_INCREMENTAL', '0']]) {
    assert.ok(scheduler.includes(`${key}: "${value}"`));
  }
  assert.match(scheduler, /actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1/);
  assert.match(scheduler, /persist-credentials: false/);
  assert.ok(scheduler.includes('ref: ${{ github.event.pull_request.head.sha || github.sha }}'));
  assert.match(scheduler, /jdx\/mise-action@c1ecc8f748cd28cdeabf76dab3cccde4ce692fe4/);
  assert.match(scheduler, /mise install rust/);
  assert.match(scheduler, /experiments\/document-semantic-inspection\/scripts\/install-pdfium\.sh/);
  assert.match(scheduler, /run: mise run document:scheduler:acceptance/);
  assert.doesNotMatch(scheduler, /continue-on-error|--privileged|RUST_MIN_STACK|secrets\./);
  assert.match(workflow, /permissions:\n  contents: read/);
});

test('required-check waits for scheduler and rejects any non-success result', () => {
  const required = job('required-check');
  assert.match(required, /needs:[\s\S]*?\n      - document-scheduler\n/);
  assert.ok(required.includes('DOCUMENT_SCHEDULER: ${{ needs.document-scheduler.result }}'));
  assert.match(required, /for result in [^\n]*"\$DOCUMENT_SCHEDULER"/);
  assert.match(required, /test "\$result" = success/);
});

test('scheduler task builds production DSI and explicitly executes the ignored real-process canary', () => {
  const task = tasks.split('[tasks."document:scheduler:acceptance"]')[1];
  assert.ok(task);
  assert.match(task, /cargo build --locked -p document-semantic-inspection-worker/);
  assert.match(task, /--test linux_container_canary -- --ignored --test-threads=1/);
  assert.doesNotMatch(task, /\|\| true|RUST_MIN_STACK/);
});
