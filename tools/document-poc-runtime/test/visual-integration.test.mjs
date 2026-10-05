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
  assert.match(config, /: \['persistence\.spec\.ts', 'metadata-editor\.spec\.ts', 'lifecycle-operations-persistence\.spec\.ts', 'document-schedule-cancellation\.spec\.ts', 'working-version-editor-persistence\.spec\.ts'\]/);
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

test('複数原本WORKINGのjourneyと再起動は同じrunnerで全recordingをoffに保つ', async () => {
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  assert.match(config, /testMatch: phase === 'journey' \? \[[^\]]*'working-version-editor\.spec\.ts'/);
  assert.match(config, /: \[[^\]]*'working-version-editor-persistence\.spec\.ts'/);
  for (const name of ['working-version-editor', 'working-version-editor-persistence']) {
    const source = await read(`../../../apps/document-web/e2e-runtime/${name}.spec.ts`);
    assert.match(source, /^test\.use\(\{ screenshot: 'off', trace: 'off', video: 'off' \}\);$/m);
    assert.equal((source.match(/^test\('/gm) ?? []).length, name === 'working-version-editor' ? 2 : 1);
    assert.doesNotMatch(source, /visualCheckpoint|screenshot\(|recordVideo|tracing|page\.route\(|route\.fulfill\(|route\.abort\(/);
  }
  const helper = await read('../../../apps/document-web/e2e-runtime/working-version-support.ts');
  assert.match(helper, /context\.statePath\}\.working-editor\.json/);
  assert.doesNotMatch(helper, /manifest\.documents|saveSnapshot\(/);
});

// A split integration must preserve both previously accepted journeys, without changing phases.
test('WORKING編集と予約取消を既存journey・再起動の正確な集合へ共存させる', async () => {
  const config = await read('../../../apps/document-web/playwright.runtime.config.ts');
  const match = config.match(/testMatch: phase === 'journey' \? (\[[^\]]*\]) : (\[[^\]]*\])/);
  assert.ok(match);
  const entries = value => [...value.matchAll(/'([^']+)'/g)].map(item => item[1]);
  assert.deepEqual(entries(match[1]), ['document-runtime.spec.ts', 'initial-registration.spec.ts', 'metadata-editor.spec.ts',
    'lifecycle-operations.spec.ts', 'document-schedule-cancellation.spec.ts', 'working-version-editor.spec.ts',
    'human-agent-consistency.spec.ts', 'worker-failure.spec.ts', 'timestamp-layout.spec.ts']);
  assert.deepEqual(entries(match[2]), ['persistence.spec.ts', 'metadata-editor.spec.ts', 'lifecycle-operations-persistence.spec.ts',
    'document-schedule-cancellation.spec.ts', 'working-version-editor-persistence.spec.ts']);
});

test('WORKING実応答喪失は専用pageだけをproxyへ通し結果不明から明示再送する', async () => {
  const source = await read('../../../apps/document-web/e2e-runtime/working-version-editor.spec.ts');
  assert.match(source, /withWorkingResponseLoss/);
  assert.match(source, /page: async \(\{ browser, loss \}/);
  assert.match(source, /browser\.newContext\([\s\S]*?proxy: \{ server: loss\.origin \}/);
  assert.match(source, /finally \{ await context\.close\(\); \}/);
  assert.doesNotMatch(source, /test\.use\(\{\s*proxy|PLAYWRIGHT_|launchOptions|page\.route\(|route\.fulfill\(|route\.abort\(/);
  const save = source.slice(source.indexOf('async function save('), source.indexOf('async function openWorking('));
  const stages = ['loss.arm(', 'loss.dropped()', '保存結果を確認できません', 'const committed =',
    'loss.allowRetry()', "name: '同じ内容で再試行'", 'await loss.assertRecovered()', '.toEqual(committed)'];
  let position = -1;
  for (const stage of stages) { const next = save.indexOf(stage); assert.ok(next > position, stage); position = next; }
  assert.match(save, /received: 2, dispatched: 2, dropped: 1, unexpected: 0/);
  assert.match(source, /loss\.allowPublish\(`/);
  assert.equal((source.match(/^test\('/gm) ?? []).length, 2);
});


test('WORKING喪失の有限到達段階は実操作の完了後だけ記録する', async () => {
  const source = await read('../../../apps/document-web/e2e-runtime/working-version-editor.spec.ts');
  const save = source.slice(source.indexOf('async function save('), source.indexOf('async function openWorking('));
  assert.match(save, /\.click\(\)\.then\(\(\) => completed\('gui-working-loss-save-clicked'\)\)/);
  const sequence = ['loss.arm(', "completed('gui-working-loss-armed')", 'await Promise.all([', 'loss.dropped()',
    "completed('gui-working-loss-save-clicked')", "completed('gui-working-loss-dropped')", "name: '保存結果を確認できません'", '.toBeVisible();',
    "completed('gui-working-loss-unknown-visible')", 'loss.allowRetry()', "completed('gui-working-loss-retry-armed')",
    'await loss.assertRecovered()', "completed('gui-working-loss-recovered')"];
  let position = -1;
  for (const token of sequence) { const next = save.indexOf(token, position + 1); assert.ok(next > position, token); position = next; }
});


test('WORKING body途中喪失は実headersと読取失敗を確認してから明示再送をarmする', async () => {
  const source = await read('../../../apps/document-web/e2e-runtime/working-version-editor.spec.ts');
  const save = source.slice(source.indexOf('async function save('), source.indexOf('async function openWorking('));
  const sequence = ['const initialResponsePromise = page.waitForResponse(', "const failedRequestPromise = page.waitForEvent('requestfailed',",
    'const [initialResponse, failedRequest, lost] = await Promise.all([', 'initialResponsePromise, failedRequestPromise, loss.dropped()', '.click().then(',
    'expect(initialResponse.status())', "initialResponse.headerValue('content-length')",
    'expect(failedRequest === initialResponse.request()).toBe(true)', 'expect(failedRequest.failure()).not.toBeNull()',
    "completed('gui-working-loss-headers-observed')", "name: '保存結果を確認できません'", 'loss.allowRetry()'];
  let position = -1;
  for (const token of sequence) { const next = save.indexOf(token, position + 1); assert.ok(next > position, token); position = next; }
  assert.doesNotMatch(save, /initialResponse\.(?:finished|body|json)\(/);
});
