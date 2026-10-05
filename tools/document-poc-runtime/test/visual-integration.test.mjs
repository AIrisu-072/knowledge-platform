import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const read = path => readFile(new URL(path, import.meta.url), 'utf8');
test('opt-in is wired only through owned harness, rejects prebuilt, and exports after final acceptance', async () => {
  const run = await read('../run.mjs');
  assert.match(run, /KP_POC_CAPTURE_VISUAL/);
  assert.match(run, /visualEnabled && args.includes\('--prebuilt'\)/);
  assert.ok(run.indexOf('exportVisualEvidence({') > run.indexOf('await report.finish()'));
  assert.match(run, /report\.data\.acceptanceQualified.*\n.*exportVisualEvidence/s);
  assert.match(run, /visual-checkpoints/);
});
test('runtime browser overrides Desktop Chrome viewport and never emits failure diagnostics in visual mode', async () => {
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  assert.match(config, /\.\.\.devices\['Desktop Chrome'\], viewport: \{ width: 1440, height: 900 \}, deviceScaleFactor: 1/);
  for (const kind of ['trace', 'screenshot', 'video']) assert.match(config, new RegExp(`${kind}: context.visualCapture \\? 'off'`));
});
test('初回登録だけを既存journeyへ追加し、成功時も失敗時も画像を記録しない', async () => {
  const source = await read('../../../apps/document-web/e2e-runtime/initial-registration.spec.ts');
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  assert.match(config, /testMatch: phase === 'journey' \? \[[^\]]*'initial-registration\.spec\.ts'/);
  assert.match(config, /retries: 0/);
  assert.match(source, /^test\.use\(\{ screenshot: 'off', trace: 'off', video: 'off' \}\);$/m);
  assert.equal((source.match(/^test\('/gm) ?? []).length, 1);
  assert.match(source, /await startDiagnostics\(page\)/);
  assert.match(source, /await finishDiagnostics\(page\)/);
  assert.doesNotMatch(source, /visualCheckpoint|screenshot\(|recordVideo|tracing|page\.route\(|route\.fulfill\(|route\.abort\(/);
});
test('予約取消は同じjourneyと再起動phaseへ追加し、専用画像や外部artifactを生成しない', async () => {
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  assert.match(config, /testMatch: phase === 'journey' \? \[[^\]]*'document-schedule-cancellation\.spec\.ts'[^\]]*\] : \[[^\]]*'persistence\.spec\.ts'[^\]]*'document-schedule-cancellation\.spec\.ts'/);
  const source = await read('../../../apps/document-web/e2e-runtime/document-schedule-cancellation.spec.ts');
  assert.match(source, /^test\.use\(\{ screenshot: 'off', trace: 'off', video: 'off' \}\);$/m);
  assert.match(source, /process\.env\.KP_POC_RUNTIME_PHASE === 'journey'/);
  assert.match(source, /await startDiagnostics\(page\)/);
  assert.match(source, /await finishDiagnostics\(page\)/);
  assert.match(source, /\$\{context\.statePath\}\.schedule-cancellation\.json/);
  assert.match(source, /mode: 0o600/);
  assert.doesNotMatch(source, /saveSnapshot\(|visualCheckpoint|screenshot\(|recordVideo|tracing|page\.route\(|route\.fulfill\(|route\.abort\(/);
});

test('取下げ・公開終了の専用journeyと再起動だけを既存runnerへ追加し、画像を記録しない', async () => {
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  assert.match(config, /testMatch: phase === 'journey' \? \[[^\]]*'lifecycle-operations\.spec\.ts'/);
  assert.match(config, /: \['persistence\.spec\.ts', 'lifecycle-operations-persistence\.spec\.ts'[^\]]*\]/);
  assert.match(config, /retries: 0/);
  for (const name of ['lifecycle-operations', 'lifecycle-operations-persistence']) {
    const source = await read(`../../../apps/document-web/e2e-runtime/${name}.spec.ts`);
    assert.match(source, /^test\.use\(\{ screenshot: 'off', trace: 'off', video: 'off' \}\);$/m);
    assert.equal((source.match(/^test\('/gm) ?? []).length, name === 'lifecycle-operations' ? 2 : 1);
    assert.doesNotMatch(source, /visualCheckpoint|screenshot\(|recordVideo|tracing|page\.route\(|route\.fulfill\(|route\.abort\(/);
  }
  const helper = await read('../../../apps/document-web/e2e-runtime/lifecycle-support.ts');
  assert.match(helper, /context\.statePath\}\.lifecycle\.json/);
  assert.doesNotMatch(helper, /manifest\.documents|saveSnapshot\(/);
});
test('all fixed checkpoints occur exactly once in real runtime suites without route interception', async () => {
  const { VISUAL_CHECKPOINTS } = await import('../visual-evidence.mjs');
  const source = (await read('../../../apps/document-web/e2e-runtime/document-runtime.spec.ts')) + (await read('../../../apps/document-web/e2e-runtime/human-agent-consistency.spec.ts'));
  for (const { name } of VISUAL_CHECKPOINTS) assert.equal(source.split(`'${name}'`).length - 1, 1, name);
  assert.doesNotMatch(source, /page\.route\(|route\.fulfill\(|route\.abort\(/);
});
test('active upload uses only the approved event gate, 13 literal PNGs, immutable pin and one day', async () => {
  const workflow = await read('../../../.github/workflows/ci.yml');
  const { VISUAL_CHECKPOINTS } = await import('../visual-evidence.mjs');
  const paths = [...workflow.matchAll(/^            (tools\/document-poc-runtime\/\.state\/visual-export\/[^\n]+)$/gm)].map(match => match[1]);
  assert.deepEqual(paths, VISUAL_CHECKPOINTS.map(item => `tools/document-poc-runtime/.state/visual-export/${item.name}`));
  assert.match(workflow, /actions\/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/);
  assert.match(workflow, /retention-days: 1/); assert.match(workflow, /if-no-files-found: error/);
  assert.match(workflow, /if: \$\{\{ success\(\) && steps\.visual-review-gate\.outputs\.enabled == 'true' \}\}/);
  assert.match(workflow, /KP_POC_VISUAL_REVIEW_ENABLED: \$\{\{ steps\.visual-review-gate\.outputs\.enabled \}\}/);
  assert.match(workflow, /unset KP_POC_CAPTURE_VISUAL\n.*if \[\[ "\$KP_POC_VISUAL_REVIEW_ENABLED" == "true" \]\]; then\n.*export KP_POC_CAPTURE_VISUAL=true/);
  assert.match(workflow, /types: \[opened, synchronize, reopened, labeled\]/);
  assert.match(workflow, /permissions:\n  contents: read\n/);
  assert.equal((workflow.match(/permissions:/g) ?? []).length, 1);
  assert.match(workflow, /ref: \$\{\{ github\.event\.pull_request\.head\.sha \|\| github\.sha \}\}/);
  assert.doesNotMatch(workflow, /actions: write|secrets\.|pull_request_target|workflow_dispatch/);
  assert.ok(paths.every(path => !/[*!?]/.test(path)));
});
test('visual external-database rejection precedes evidence setup and database provisioning', async () => {
  const run = await read('../run.mjs');
  const guard = run.indexOf('if (visualEnabled) assertOwnedVisualDatabaseInput(process.env);');
  assert.ok(guard !== -1, 'Visual input guard must run before any harness setup');
  assert.ok(guard < run.indexOf('const base = resolve'));
  assert.ok(guard < run.indexOf("report.stage('database'"));
  assert.match(run, /const value = externalDatabase\(process.env\)/, 'Nonvisual external-database path is retained');
});
test('actual runner rejects visual external DB before creating evidence or launching any command', async () => {
  const { spawnSync } = await import('node:child_process');
  const { mkdtemp, lstat, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { fileURLToPath } = await import('node:url');
  const root = await mkdtemp(join(tmpdir(), 'visual-input-gate-'));
  const evidence = join(root, 'must-not-exist');
  try {
    // Empty PATH also prevents git/build/database commands if the early guard regresses.
    const result = spawnSync(process.execPath, [fileURLToPath(new URL('../run.mjs', import.meta.url))], {
      env: { PATH: '', KP_POC_CAPTURE_VISUAL: 'true', KP_POC_EVIDENCE_DIR: evidence,
        TEST_DATABASE_URL: 'postgres://127.0.0.1:5432/unverified', KP_POC_DISPOSABLE_DATABASE: 'true' },
      encoding: 'utf8', timeout: 5000,
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Visual capture requires a harness-owned disposable database/);
    await assert.rejects(lstat(evidence), { code: 'ENOENT' });
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('normal runtime checkpoints qualify readiness before the optional capture gate', async () => {
  const source = await read('../../../apps/document-web/e2e-runtime/visual-capture.ts');
  const ready = source.indexOf('await assertVisualReadiness({ page, name, humanOrigin: context.human });');
  const gate = source.indexOf('if (!context.visualCapture) return;');
  const capture = source.indexOf('await captureVisualCheckpoint(');
  assert.ok(ready > 0 && gate > ready && capture > gate, 'Ordinary runs must check settlement and page bounds without taking screenshots');
});
