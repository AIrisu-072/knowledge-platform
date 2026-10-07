#!/usr/bin/env node
// TEST-ONLY: drives the REAL desktop app (Tauri shell + WebKitGTK + the bundled
// production React build + the local Workspace broker) through its GUI:
// tauri-driver -> WebKitWebDriver, a private Xvfb display, xdotool/xclip for
// the native GTK folder dialog, and a real synthetic backend (PostgreSQL 18.6 +
// organization-server) reached only through the shell's /v1 forwarding.
// Local developer verification only; it is intentionally not a CI job.
// This is Linux evidence. It is NOT Windows/WebView2 evidence.
//
// Checks labelled "IPC" call the broker command from page script (as hostile
// or replaying page code would); every other check goes through the screen.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { spawn, execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { access, link, mkdir, mkdtemp, open, readFile, readdir, rename, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { freePort } from '../../../tools/document-poc-runtime/harness.mjs';
import { startBackend } from './backend.mjs';
import { Keys, Session } from './webdriver.mjs';
import { startXvfb, x11 } from './x11.mjs';

const run = promisify(execFile);
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const binary = resolve(process.env.KP_DESKTOP_BINARY ?? join(root, 'apps/desktop/src-tauri/target/debug/knowledge-platform-desktop'));
const tauriDriver = process.env.KP_TAURI_DRIVER ?? 'tauri-driver';
const webkitDriver = process.env.KP_WEBKIT_DRIVER ?? '/usr/bin/WebKitWebDriver';
const pdfium = process.env.KP_DSI_PDFIUM_RUNTIME_DIR;
const IDENTIFIER = 'dev.knowledgeplatform.desktop';
const SHARED_FOLDER = '00000000-0000-7000-8000-000000000001';
const MiB = 1024 * 1024;
const OUTCOME_UNKNOWN = '結果を確認できませんでした。「結果を確認」で同じ操作として確認してください。新しい操作としては送りません。';
const TOO_LARGE = 'サイズの上限（読み取り・作成とも8MiB）を超えています。';
// ASCII, so the first 1MiB of bytes is also the first 1MiB of characters.
const BIG = Array.from({ length: 40_000 }, (_, i) => `line ${String(i).padStart(5, '0')} of the synthetic large file\n`).join('');

const base = resolve(process.env.KP_DESKTOP_EVIDENCE_DIR ?? join(root, 'apps/desktop/e2e/.state'));
let directory, display, X, backend, sentinel, home, fixtures, stateRoot;
const drivers = [];
const backendStops = [];
const filtered = process.execArgv.some((arg) => /^--test-(name-pattern|skip-pattern|only)/.test(arg));
const report = {
  schemaVersion: 2,
  scope: 'Linux desktop GUI (Tauri 2.12.1 + WebKitGTK) against a real synthetic backend; not Windows/WebView2 evidence',
  status: 'running',
  qualifying: false,
  filtered,
  setupError: null,
  scenarios: [],
};
let current;

function scenario(name, body) {
  // Registered when defined, so a scenario that never ran stays visible as not-run.
  const entry = { name, status: 'not-run', checks: [], screenshots: [] };
  report.scenarios.push(entry);
  test(name, async () => {
    current = entry;
    entry.status = 'running';
    try {
      await body();
      entry.status = 'passed';
    } catch (error) {
      entry.status = 'failed';
      entry.error = String(error?.stack ?? error).slice(0, 2000);
      throw error;
    } finally {
      await saveReport();
    }
  });
}

function check(label, condition, detail) {
  current.checks.push({ label, ok: Boolean(condition), ...(detail === undefined ? {} : { detail }) });
  assert.ok(condition, `${label}${detail === undefined ? '' : `: ${JSON.stringify(detail)}`}`);
}

function overallStatus() {
  if (report.setupError || report.scenarios.some((item) => item.status === 'failed' || item.status === 'running')) return 'failed';
  if (report.scenarios.some((item) => item.status === 'not-run')) return 'incomplete';
  return 'passed';
}

async function saveReport() {
  if (directory) await writeFile(join(directory, 'report.json'), `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });
}

function prefix() {
  return String(report.scenarios.indexOf(current) + 1).padStart(2, '0');
}

async function shot(session, name) {
  const file = join(directory, `${prefix()}-${name}.png`);
  await writeFile(file, await session.screenshot());
  current.screenshots.push(file.slice(directory.length + 1));
}

async function rootShot(name) {
  const file = join(directory, `${prefix()}-${name}-screen.png`);
  await X.screenshot(file);
  current.screenshots.push(file.slice(directory.length + 1));
}

function appEnvironment({ apiOrigin, homeDir = home }) {
  const env = {
    PATH: process.env.PATH,
    DISPLAY: display.display,
    HOME: homeDir,
    XDG_CONFIG_HOME: join(homeDir, '.config'),
    XDG_DATA_HOME: join(homeDir, '.local/share'),
    XDG_CACHE_HOME: join(homeDir, '.cache'),
    XDG_RUNTIME_DIR: join(homeDir, '.runtime'),
    LANG: 'C.UTF-8',
    NO_AT_BRIDGE: '1',
  };
  if (apiOrigin) env.KNOWLEDGE_PLATFORM_API_ORIGIN = apiOrigin;
  return env;
}

async function startDriver(name, env) {
  const port = await freePort();
  let nativePort = await freePort();
  while (nativePort === port) nativePort = await freePort();
  const logFile = join(directory, `driver-${name}.log`);
  const log = createWriteStream(logFile, { mode: 0o600 });
  // Own process group: stop() also ends WebKitWebDriver and the app, even on Ctrl-C.
  const child = spawn(tauriDriver, ['--port', String(port), '--native-port', String(nativePort), '--native-driver', webkitDriver], { env, stdio: ['ignore', 'pipe', 'pipe'], detached: true });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  const url = `http://127.0.0.1:${port}`;
  const stop = () => { try { process.kill(-child.pid, 'SIGTERM'); } catch { /* already gone */ } };
  const driver = { name, url, logFile, stop };
  drivers.push(driver);
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error(`tauri-driver ${name} exited`);
    try { if ((await fetch(`${url}/status`)).ok) return driver; } catch { /* starting */ }
    await delay(100);
  }
  throw new Error(`tauri-driver ${name} did not start`);
}

async function launch(driver) {
  const session = await Session.create(driver.url, binary);
  await session.waitFor(async () => (await session.url()).startsWith('tauri://localhost/'), { message: 'the bundled app URL' });
  return session;
}

/** Waits until no app process holds the broker's instance lock. */
async function lockReleased() {
  const lock = join(stateRoot, '.lock');
  for (let i = 0; i < 100; i++) {
    try { await run('flock', ['-n', lock, 'true']); return; } catch { await delay(100); }
  }
  throw new Error('the app did not release its instance lock after exit');
}

/** Ends the session (the app exits) and waits for the broker's instance lock. */
async function quit(session) {
  // End-state evidence for every scenario (also shows the screen on failure).
  await shot(session, 'end').catch(() => undefined);
  await session.delete();
  await lockReleased();
}

async function appPids() {
  const pattern = `^${binary.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}( |$)`;
  const { stdout } = await run('pgrep', ['-f', pattern]).catch(() => ({ stdout: '' }));
  return stdout.split('\n').filter(Boolean).map(Number);
}

function invoke(session, command, request) {
  return session.executeAsync(`const done = arguments[arguments.length - 1];
    window.__TAURI__.core.invoke('local_workspace_runtime', { command: arguments[0], request: arguments[1] })
      .then((ok) => done({ ok }), (err) => done({ err }));`, [command, request]);
}

function invokeRaw(session, name, args) {
  return session.executeAsync(`const done = arguments[arguments.length - 1];
    window.__TAURI__.core.invoke(arguments[0], arguments[1]).then((ok) => done({ ok }), (err) => done({ err: String(err) }));`, [name, args]);
}

/**
 * Wraps the page's broker IPC to count commands and, when `lose` names one,
 * to drop the reply of its next call after the broker has completed it (a
 * lost IPC reply). Tauri's IPC entry points are non-writable, but on Linux its
 * custom-protocol IPC calls the global fetch at call time
 * (ipc://localhost/<command>), so the wrapper sits on window.fetch.
 */
function instrumentIpc(session, lose = null) {
  return session.execute(`const realFetch = window.__kpRealFetch ?? window.fetch;
    window.__kpRealFetch = realFetch;
    const state = window.__kpIpc = { calls: [], lost: [], lose: arguments[0] };
    const isBrokerIpc = (url) => ['ipc://localhost/local_workspace_runtime', 'http://ipc.localhost/local_workspace_runtime'].some((prefix) => url === prefix || url.startsWith(prefix + '?'));
    window.fetch = function fetch(input, init) {
      const url = typeof input === 'string' ? input : input instanceof Request ? input.url : String(input);
      if (!isBrokerIpc(url) || !init || typeof init.body !== 'string') return realFetch.call(this, input, init);
      let payload;
      try { payload = JSON.parse(init.body); } catch { return realFetch.call(this, input, init); }
      const command = payload && payload.command;
      const operationId = payload && payload.request && payload.request.operationId;
      state.calls.push({ command, operationId });
      const reply = realFetch.call(this, input, init);
      if (!state.lose || command !== state.lose) return reply;
      state.lose = null;
      return reply.then(async (response) => {
        state.lost.push({ command, operationId, delivered: response.headers.get('Tauri-Response') === 'ok' ? 'ok' : 'error' });
        await response.arrayBuffer();
        // What the page receives instead: an untyped IPC failure.
        return new Response(JSON.stringify('test: the IPC reply was lost'), { headers: { 'content-type': 'application/json', 'Tauri-Response': 'error' } });
      });
    };
    return window.fetch !== realFetch;`, [lose]);
}

const ipcState = (session) => session.execute('return window.__kpIpc;');
const callsOf = (state, command) => state.calls.filter((call) => call.command === command);

function pageFetch(session, url, init = {}) {
  return session.executeAsync(`const done = arguments[arguments.length - 1];
    fetch(arguments[0], arguments[1]).then(async (r) => done({ status: r.status, type: r.headers.get('content-type'),
      csp: r.headers.get('content-security-policy'), nosniff: r.headers.get('x-content-type-options'), cookie: r.headers.get('set-cookie'),
      body: (await r.text()).slice(0, 400) }),
      (e) => done({ error: String(e) }));`, [url, init]);
}

/** Sets a React-controlled field to `char` repeated `count` times, built inside the page. */
function fillRepeated(session, css, char, count) {
  return session.execute(`const element = document.querySelector(arguments[0]);
    const proto = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(element, arguments[1].repeat(arguments[2]));
    element.dispatchEvent(new Event('input', { bubbles: true }));
    return element.value.length;`, [css, char, count]);
}

async function clickText(session, css, text) {
  const element = await session.waitForText(css, text);
  // As a user would: scroll it into view first (for example the footer of a long dialog).
  await session.execute('arguments[0].scrollIntoView({ block: "center" });', [element.ref]);
  await element.click();
  return element;
}

async function notice(session, text, options) {
  return session.waitForText('p[role="status"]', text, options);
}

async function alertText(session, text, options) {
  return session.waitForText('[role="alert"]', text, options);
}

async function gotoLocalWorkspaces(session) {
  await clickText(session, 'header a', 'ローカルWorkspace');
  await session.waitForText('h1', 'ローカルWorkspace');
  await session.waitFor(async () => (await session.bodyText()).includes('管理フォルダー') || (await session.bodyText()).includes('Workspaceはまだありません'), { message: 'workspace list' });
}

/** Opens the screen with the named Workspace selected; returns its IPC record. */
async function openWorkspace(session, name) {
  await gotoLocalWorkspaces(session);
  await clickText(session, 'nav[aria-labelledby="local-workspace-list-title"] button', name);
  await session.waitForText('h2', name);
  return (await workspaces(session)).find((item) => item.name === name);
}

const FILE_FORM = 'form[aria-label="この場所にファイルを作成"]';

async function workspaces(session) {
  const reply = await invoke(session, 'workspace.list', null);
  assert.ok(reply.ok, JSON.stringify(reply));
  return reply.ok;
}

function contextOf(workspace) {
  return { workspaceId: workspace.workspaceId, effectiveContextRevision: workspace.effectiveContextRevision };
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

/** Paths under `dir` whose last segment is `name` (symlinks are not followed). */
async function findNamed(dir, name) {
  return (await readdir(dir, { recursive: true })).filter((path) => basename(path) === name);
}

async function newestMtime(paths) {
  let newest = 0;
  for (const path of paths) {
    const info = await stat(path).catch(() => undefined);
    if (!info) continue;
    if (!info.isDirectory()) { newest = Math.max(newest, info.mtimeMs); continue; }
    for (const child of await readdir(path, { recursive: true })) {
      const childInfo = await stat(join(path, child)).catch(() => undefined);
      if (childInfo?.isFile()) newest = Math.max(newest, childInfo.mtimeMs);
    }
  }
  return newest;
}

/** Path and sha256 of a tool, plus its Debian package version when it has one. */
async function toolIdentity(command, debianPackage) {
  const path = command.includes('/') ? command : (await run('sh', ['-c', 'command -v "$1"', 'sh', command]).catch(() => ({ stdout: '' }))).stdout.trim();
  const identity = { path: path || command };
  if (path) identity.sha256 = sha256(await readFile(path).catch(() => Buffer.alloc(0)));
  if (debianPackage) {
    const output = await run('dpkg-query', ['-W', '-f', '${Version}', debianPackage]).catch(() => ({ stdout: '' }));
    identity.package = `${debianPackage} ${output.stdout.trim() || 'unknown'}`;
  }
  return identity;
}

/** A loopback server the page must never reach (CSP, navigation, windows, frames). */
async function startSentinel() {
  const hits = [];
  const server = createServer((request, response) => {
    hits.push(`${request.method} ${request.url}`);
    // Permissive CORS on purpose: only CSP can stop a page fetch from reading this.
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'access-control-allow-origin': '*' });
    response.end('<!doctype html><title>sentinel</title><p>sentinel</p>');
  });
  await new Promise((resolveListen) => server.listen(0, '127.0.0.1', resolveListen));
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    hits,
    close: () => new Promise((resolveClose) => { server.closeAllConnections(); server.close(resolveClose); }),
  };
}

async function createFixtures() {
  const folder = join(fixtures, '資料フォルダー');
  const outside = join(fixtures, '外部の場所');
  const second = join(fixtures, '第二フォルダー');
  const third = join(fixtures, '第三フォルダー');
  await mkdir(join(folder, 'sub'), { recursive: true });
  await mkdir(outside, { recursive: true });
  await mkdir(join(second, '多数'), { recursive: true });
  await mkdir(third, { recursive: true });
  await writeFile(join(folder, 'readme.txt'), '合成データの説明文です。\n');
  await writeFile(join(folder, 'sub', 'nested.txt'), '下の階層の合成ファイルです。\n');
  await writeFile(join(folder, '大きいファイル.txt'), BIG);
  await writeFile(join(outside, 'secret.txt'), '外部の合成ファイル（読めてはいけない）\n');
  await writeFile(join(outside, 'linked-origin.txt'), 'ハードリンク元（合成）\n');
  await link(join(outside, 'linked-origin.txt'), join(folder, '二重リンク.txt'));
  await symlink(outside, join(folder, '外部へのリンク'));
  await writeFile(join(second, '第二の資料.txt'), '第二フォルダーの合成ファイルです。\n');
  for (let i = 0; i < 230; i++) await writeFile(join(second, '多数', `item-${String(i).padStart(3, '0')}.txt`), `合成の項目 ${i}\n`);
  await writeFile(join(third, 'third.txt'), '第三フォルダーの合成ファイルです。\n');
  return { folder, outside, second, third };
}

let main, folders, workspaceName, managedDir, docId, registeredId, replayOp;

async function cleanup() {
  for (const driver of drivers) driver.stop();
  for (const stop of backendStops.splice(0)) await stop().catch(() => undefined);
  display?.stop();
  await sentinel?.close().catch(() => undefined);
}

// node:test skips after() on SIGINT/SIGTERM; stop only what this run started.
for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143]]) {
  process.once(signal, () => {
    report.interrupted = signal;
    cleanup().finally(async () => {
      report.status = 'interrupted';
      report.qualifying = false;
      await saveReport().catch(() => undefined);
      process.exit(code);
    });
  });
}

before(async () => {
  await mkdir(base, { recursive: true, mode: 0o700 });
  directory = await mkdtemp(join(base, 'run-'));
  try {
    if (!pdfium) throw new Error('KP_DSI_PDFIUM_RUNTIME_DIR is required (experiments/document-semantic-inspection/scripts/install-pdfium.sh)');
    await access(binary);
    home = join(directory, 'home');
    fixtures = join(directory, 'fixtures');
    for (const path of ['.config', '.local/share', '.cache', '.runtime', 'Downloads']) await mkdir(join(home, path), { recursive: true, mode: 0o700 });
    await writeFile(join(home, '.config/user-dirs.dirs'), 'XDG_DOWNLOAD_DIR="$HOME/Downloads"\n');
    stateRoot = join(home, '.local/share', IDENTIFIER, 'workspace-runtime');
    folders = await createFixtures();
    const head = (await run('git', ['rev-parse', 'HEAD'], { cwd: root })).stdout.trim();
    const dirty = (await run('git', ['status', '--porcelain'], { cwd: root })).stdout.trim() !== '';
    const webkit = (await run('pkg-config', ['--modversion', 'webkit2gtk-4.1'])).stdout.trim();
    // The binary embeds the web build, so both must be newer than their inputs.
    const shell = join(root, 'apps/desktop/src-tauri');
    const dist = await newestMtime([join(root, 'apps/document-web/dist')]);
    const webInputs = await newestMtime([join(root, 'apps/document-web/src'), join(root, 'packages/document-api-client/src')]);
    const shellInputs = await newestMtime(['src', 'capabilities', 'permissions', 'build.rs', 'Cargo.toml', 'Cargo.lock', 'tauri.conf.json'].map((path) => join(shell, path)).concat(join(root, 'crates/local-workspace-runtime/src')));
    const binaryInfo = await stat(binary);
    const binaryStale = binaryInfo.mtimeMs < Math.max(shellInputs, dist) || dist < webInputs;
    report.environment = {
      gitHead: head, worktreeDirty: dirty, binary: { sha256: sha256(await readFile(binary)), builtAt: binaryInfo.mtime.toISOString(), staleAgainstSources: binaryStale },
      webkit2gtk: webkit, node: process.version, startedAt: new Date().toISOString(),
      tools: {
        // tauri-driver 2.1.0 has no --version; its identity is the binary hash.
        tauriDriver: await toolIdentity(tauriDriver),
        webkitWebDriver: await toolIdentity(webkitDriver, 'webkit2gtk-driver'),
        xvfb: await toolIdentity('Xvfb', 'xvfb'),
        xdotool: await toolIdentity('xdotool', 'xdotool'),
        xclip: await toolIdentity('xclip', 'xclip'),
      },
    };
    display = await startXvfb();
    report.environment.display = display.display;
    X = x11(display.env);
    sentinel = await startSentinel();
    backend = await startBackend({ root, directory, pdfium, register: (stop) => backendStops.push(stop) });
    report.environment.backend = { postgres: backend.postgres, server: 'organization-server (sales-01 synthetic profile)', origin: 'loopback' };
    docId = backend.documentId;
    main = await startDriver('main', appEnvironment({ apiOrigin: backend.origin }));
  } catch (error) {
    report.setupError = String(error?.stack ?? error).slice(0, 2000);
    throw error;
  } finally {
    await saveReport();
  }
});

after(async () => {
  await cleanup();
  report.finishedAt = new Date().toISOString();
  report.status = overallStatus();
  // Evidence only from a complete, unfiltered run of committed sources with a fresh binary.
  report.qualifying = report.status === 'passed' && !report.filtered && report.environment?.worktreeDirty === false && report.environment?.binary?.staleAgainstSources === false;
  await saveReport();
  console.log(`desktop GUI evidence: ${directory} (status ${report.status}, qualifying ${report.qualifying})`);
});

scenario('起動・単一ウィンドウ・既存の文書画面（一覧→詳細→戻る）', async () => {
  const s = await launch(main);
  try {
    await s.waitFor(async () => new URL(await s.url()).pathname === '/documents', { message: '/documents' });
    check('既存の文書一覧が実Document APIの合成文書を表示する', await s.waitForText('[role="row"]', 'デスクトップ確認用資料'));
    check('ウィンドウはmain 1つだけ', (await s.handles()).length === 1, await s.handles());
    check('文書画面のtitle', (await s.title()) === '文書管理 | Knowledge Platform', await s.title());
    check('ヘッダーにデスクトップ実行の表示', (await s.bodyText()).includes('デスクトップで実行中'));
    await shot(s, 'documents');
    await clickText(s, 'button[data-document-id]', 'デスクトップ確認用資料');
    await clickText(s, 'button', '詳細を開く');
    await s.waitFor(async () => new URL(await s.url()).pathname === `/documents/${docId}`, { message: 'detail route' });
    check('詳細画面へRouterで遷移し、題名を表示', await s.waitForText('h1', 'デスクトップ確認用資料'));
    await shot(s, 'detail');
    await s.back();
    await s.waitFor(async () => new URL(await s.url()).pathname === '/documents', { message: 'history back' });
    check('戻る操作で一覧へ戻る', await s.waitForText('[role="row"]', 'デスクトップ確認用資料'));
  } finally {
    await quit(s);
  }
});

scenario('Router・Query：deep link（/tasks）と再読み込み、タスク・検索・担当と委任・文書のナビゲーション', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    await s.execute('window.location.assign("/tasks")');
    await s.waitFor(async () => new URL(await s.url()).pathname === '/tasks', { message: '/tasks deep link' });
    const api = await s.executeAsync(`const done = arguments[arguments.length - 1];
      fetch('/v1/organization/tasks').then(async (r) => done({ status: r.status, title: (await r.json()).items?.[0]?.title }), (e) => done({ error: String(e) }));`);
    const firstTitle = api.title;
    check('タスク一覧のAPI（Work API）はshell経由で200', api.status === 200, api);
    check('タスク画面がWork APIのタスクを表示（deep linkはindex.htmlへfallback）', firstTitle && await s.waitForText('[aria-label="タスク一覧"]', firstTitle), firstTitle);
    await s.execute('window.location.reload()');
    await s.waitFor(async () => (await s.execute('return document.readyState')) === 'complete', { message: 'reload' });
    check('/tasksで再読み込みしても同じ画面を復元', new URL(await s.url()).pathname === '/tasks' && await s.waitForText('[aria-label="タスク一覧"]', firstTitle));
    await shot(s, 'tasks');
    await clickText(s, 'nav a', '検索');
    await s.waitFor(async () => new URL(await s.url()).pathname === '/search', { message: '/search' });
    check('検索画面へ遷移（Search内部は変更しない。PoCでは未実装表示）', await s.waitForText('h1', '検索'));
    await s.execute('window.location.assign("/organization/responsibilities")');
    check('担当と委任の画面を表示', await s.waitForText('h1', '担当と委任'));
    await s.waitFor(async () => !(await s.bodyText()).includes('読み込み中'), { message: 'responsibilities loaded' });
    const responsibilities = await s.bodyText();
    check('担当と委任をshell経由で取得（取得失敗の表示なし）', !responsibilities.includes('確認できません'), responsibilities.slice(responsibilities.indexOf('担当と委任'), responsibilities.indexOf('担当と委任') + 200));
    await shot(s, 'responsibilities');
    await clickText(s, 'nav a', '文書');
    await s.waitFor(async () => new URL(await s.url()).searchParams.get('view') === 'published', { message: 'documents published view' });
    check('文書（公開中）へ戻る', await s.waitForText('[role="row"]', 'デスクトップ確認用資料'));
    await clickText(s, 'nav a', '編集作業');
    await s.waitFor(async () => new URL(await s.url()).searchParams.get('view') === 'authoring', { message: 'authoring view' });
    check('編集作業ビューへ遷移', true);
  } finally {
    await quit(s);
  }
});

scenario('キーボード操作とfocus（skip link、ダイアログの開閉とfocus復帰）', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    // Start sequential navigation from the top of the page. blur() alone keeps
    // WebKit's starting point at the last focused element (the document list
    // may restore focus to its selected row), so focus the body itself.
    await s.execute('window.focus(); const body = document.body; body.setAttribute("tabindex", "-1"); body.focus(); body.removeAttribute("tabindex");');
    await s.keys([Keys.TAB]);
    const skip = await s.execute('return document.activeElement.textContent.trim();');
    check('最初のTabでskip link「メインコンテンツへ」にfocus', skip === 'メインコンテンツへ', skip);
    const outline = await s.execute('const st = getComputedStyle(document.activeElement); return { outline: st.outlineStyle, width: st.outlineWidth, shadow: st.boxShadow };');
    check('focus表示が見える（outlineまたはbox-shadow）', outline.outline !== 'none' || outline.shadow !== 'none', outline);
    await s.keys([Keys.ENTER]);
    await s.waitFor(async () => (await s.execute('return document.activeElement.id;')) === 'main-content', { message: 'focus on main' });
    check('Enterでmainへfocusが移る', true);
    await gotoLocalWorkspaces(s);
    const trigger = await s.waitForText('button', '新しいWorkspace');
    await s.execute('arguments[0].focus();', [trigger.ref]);
    await s.keys([Keys.ENTER]);
    await s.waitFor(async () => (await s.execute('return document.activeElement.getAttribute("aria-label");')) === 'Workspace名', { message: 'dialog input focus' });
    check('Enterでダイアログが開き、名前欄にfocus', true);
    await s.keys([Keys.ESCAPE]);
    await s.waitFor(async () => (await s.execute('return document.activeElement.textContent.trim();')) === '新しいWorkspace', { message: 'focus restored' });
    check('Escapeで閉じ、開いたボタンへfocusが戻る', true);
    await shot(s, 'keyboard');
  } finally {
    await quit(s);
  }
});

scenario('reduced motion：既定（動きあり）', async () => {
  const s = await launch(main);
  try {
    const motion = await s.execute('return { reduce: matchMedia("(prefers-reduced-motion: reduce)").matches, fast: getComputedStyle(document.documentElement).getPropertyValue("--motion-fast").trim() };');
    check('既定ではprefers-reduced-motionはreduceでない', motion.reduce === false, motion);
    check('既定のmotion tokenは0msでない', motion.fast !== '0ms', motion);
  } finally {
    await quit(s);
  }
});

scenario('Document API転送：上り下りのbody完全性、原本ダウンロード、境界（脱出・CSP・応答の型）', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    const integrity = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const line = '【合成】デスクトップ転送確認の行です。0123456789\\n';
      const text = Array.from({ length: 40000 }, (_, i) => i + ':' + line).join('');
      const bytes = new TextEncoder().encode(text);
      const form = new FormData();
      form.append('request', new Blob([JSON.stringify({ folderId: arguments[0], title: 'デスクトップ転送確認（合成）', documentMetadata: {}, versionMetadata: {} })], { type: 'application/json' }));
      form.append('file', new Blob([bytes], { type: 'text/plain' }), 'transfer-check.txt');
      const created = await fetch('/v1/documents', { method: 'POST', body: form });
      if (created.status !== 201) return { step: 'create', status: created.status, body: (await created.text()).slice(0, 300) };
      const { documentId, documentVersionId } = await created.json();
      const files = await (await fetch('/v1/documents/' + documentId + '/versions/' + documentVersionId + '/files?purpose=authoring')).json();
      const item = files.items[0];
      const got = new Uint8Array(await (await fetch('/v1/documents/' + documentId + '/versions/' + documentVersionId + '/files/' + item.contentItemId + '/' + item.representationId + '?purpose=authoring')).arrayBuffer());
      let same = got.length === bytes.length;
      for (let i = 0; same && i < got.length; i++) same = got[i] === bytes[i];
      return { step: 'done', sent: bytes.length, received: got.length, same };
    })().then(done, (e) => done({ error: String(e) }));`, [SHARED_FOLDER]);
    check('multipart上り（約2.8MB）と下りのbyte列が完全一致', integrity.same === true && integrity.sent > 2_500_000, integrity);

    const identity = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const pick = (j) => ({ principalId: j.principalId, actingAssignmentId: j.actingAssignmentId });
      const plain = await fetch('/v1/organization/session');
      const claimed = await fetch('/v1/organization/session', { headers: { 'x-organization-profile': 'approver-01', 'x-principal-id': 'someone-else', 'x-acting-principal': 'someone-else' } });
      return { plain: pick(await plain.json()), claimed: pick(await claimed.json()), nosniff: plain.headers.get('x-content-type-options'), cookie: plain.headers.get('set-cookie') };
    })().then(done, (e) => done({ error: String(e) }));`);
    check('identity系ヘッダーを付けても主体は変わらない（転送しない）', identity.plain?.principalId && JSON.stringify(identity.plain) === JSON.stringify(identity.claimed), identity);
    check('API応答にnosniff、Set-Cookieなし', identity.nosniff === 'nosniff' && !identity.cookie, identity);

    // WebKit's URL parser removes "/v1/../" and "%2e%2e" itself (the shell sees
    // "/health/ready" and serves index.html); these encoded forms are not
    // normalized, so they reach the shell's /v1 rule and must stay inside /v1.
    const normalized = await pageFetch(s, '/v1/../health/ready');
    check('「/v1/../」はWebKitが正規化し、shellはアプリのHTMLを返す（転送しない）', normalized.status === 200 && (normalized.type ?? '').includes('text/html') && !normalized.body.includes('"ok"'), normalized);
    for (const probe of ['/v1/..%2fhealth/ready', '/v1/%2e%2e%2fhealth/ready', '/v1/..%5chealth/ready', '/v1/..%2f..%2fhealth/ready']) {
      const reply = await pageFetch(s, probe);
      check(`${probe} は/v1の内側として転送され、/v1外（health）へは出ない`, reply.status === 404 && !reply.body.includes('"ok"') && !(reply.type ?? '').includes('text/html'), reply);
    }
    const options = await pageFetch(s, '/v1/organization/session', { method: 'OPTIONS' });
    check('OPTIONSは405（転送しない）', options.status === 405, options);

    const marker = randomUUID();
    const csp = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const violations = [];
      const listener = (e) => violations.push({ directive: e.effectiveDirective || e.violatedDirective, blocked: e.blockedURI });
      document.addEventListener('securitypolicyviolation', listener);
      const attempt = async (url, init) => { try { const r = await fetch(url, init); return { status: r.status, type: r.type }; } catch (e) { return { error: String(e) }; } };
      const out = {
        sentinel: await attempt(arguments[0] + '/connect-cors-' + arguments[2]),
        sentinelNoCors: await attempt(arguments[0] + '/connect-no-cors-' + arguments[2], { mode: 'no-cors' }),
        backendNoCors: await attempt(arguments[1] + '/v1/organization/session', { mode: 'no-cors' }),
      };
      await new Promise((r) => setTimeout(r, 300));
      document.removeEventListener('securitypolicyviolation', listener);
      return { ...out, violations };
    })().then(done, (e) => done({ error: String(e) }));`, [sentinel.origin, backend.origin, marker]);
    await delay(300);
    const reached = sentinel.hits.filter((hit) => hit.includes(marker));
    check('他originへのfetchはCSP（connect-src）で送信前に遮断（CORS許可のloopbackにも届かない）', csp.sentinel?.error && csp.sentinelNoCors?.error && reached.length === 0, { csp, reached });
    check('backendへの直接接続（no-cors）もCSPで遮断', Boolean(csp.backendNoCors?.error), csp.backendNoCors);
    check('遮断はsecuritypolicyviolation（connect-src）として報告される', csp.violations.filter((item) => item.directive === 'connect-src').length >= 2, csp.violations);

    const script = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const form = new FormData();
      form.append('request', new Blob([JSON.stringify({ folderId: arguments[0], title: 'スクリプト原本の確認（合成）', documentMetadata: {}, versionMetadata: {} })], { type: 'application/json' }));
      form.append('file', new Blob(['window.__uploadedScriptRan = true;'], { type: 'text/javascript' }), 'uploaded.js');
      const created = await fetch('/v1/documents', { method: 'POST', body: form });
      if (created.status !== 201) return { created: created.status, body: (await created.text()).slice(0, 300) };
      const { documentId, documentVersionId } = await created.json();
      const files = await (await fetch('/v1/documents/' + documentId + '/versions/' + documentVersionId + '/files?purpose=authoring')).json();
      const f = files.items[0];
      const url = '/v1/documents/' + documentId + '/versions/' + documentVersionId + '/files/' + f.contentItemId + '/' + f.representationId + '?purpose=authoring';
      const response = await fetch(url);
      const type = response.headers.get('content-type');
      const csp = response.headers.get('content-security-policy');
      const text = await response.text();
      const loaded = await new Promise((resolve) => { const el = document.createElement('script'); el.src = url; el.onload = () => resolve('loaded'); el.onerror = () => resolve('error'); document.head.appendChild(el); });
      await new Promise((r) => setTimeout(r, 500));
      return { storedType: f.mediaType, type, csp, text, loaded, ran: window.__uploadedScriptRan === true };
    })().then(done, (e) => done({ error: String(e) }));`, [SHARED_FOLDER]);
    check('JavaScriptとして登録した原本も、/v1応答はapplication/octet-stream＋CSP sandbox', script.type === 'application/octet-stream' && (script.csp ?? '').includes('sandbox') && script.text === 'window.__uploadedScriptRan = true;', script);
    check('/v1の原本を<script>で読み込んでも実行されない（アプリのoriginのコードにならない）', script.loaded === 'error' && script.ran === false, script);

    const panel = await s.waitForText('button', 'ファイルを取得');
    await panel.click();
    const downloaded = join(home, 'Downloads', 'desktop-reference.txt');
    await s.waitFor(async () => (await stat(downloaded).catch(() => undefined))?.size > 0, { message: 'download file' });
    const content = await readFile(downloaded, 'utf8');
    check('「ファイルを取得」で原本がDownloadsへ保存され、内容が一致', content === '【合成データ】デスクトップ版の画面確認に使う共有資料です。\n', content);
    await shot(s, 'download');
  } finally {
    await quit(s);
  }
});

scenario('GUIでの文書登録：ファイル選択→multipart上り（shell経由）→登録結果と一覧の更新', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    const source = join(fixtures, '登録用原本.txt');
    const original = `${'【合成】デスクトップから登録する原本です。\n'.repeat(2000)}`;
    await writeFile(source, original);
    await clickText(s, 'button', '文書を登録');
    const titleInput = await s.waitFor(() => s.find('form input:not([type])'), { message: 'title input' });
    await titleInput.type('デスクトップ登録確認（合成）');
    await (await s.find('form input[type="file"]')).type(source);
    await s.waitForText('form p', '登録用原本.txt');
    await clickText(s, 'button[type="submit"]', '下書きとして登録');
    check('登録後、新しい文書の詳細画面を表示', await s.waitForText('h1', 'デスクトップ登録確認（合成）', { timeout: 60_000 }));
    registeredId = new URL(await s.url()).pathname.split('/').at(-1);
    await shot(s, 'registered');
    const roundTrip = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const list = await (await fetch('/v1/documents?view=authoring&folderId=${SHARED_FOLDER}&pageSize=50')).json();
      const item = (list.items ?? []).find((entry) => entry.documentId === arguments[0]);
      if (!item) return { found: false, titles: (list.items ?? []).map((entry) => entry.title) };
      const versionId = item.displayVersion?.versionId ?? item.documentVersionId;
      const files = await (await fetch('/v1/documents/' + item.documentId + '/versions/' + versionId + '/files?purpose=authoring')).json();
      const file = files.items[0];
      const text = await (await fetch('/v1/documents/' + item.documentId + '/versions/' + versionId + '/files/' + file.contentItemId + '/' + file.representationId + '?purpose=authoring')).text();
      return { found: true, name: file.displayName, text };
    })().then(done, (e) => done({ error: String(e) }));`, [registeredId]);
    check('登録した原本をAPIから取得すると内容が一致', roundTrip.found && roundTrip.text === original, { found: roundTrip.found, name: roundTrip.name, error: roundTrip.error, titles: roundTrip.titles, length: roundTrip.text?.length });
    await clickText(s, 'nav a', '編集作業');
    check('編集作業の一覧に登録した文書が表示される（Queryの再取得）', await s.waitForText('[role="row"]', 'デスクトップ登録確認（合成）'));
  } finally {
    await quit(s);
  }
});

scenario('GUIでの文書編集：メタデータ保存（PATCH）と作業版の原本差し替え（multipart PUT）', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    await s.execute('window.location.assign(arguments[0]);', [`/documents/${registeredId}?view=authoring`]);
    await s.waitForText('h1', 'デスクトップ登録確認（合成）');
    await clickText(s, 'button', 'メタデータを編集');
    await (await s.waitFor(() => s.find('[role="dialog"] textarea[aria-label="文書種別"]'))).type('デスクトップ確認（合成）');
    await (await s.find('[role="dialog"] textarea[aria-label="変更理由"]')).type('GUIからの確認');
    await clickText(s, '[role="dialog"] button', '保存する');
    check('メタデータの保存（PATCH）が成功', await s.waitForText('[role="dialog"]', 'メタデータを更新しました。', { timeout: 30_000 }));
    await shot(s, 'metadata-saved');
    await clickText(s, '[role="dialog"] button', '閉じる');
    const detail = await s.executeAsync(`const done = arguments[arguments.length - 1];
      fetch('/v1/documents/' + arguments[0] + '?view=authoring').then(async (r) => done(JSON.stringify(await r.json())), (e) => done(String(e)));`, [registeredId]);
    check('保存したメタデータがAPIから読める', detail.includes('デスクトップ確認（合成）'), detail.slice(0, 300));
    const replacement = join(fixtures, '差し替え原本.txt');
    const replaced = '【合成】作業版を差し替えた原本です。\n'.repeat(500);
    await writeFile(replacement, replaced);
    await clickText(s, '[role="tab"], button', '版・改訂');
    await clickText(s, 'button', '作業版を編集');
    await (await s.waitFor(() => s.find('main input[type="file"]'))).type(replacement);
    await s.waitForText('main', '差替後: 差し替え原本.txt');
    await clickText(s, 'button[type="submit"]', '作業版を保存');
    check('作業版の保存（multipart PUT）が成功', await s.waitFor(async () => (await s.bodyText()).includes('作業版を保存しました。'), { timeout: 60_000, message: '作業版を保存しました。' }));
    await shot(s, 'working-version-saved');
    const files = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const list = await (await fetch('/v1/documents?view=authoring&folderId=${SHARED_FOLDER}&pageSize=50')).json();
      const item = list.items.find((entry) => entry.documentId === arguments[0]);
      const versionId = item.displayVersion?.versionId ?? item.documentVersionId;
      const files = await (await fetch('/v1/documents/' + item.documentId + '/versions/' + versionId + '/files?purpose=authoring')).json();
      const file = files.items[0];
      const text = await (await fetch('/v1/documents/' + item.documentId + '/versions/' + versionId + '/files/' + file.contentItemId + '/' + file.representationId + '?purpose=authoring')).text();
      return { name: file.displayName, text };
    })().then(done, (e) => done({ error: String(e) }));`, [registeredId]);
    check('差し替えた原本がAPIから同じ内容で取得できる', files.text === replaced, { name: files.name, error: files.error, length: files.text?.length });
  } finally {
    await quit(s);
  }
});

scenario('ローカルWorkspace：作成・名前変更・管理フォルダーへの作成と読み取り・二重操作', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    check('実行環境がデスクトップ版・各機能が利用可', (await s.bodyText()).includes('デスクトップ版') && !(await s.bodyText()).includes('利用できません'));
    await clickText(s, 'button', '新しいWorkspace');
    await (await s.waitFor(() => s.find('input[aria-label="Workspace名"]'))).type('案件A（合成）');
    await clickText(s, '[role="dialog"] button[type="submit"]', '作成する');
    check('作成の通知', await notice(s, 'Workspace「案件A（合成）」を作成しました。'));
    await clickText(s, 'button', '名前を変更');
    const renameInput = await s.waitFor(() => s.find('input[aria-label="新しいWorkspace名"]'));
    await renameInput.clear();
    await renameInput.type('案件A（改名）');
    await clickText(s, 'button[type="submit"]', '変更する');
    check('名前変更の通知（場所は変わらない）', await notice(s, '「案件A（改名）」に変更しました。フォルダーの場所は変わりません。'));
    workspaceName = '案件A（改名）';
    const [workspace] = (await workspaces(s)).filter((item) => item.name === workspaceName);
    const managedDirs = await readdir(join(stateRoot, 'managed'));
    check('管理フォルダーが1つ作られた（名前と無関係なID）', managedDirs.length === 1 && !managedDirs[0].includes('案件'), managedDirs);
    managedDir = join(stateRoot, 'managed', managedDirs[0]);
    await (await s.waitFor(() => s.find('button[aria-label="管理フォルダーを開く"]'), { message: '管理フォルダーを開く' })).click();
    await s.waitForText('p', 'このフォルダーには表示できる項目がありません。');
    await (await s.find('input[aria-label="ファイル名"]')).type('メモ.txt');
    await (await s.find('textarea[aria-label="内容"]')).type('合成のメモです。');
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    check('ファイル作成の通知', await notice(s, 'ファイル「メモ.txt」を作成しました。'));
    const onDisk = await readFile(join(managedDir, 'メモ.txt'), 'utf8');
    check('ディスク上の管理フォルダーに同じ内容で作成', onDisk === '合成のメモです。', onDisk);
    await (await s.waitFor(() => s.find('button[aria-label="メモ.txt の内容を表示"]'))).click();
    check('内容の表示（限定read）', await s.waitForText('[role="region"] pre', '合成のメモです。'));
    await (await s.find('input[aria-label="ファイル名"]')).type('メモ.txt');
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    check('同名は上書きせず拒否', await alertText(s, '同じ名前のファイルが既にあります'));
    for (const bad of ['../escape.txt', 'CON', 'a/b.txt']) {
      const input = await s.find('input[aria-label="ファイル名"]');
      await input.clear();
      await input.type(bad);
      await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
      check(`不正な名前「${bad}」を拒否`, await alertText(s, '名前に使えない文字または予約名が含まれています。'), bad);
    }
    const escaped = [...await findNamed(home, 'escape.txt'), ...await findNamed(fixtures, 'escape.txt')];
    check('「../escape.txt」は管理フォルダーの外（managed直下・記録フォルダー・home全体）にも作られていない', escaped.length === 0 && !(await readdir(join(stateRoot, 'managed'))).includes('escape.txt'), escaped);
    check('画面に絶対pathが出ない', !(await s.bodyText()).includes(home), workspace?.workspaceId);
    await shot(s, 'managed-folder');

    // Two synchronous submits in one task: only one operation may run.
    const nameInput = await s.find('input[aria-label="ファイル名"]');
    await nameInput.clear();
    await nameInput.type('二重送信.txt');
    await (await s.find('textarea[aria-label="内容"]')).type('二重送信の確認（合成）');
    check('IPCの計測を設定', await instrumentIpc(s));
    await s.execute(`const form = document.querySelector(arguments[0]); form.requestSubmit(); form.requestSubmit();`, [FILE_FORM]);
    check('連続した2回の送信でも作成は1回', await notice(s, 'ファイル「二重送信.txt」を作成しました。'));
    const fileCalls = callsOf(await ipcState(s), 'file.create');
    check('file.createのIPCは1回だけ', fileCalls.length === 1, fileCalls);
    check('二重作成による「既にあります」の表示はない', !(await s.bodyText()).includes('同じ名前のファイルが既にあります'));
    check('ディスク上も1件', (await readdir(managedDir)).filter((name) => name === '二重送信.txt').length === 1);

    await clickText(s, 'button', '新しいWorkspace');
    await (await s.waitFor(() => s.find('input[aria-label="Workspace名"]'))).type('二重クリック確認（合成）');
    await (await s.waitForText('[role="dialog"] button[type="submit"]', '作成する')).doubleClick();
    check('「作成する」の二重クリックでもWorkspace作成は1回', await notice(s, 'Workspace「二重クリック確認（合成）」を作成しました。'));
    const createCalls = callsOf(await ipcState(s), 'workspace.create');
    check('workspace.createのIPCは1回だけ', createCalls.length === 1, createCalls);
    check('同名のWorkspaceは1つだけ', (await workspaces(s)).filter((item) => item.name === '二重クリック確認（合成）').length === 1);
  } finally {
    await quit(s);
  }
});

scenario('ネイティブのフォルダー選択（実GTKダイアログ）：取消・選択・二重選択・同じフォルダー', async () => {
  const s = await launch(main);
  try {
    const before = await openWorkspace(s, workspaceName);
    await clickText(s, 'button', 'フォルダーを追加');
    await X.waitDialog(true);
    await rootShot('native-dialog');
    const busy = await invoke(s, 'directory.choose', { context: contextOf(before) });
    check('IPC：ダイアログ表示中の2つ目の選択要求はpicker_busy', busy.err?.reason === 'picker_busy', busy);
    await X.cancelDialog();
    check('取消は変更なしの通知', await notice(s, 'フォルダーの選択を取り消しました。変更はありません。'));
    check('取消後もフォルダー数は不変', (await workspaces(s)).find((item) => item.name === workspaceName).bindings.length === before.bindings.length);
    const focused = await s.execute('return document.activeElement.textContent.trim();');
    check('ダイアログを閉じた後もページのfocusは「フォルダーを追加」に残る', focused === 'フォルダーを追加', focused);
    await clickText(s, 'button', 'フォルダーを追加');
    await X.chooseFolder(folders.folder);
    check('選択したフォルダーを追加（参照のみ）', await notice(s, 'フォルダー「資料フォルダー」を追加しました。'));
    const attached = (await workspaces(s)).find((item) => item.name === workspaceName);
    check('IPC応答に絶対pathを含まない', !JSON.stringify(attached).includes(fixtures), attached.bindings.map((item) => item.label));
    await clickText(s, 'button', 'フォルダーを追加');
    await X.chooseFolder(folders.folder);
    check('同じフォルダーの再追加は拒否', await alertText(s, 'このフォルダーは既に追加されています。'));
    await shot(s, 'attached');
  } finally {
    await quit(s);
  }
});

scenario('追加フォルダーの閲覧：一覧・階層移動・先頭1MiBの内容・書き込み中の拒否・リンク拒否・作成', async () => {
  const s = await launch(main);
  try {
    await openWorkspace(s, workspaceName);
    await (await s.waitFor(() => s.find('button[aria-label="資料フォルダーを開く"]'), { message: '資料フォルダーを開く' })).click();
    const table = await s.waitForText('table', 'readme.txt');
    const text = await table.text();
    check('通常ファイルとフォルダーを一覧', text.includes('readme.txt') && text.includes('sub') && text.includes('大きいファイル.txt'), text);
    check('symlinkは一覧に出さない', !text.includes('外部へのリンク'), text);
    check('表示できない項目の件数を表示', await s.waitForText('p', '表示できない項目が'));
    const writer = await open(join(folders.folder, 'readme.txt'), 'r+');
    try {
      await (await s.find('button[aria-label="readme.txt の内容を表示"]')).click();
      check('別のプロセスが書き込みで開いているファイルは読まない（concurrent_change）', await alertText(s, '操作中に内容が変更されました。一覧を更新してからもう一度試してください。'));
      check('書き込み中は内容を表示しない', (await s.findAll('[role="region"] pre')).length === 0);
    } finally {
      await writer.close();
    }
    await shot(s, 'concurrent-change');
    await (await s.find('button[aria-label="readme.txt の内容を表示"]')).click();
    check('書き込みが終われば表示できる', await s.waitForText('[role="region"] pre', '合成データの説明文です。'));
    await (await s.find('button[aria-label="大きいファイル.txt の内容を表示"]')).click();
    check('1MiBを超えるファイルは「先頭1MiBのみ」と表示', await s.waitForText('[role="region"] p', '（先頭1MiBのみ表示）'));
    const shown = await s.execute('return document.querySelector("[role=region] pre").textContent;');
    check('表示内容はファイルの先頭1,048,576バイトと一致', shown === BIG.slice(0, MiB), { shown: shown.length, expected: MiB, file: BIG.length });
    const linked = await s.findAll('button[aria-label="二重リンク.txt の内容を表示"]');
    if (linked.length) {
      await linked[0].click();
      check('ハードリンクされたファイルは開かない', await alertText(s, '複数の場所にリンクされたファイルは安全のため開けません。'));
    } else {
      check('ハードリンクされたファイルは一覧に出ない', !text.includes('二重リンク.txt'), text);
    }
    await clickText(s, 'table button', 'sub');
    check('下の階層へ移動', await s.waitForText('table', 'nested.txt'));
    await clickText(s, 'button', '上の階層へ');
    await s.waitForText('table', 'readme.txt');
    await (await s.find('input[aria-label="ファイル名"]')).type('デスクトップから作成.txt');
    await (await s.find('textarea[aria-label="内容"]')).type('選択フォルダーへの作成（合成）');
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    await notice(s, 'ファイル「デスクトップから作成.txt」を作成しました。');
    check('選択フォルダー内に作成', (await readFile(join(folders.folder, 'デスクトップから作成.txt'), 'utf8')) === '選択フォルダーへの作成（合成）');
    check('外部の場所には何も作られていない', (await readdir(folders.outside)).sort().join(',') === 'linked-origin.txt,secret.txt');
    await shot(s, 'browse');
  } finally {
    await quit(s);
  }
});

scenario('ページscriptからの不正なIPC・遷移・新規ウィンドウ・frame、読み取りの範囲と同時数', async () => {
  const s = await launch(main);
  try {
    const workspace = await openWorkspace(s, workspaceName);
    const context = contextOf(workspace);
    const explicit = workspace.bindings.find((item) => item.source === 'explicit');
    for (const locator of [['..'], ['/etc'], ['a/b'], ['C:\\Windows'], ['sub', '..', '..'], ['']]) {
      const reply = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator } });
      check(`IPC：locator ${JSON.stringify(locator)} は拒否`, reply.err?.code === 'invalid_locator', reply);
    }
    const link = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator: ['外部へのリンク'] } });
    check('IPC：symlinkを名指ししても辿らない', link.err?.reason === 'symbolic_link', link);
    const openLink = await invoke(s, 'file.openRead', { context, ref: { bindingId: explicit.bindingId, locator: ['外部へのリンク', 'secret.txt'] }, expectedFileIdentity: 'x' });
    check('IPC：symlink配下のファイルはsymlinkとして拒否（識別子の不一致より前に止まる）', openLink.err?.reason === 'symbolic_link', openLink);
    const forged = await invoke(s, 'entries.list', { context, ref: { bindingId: 'b_forged', locator: [] } });
    check('IPC：偽のbindingIdは拒否', Boolean(forged.err), forged);
    const unknown = await invoke(s, 'shell.execute', { program: '/bin/sh' });
    check('IPC：未知のcommandは拒否', unknown.err?.reason === 'unknown_command', unknown);
    const extra = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator: [] }, path: '/etc' });
    check('IPC：余分なfield（path等）は拒否', extra.err?.reason === 'invalid_request', extra);
    for (const [name, args] of [['plugin:fs|read_text_file', { path: '/etc/passwd' }], ['plugin:shell|execute', { program: 'sh' }], ['plugin:window|create', { options: { label: 'x' } }], ['plugin:webview|create_webview_window', { options: { label: 'x', url: 'https://example.com' } }]]) {
      const reply = await invokeRaw(s, name, args);
      check(`IPC：${name} は利用できない`, Boolean(reply.err), reply);
    }

    const listing = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator: [] } });
    const entry = (name) => listing.ok.entries.find((item) => item.name === name);
    const openRead = (name) => invoke(s, 'file.openRead', { context, ref: { bindingId: explicit.bindingId, locator: entry(name).locator }, expectedFileIdentity: entry(name).fileIdentity });
    const handles = [];
    for (let i = 0; i < 4; i++) handles.push((await openRead('readme.txt')).ok);
    check('IPC：読み取りは同時に4件まで開ける', handles.every(Boolean), handles.length);
    const fifth = await openRead('readme.txt');
    check('IPC：5件目はtoo_many', fifth.err?.reason === 'too_many', fifth);
    for (const handle of handles) await invoke(s, 'file.closeRead', { context, readHandleId: handle.readHandleId });
    const big = (await openRead('大きいファイル.txt')).ok;
    check('IPC：閉じれば再び開ける（大きいファイルのsnapshot）', big?.sizeBytes === BIG.length, big);
    const tail = await invoke(s, 'file.read', { context, readHandleId: big.readHandleId, offset: MiB, length: MiB });
    check('IPC：offset 1MiBからの読み取りはファイルの残りと一致し、eof', tail.ok && Buffer.from(tail.ok.bytesBase64, 'base64').toString('latin1') === BIG.slice(MiB) && tail.ok.eof === true && tail.ok.offset === MiB, tail.ok ? { offset: tail.ok.offset, eof: tail.ok.eof } : tail);
    const tooLong = await invoke(s, 'file.read', { context, readHandleId: big.readHandleId, offset: 0, length: MiB + 1 });
    check('IPC：1回の読み取りが1MiBを超える要求はbrokerが拒否（too_large）', tooLong.err?.reason === 'too_large', tooLong);
    await invoke(s, 'file.closeRead', { context, readHandleId: big.readHandleId });

    const marker = randomUUID();
    const opened = await s.execute('return window.open(arguments[0]) === null;', [`${sentinel.origin}/window-${marker}`]);
    await delay(1000);
    check('window.openで新しいウィンドウは開かない（要求も出ない）', opened === true && (await s.handles()).length === 1 && !sentinel.hits.some((hit) => hit.includes(`window-${marker}`)), { handles: await s.handles(), hits: sentinel.hits });
    const frames = await s.executeAsync(`const done = arguments[arguments.length - 1]; (async () => {
      const violations = [];
      const messages = [];
      const listener = (e) => violations.push({ directive: e.effectiveDirective || e.violatedDirective, blocked: e.blockedURI });
      document.addEventListener('securitypolicyviolation', listener);
      window.addEventListener('message', (e) => messages.push(String(e.data)));
      const remote = document.createElement('iframe');
      remote.src = arguments[0];
      const inline = document.createElement('iframe');
      inline.src = 'data:text/html,<script>parent.postMessage(typeof window.__TAURI_INTERNALS__, "*")<\\/script>';
      document.body.append(remote, inline);
      await new Promise((r) => setTimeout(r, 1500));
      remote.remove();
      inline.remove();
      document.removeEventListener('securitypolicyviolation', listener);
      return { violations, messages };
    })().then(done, (e) => done({ error: String(e) }));`, [`${sentinel.origin}/frame-${marker}`]);
    check('iframe（他origin・data:）はCSP frame-srcで遮断され、frameからのscriptも動かない', frames.messages?.length === 0 && frames.violations?.some((item) => item.directive === 'frame-src') && !sentinel.hits.some((hit) => hit.includes(`frame-${marker}`)), { frames, hits: sentinel.hits });
    await s.execute('window.location.assign(arguments[0]);', [`${sentinel.origin}/navigate-${marker}`]);
    // The loopback sentinel answers in milliseconds, so an allowed navigation would have committed.
    await delay(2000);
    await s.waitFor(async () => (await s.execute('return document.readyState')) === 'complete', { message: 'document ready' });
    check('他originへの遷移は拒否され、要求も出ずアプリに留まる', (await s.url()).startsWith('tauri://localhost/') && !sentinel.hits.some((hit) => hit.includes(`navigate-${marker}`)), { url: await s.url(), hits: sentinel.hits });
    check('画面とIPC応答に絶対pathが出ない', !(await s.bodyText()).includes(fixtures));
  } finally {
    await quit(s);
  }
});

scenario('8MiBの上限：ちょうど8MiBは作成・表示でき、超過は画面とbrokerの両方で拒否', async () => {
  const s = await launch(main);
  try {
    const workspace = await openWorkspace(s, workspaceName);
    await (await s.waitFor(() => s.find('button[aria-label="管理フォルダーを開く"]'))).click();
    await s.waitForText('table', 'メモ.txt');
    check('IPCの計測を設定', await instrumentIpc(s));
    await (await s.find('input[aria-label="ファイル名"]')).type('上限ちょうど.txt');
    check('内容欄に8MiB（8,388,608文字）を入力', (await fillRepeated(s, 'textarea[aria-label="内容"]', 'k', 8 * MiB)) === 8 * MiB);
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    check('ちょうど8MiBは作成できる', await notice(s, 'ファイル「上限ちょうど.txt」を作成しました。', { timeout: 120_000 }));
    const exact = await readFile(join(managedDir, '上限ちょうど.txt'));
    check('ディスク上も8,388,608バイトで内容が一致', exact.length === 8 * MiB && sha256(exact) === sha256(Buffer.alloc(8 * MiB, 'k')), exact.length);
    const nameInput = await s.find('input[aria-label="ファイル名"]');
    await nameInput.clear();
    await nameInput.type('上限超過.txt');
    await fillRepeated(s, 'textarea[aria-label="内容"]', 'k', 8 * MiB + 1);
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    check('8MiB+1バイトは画面で拒否', await alertText(s, TOO_LARGE));
    const calls = callsOf(await ipcState(s), 'file.create');
    check('超過分はIPCを送らずに拒否（file.createは最初の1回だけ）', calls.length === 1, calls);
    const direct = await s.executeAsync(`const done = arguments[arguments.length - 1];
      const request = arguments[0]; request.bytesBase64 = btoa('k'.repeat(8 * 1024 * 1024 + 1));
      window.__TAURI__.core.invoke('local_workspace_runtime', { command: 'file.create', request }).then((ok) => done({ ok }), (err) => done({ err }));`,
    [{ context: contextOf(workspace), parent: { bindingId: workspace.managedBindingId, locator: [] }, name: '上限超過IPC.txt', operationId: randomUUID() }]);
    check('IPC：画面を経由しない8MiB+1バイトはbrokerが拒否（too_large）', direct.err?.code === 'limit' && direct.err?.reason === 'too_large', direct);
    check('拒否したファイルはどちらも作られていない', !(await readdir(managedDir)).some((name) => name.startsWith('上限超過')));
    await writeFile(join(managedDir, '九MiB.txt'), Buffer.alloc(9 * MiB, 'n'));
    await (await s.find('button[aria-label="管理フォルダーを開く"]')).click();
    await (await s.waitFor(() => s.find('button[aria-label="九MiB.txt の内容を表示"]'))).click();
    check('8MiBを超えるファイルは読み取りを拒否', await alertText(s, TOO_LARGE));
    await (await s.find('button[aria-label="上限ちょうど.txt の内容を表示"]')).click();
    check('ちょうど8MiBのファイルは先頭1MiBを表示', await s.waitForText('[role="region"] p', '（先頭1MiBのみ表示）'));
    await shot(s, 'size-limits');
  } finally {
    await quit(s);
  }
});

scenario('結果不明（IPC応答の消失）：「結果を確認」で同じ操作として確定し、移動を止める', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    const managedBefore = (await readdir(join(stateRoot, 'managed'))).length;
    check('Workspace作成の応答を1回失わせる設定', await instrumentIpc(s, 'workspace.create'));
    await clickText(s, 'button', '新しいWorkspace');
    await (await s.waitFor(() => s.find('input[aria-label="Workspace名"]'))).type('応答消失の確認（合成）');
    await clickText(s, '[role="dialog"] button[type="submit"]', '作成する');
    check('応答が届かないと「結果を確認できませんでした」を表示', await s.waitForText('[role="dialog"] [role="alert"]', OUTCOME_UNKNOWN));
    const lost = (await ipcState(s)).lost;
    check('brokerは作成を終えていた（応答だけが失われた）', lost.length === 1 && lost[0].delivered === 'ok' && (await workspaces(s)).filter((item) => item.name === '応答消失の確認（合成）').length === 1, lost);
    check('名前欄は変更できず、ボタンは「結果を確認」', !(await (await s.find('input[aria-label="Workspace名"]')).enabled()) && await s.waitForText('[role="dialog"] button[type="submit"]', '結果を確認'));
    await shot(s, 'unknown-dialog');
    await s.keys([Keys.ESCAPE]);
    await delay(300);
    check('Escapeでは閉じない（未確定の操作を残さない）', (await s.findAll('[role="dialog"]')).length === 1);
    await clickText(s, '[role="dialog"] button', 'あとで確認する');
    check('「あとで確認する」で閉じ、確認用のボタンが残る', await s.waitForText('button', 'Workspace作成の結果を確認'));
    check('未確認の操作があることを表示', await s.waitForText('p', '結果を確認していない操作があります。'));
    const blocked = await s.execute('return [...document.querySelectorAll("nav[aria-labelledby=local-workspace-list-title] li button")].map((b) => ({ name: b.textContent, disabled: b.disabled }));');
    check('確定するまで他のWorkspaceへは移動できない', blocked.filter((item) => !item.disabled).length === 1, blocked);
    await clickText(s, 'nav a', '文書');
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    await gotoLocalWorkspaces(s);
    check('画面を移動して戻ると、確認のダイアログを再表示（同じ操作が残る）', await s.waitForText('[role="dialog"] button[type="submit"]', '結果を確認'));
    await clickText(s, '[role="dialog"] button[type="submit"]', '結果を確認');
    check('「結果を確認」で作成済みとして確定', await notice(s, 'Workspace「応答消失の確認（合成）」を作成しました。'));
    const creates = callsOf(await ipcState(s), 'workspace.create');
    check('再送は同じ操作ID（新しい操作として送らない）', creates.length === 2 && creates[0].operationId === creates[1].operationId, creates);
    check('Workspaceは1つだけ、管理フォルダーも1つだけ増加', (await workspaces(s)).filter((item) => item.name === '応答消失の確認（合成）').length === 1 && (await readdir(join(stateRoot, 'managed'))).length === managedBefore + 1);
    check('確定後は移動の制限が解ける', !(await s.bodyText()).includes('結果を確認していない操作があります。'));
    const recovered = await invoke(s, 'workspace.recover', { operationId: creates[0].operationId });
    check('IPC：同じ操作IDのworkspace.recoverはready', recovered.ok?.state === 'ready', recovered);

    const workspace = await openWorkspace(s, workspaceName);
    await (await s.waitFor(() => s.find('button[aria-label="管理フォルダーを開く"]'))).click();
    await s.waitForText('table', 'メモ.txt');
    check('ファイル作成の応答を1回失わせる設定', await instrumentIpc(s, 'file.create'));
    await (await s.find('input[aria-label="ファイル名"]')).type('応答消失.txt');
    await (await s.find('textarea[aria-label="内容"]')).type('応答が失われた作成（合成）');
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '作成する');
    check('ファイル作成でも「結果を確認できませんでした」を表示', await s.waitForText(`${FILE_FORM} [role="alert"]`, OUTCOME_UNKNOWN));
    check('ディスク上は作成済み（brokerは完了していた）', (await readFile(join(managedDir, '応答消失.txt'), 'utf8')) === '応答が失われた作成（合成）');
    const states = await s.execute(`return {
      name: document.querySelector('input[aria-label="ファイル名"]').disabled,
      open: [...document.querySelectorAll('button[aria-label$="を開く"]')].map((b) => b.disabled),
      others: [...document.querySelectorAll('nav[aria-labelledby=local-workspace-list-title] li button')].filter((b) => b.getAttribute('aria-current') !== 'true').map((b) => b.disabled),
      look: [...document.querySelectorAll('button[aria-label$="を開く"], nav[aria-labelledby=local-workspace-list-title] li button:disabled')].map((b) => getComputedStyle(b).cursor),
    };`);
    check('確定するまで入力・フォルダー移動・他Workspaceへの移動を止める', states.name && states.open.every(Boolean) && states.others.every(Boolean), states);
    check('止めているボタンは見た目でも無効とわかる（cursor: not-allowed）', states.look.length > 0 && states.look.every((cursor) => cursor === 'not-allowed'), states.look);
    await shot(s, 'unknown-file');
    await clickText(s, `${FILE_FORM} button[type="submit"]`, '結果を確認');
    check('「結果を確認」で作成済みとして確定', await notice(s, 'ファイル「応答消失.txt」を作成しました。'));
    const fileCreates = callsOf(await ipcState(s), 'file.create');
    check('ファイル作成の再送も同じ操作ID', fileCreates.length === 2 && fileCreates[0].operationId === fileCreates[1].operationId, fileCreates);
    check('「既にあります」にならず、ディスク上も1件', !(await s.bodyText()).includes('同じ名前のファイルが既にあります') && (await readdir(managedDir)).filter((name) => name === '応答消失.txt').length === 1);
    check('確定後はフォルダーを開ける', await (await s.find('button[aria-label="管理フォルダーを開く"]')).enabled(), workspace.workspaceId);
  } finally {
    await quit(s);
  }
});

scenario('置き換え・脱出への対応と解除（中身は残る）', async () => {
  const s = await launch(main);
  try {
    await openWorkspace(s, workspaceName);
    await (await s.waitFor(() => s.find('button[aria-label="資料フォルダーを開く"]'), { message: '資料フォルダーを開く' })).click();
    await s.waitForText('table', 'sub');
    await rename(join(folders.folder, 'sub'), join(folders.folder, 'sub-moved'));
    await symlink(folders.outside, join(folders.folder, 'sub'));
    await clickText(s, 'table button', 'sub');
    check('一覧取得後にsymlinkへ差し替えた階層は開かない', await alertText(s, 'リンク（シンボリックリンク・ジャンクション）は安全のため開けません。'));
    await clickText(s, 'button', '上の階層へ');
    await rename(folders.folder, `${folders.folder}-old`);
    await mkdir(folders.folder);
    await (await s.waitFor(() => s.find('button[aria-label="資料フォルダーを開く"]'), { message: '資料フォルダーを開く' })).click();
    check('フォルダー自体の置き換えを検出', await alertText(s, 'フォルダーが移動・削除・置き換えされたため利用できません。'));
    check('置き換え前の古い一覧は表示しない', (await s.findAll('table')).length === 0);
    await shot(s, 'replaced');
    await (await s.waitFor(() => s.find('button[aria-label="資料フォルダーを解除"]'), { message: '資料フォルダーを解除' })).click();
    await clickText(s, 'button', '解除する');
    check('解除の通知', await notice(s, 'フォルダー「資料フォルダー」を解除しました。フォルダーの中身はそのままです。'));
    check('解除してもフォルダーの中身は残る', (await readFile(join(`${folders.folder}-old`, 'readme.txt'), 'utf8')).startsWith('合成データ'));
    await clickText(s, 'button', 'フォルダーを追加');
    await X.chooseFolder(folders.second);
    check('別のフォルダーを追加', await notice(s, 'フォルダー「第二フォルダー」を追加しました。'));
  } finally {
    await quit(s);
  }
});

scenario('再起動後の復元と、IPCでの同じ操作IDの再送・同時作成', async () => {
  let s = await launch(main);
  let first;
  try {
    const workspace = await openWorkspace(s, workspaceName);
    replayOp = randomUUID();
    const request = { context: contextOf(workspace), parent: { bindingId: workspace.managedBindingId, locator: [] }, name: '再送確認.txt', bytesBase64: Buffer.from('同じ操作（合成）').toString('base64'), operationId: replayOp };
    first = await invoke(s, 'file.create', request);
    check('IPC：作成（応答を受け取れなかった想定の1回目）', Boolean(first.ok), first);
    const createOp = randomUUID();
    const [a, b] = await Promise.all([invoke(s, 'workspace.create', { name: '同時作成（合成）', operationId: createOp }), invoke(s, 'workspace.create', { name: '同時作成（合成）', operationId: createOp })]);
    check('IPC：同じ操作IDの同時作成は1つのWorkspaceに収束', a.ok && b.ok && a.ok.workspace.workspaceId === b.ok.workspace.workspaceId, { a, b });
    check('IPC：同時作成で重複Workspaceはできない', (await workspaces(s)).filter((item) => item.name === '同時作成（合成）').length === 1);
  } finally {
    await quit(s);
  }
  s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    check('再起動後もWorkspace名を復元', await s.waitForText('nav[aria-labelledby="local-workspace-list-title"] button', workspaceName));
    await clickText(s, 'nav[aria-labelledby="local-workspace-list-title"] button', workspaceName);
    check('再起動後も追加フォルダーの登録を復元', await s.waitForText('li', '第二フォルダー'));
    await (await s.waitFor(() => s.find('button[aria-label="第二フォルダーを開く"]'), { message: '第二フォルダーを開く' })).click();
    check('復元したフォルダーを閲覧できる', await s.waitForText('table', '第二の資料.txt'));
    await (await s.waitFor(() => s.find('button[aria-label="管理フォルダーを開く"]'), { message: '管理フォルダーを開く' })).click();
    check('管理フォルダーの内容も残る', await s.waitForText('table', 'メモ.txt'));
    await shot(s, 'restored');
    const workspace = (await workspaces(s)).find((item) => item.name === workspaceName);
    const request = { context: contextOf(workspace), parent: { bindingId: workspace.managedBindingId, locator: [] }, name: '再送確認.txt', bytesBase64: Buffer.from('同じ操作（合成）').toString('base64'), operationId: replayOp };
    const replay = await invoke(s, 'file.create', request);
    check('IPC：再起動後に同じ操作IDで再送すると同じ結果（二重作成なし）', replay.ok && replay.ok.fileIdentity === first.ok.fileIdentity && replay.ok.sha256 === first.ok.sha256, { replay, first });
    const mismatch = await invoke(s, 'file.create', { ...request, bytesBase64: Buffer.from('別の内容').toString('base64') });
    check('IPC：同じ操作IDで内容が違えばoperation_mismatch', mismatch.err?.reason === 'operation_mismatch', mismatch);
    check('ディスク上も1ファイルだけ', (await readdir(managedDir)).filter((name) => name === '再送確認.txt').length === 1);
  } finally {
    await quit(s);
  }
});

scenario('強制終了（SIGKILL）からの再起動：記録の復元と、書き込み中に止めた作成の収束', async () => {
  const name = '強制終了時の作成.txt';
  const expected = Buffer.alloc(8 * MiB, 'z');
  const namesBefore = await readdir(managedDir);
  let request;
  // A separate driver for the app that is killed, so the main driver stays clean.
  const killed = await startDriver('sigkill', appEnvironment({ apiOrigin: backend.origin }));
  let s = await launch(killed);
  try {
    const workspace = await openWorkspace(s, workspaceName);
    request = { context: contextOf(workspace), parent: { bindingId: workspace.managedBindingId, locator: [] }, name, operationId: randomUUID() };
    const pids = await appPids();
    check('アプリのプロセスは1つ', pids.length === 1, pids);
    // Start an 8MiB create and kill the app as soon as its bytes reach the disk
    // (the broker records the file identity before writing), so the crash
    // lands while it writes or just after.
    await s.execute(`const request = arguments[0]; request.bytesBase64 = btoa('z'.repeat(8 * 1024 * 1024));
      window.__TAURI__.core.invoke('local_workspace_runtime', { command: 'file.create', request }); return true;`, [request]);
    const target = join(managedDir, name);
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && !((await stat(target).catch(() => undefined))?.size > 0)) { /* poll as fast as possible */ }
    process.kill(pids[0], 'SIGKILL');
    await s.waitFor(async () => (await appPids()).length === 0, { message: 'app killed' });
  } finally {
    await s.delete();
    killed.stop();
  }
  await lockReleased();
  const observed = await stat(join(managedDir, name)).then((info) => info.size, () => 'absent');
  check('書き込みが始まった後で強制終了した（強制終了直後のファイルの大きさを記録）', observed !== 'absent' && observed > 0, { observed, expected: expected.length, midWrite: observed < expected.length });
  s = await launch(main);
  try {
    const workspace = await openWorkspace(s, workspaceName);
    check('強制終了後もWorkspaceと追加フォルダーを復元', workspace?.bindings.some((item) => item.label === '第二フォルダー'), workspace?.bindings.map((item) => item.label));
    const replay = await s.executeAsync(`const done = arguments[arguments.length - 1];
      const request = arguments[0]; request.bytesBase64 = btoa('z'.repeat(8 * 1024 * 1024));
      window.__TAURI__.core.invoke('local_workspace_runtime', { command: 'file.create', request }).then((ok) => done({ ok }), (err) => done({ err }));`, [{ ...request, context: contextOf(workspace) }]);
    check('IPC：同じ操作IDの再送で作成が1件に収束', Boolean(replay.ok) && replay.ok.sizeBytes === expected.length, { replay, observed });
    const bytes = await readFile(join(managedDir, name));
    check('ディスク上の内容は完全（途中までの書き込みが残らない）', bytes.length === expected.length && sha256(bytes) === sha256(expected), bytes.length);
    const namesAfter = await readdir(managedDir);
    check('管理フォルダーに余分なファイルが残らない', namesAfter.sort().join('/') === [...namesBefore, name].sort().join('/'), namesAfter);
    await (await s.waitFor(() => s.find('button[aria-label="第二フォルダーを開く"]'))).click();
    check('強制終了後も追加フォルダーを閲覧できる', await s.waitForText('table', '第二の資料.txt'));
  } finally {
    await quit(s);
  }
});

scenario('多数の項目：100件ごとのページ送り（次・前）', async () => {
  const s = await launch(main);
  try {
    await openWorkspace(s, workspaceName);
    await (await s.waitFor(() => s.find('button[aria-label="第二フォルダーを開く"]'))).click();
    await clickText(s, 'table button', '多数');
    const names = () => s.execute('return [...document.querySelectorAll("table tbody tr td:first-child")].map((td) => td.textContent.trim());');
    const page1 = await s.waitFor(async () => { const value = await names(); return value.length === 100 && value[0].startsWith('item-') ? value : undefined; }, { message: 'first page' });
    check('1ページ目は100件', page1.length === 100);
    await clickText(s, 'button', '次の100件');
    const page2 = await s.waitFor(async () => { const value = await names(); return value.length === 100 && value[0] !== page1[0] ? value : undefined; }, { message: 'second page' });
    await clickText(s, 'button', '次の100件');
    const page3 = await s.waitFor(async () => { const value = await names(); return value.length === 30 ? value : undefined; }, { message: 'last page' });
    check('最後のページは30件で「次の100件」は出ない', (await s.findAll('button')).length > 0 && !(await s.bodyText()).includes('次の100件'));
    await shot(s, 'paging');
    const all = [...page1, ...page2, ...page3];
    const expected = Array.from({ length: 230 }, (_, i) => `item-${String(i).padStart(3, '0')}.txt`);
    check('3ページで230件すべてを重複なく表示', new Set(all).size === 230 && expected.every((name) => all.includes(name)), { total: all.length, distinct: new Set(all).size });
    await clickText(s, 'button', '前の100件');
    const back = await s.waitFor(async () => { const value = await names(); return value.length === 100 ? value : undefined; }, { message: 'previous page' });
    check('「前の100件」で2ページ目に戻る', back.join('/') === page2.join('/'));
  } finally {
    await quit(s);
  }
});

scenario('利用中のフォルダーの解除：取消・解除・管理フォルダーは解除できない', async () => {
  const s = await launch(main);
  try {
    await openWorkspace(s, workspaceName);
    await clickText(s, 'button', 'フォルダーを追加');
    await X.chooseFolder(folders.third);
    check('第三フォルダーを追加', await notice(s, 'フォルダー「第三フォルダー」を追加しました。'));
    const attached = (await workspaces(s)).find((item) => item.name === workspaceName);
    const third = attached.bindings.find((item) => item.label === '第三フォルダー');
    await (await s.find('button[aria-label="第三フォルダーを解除"]')).click();
    await s.waitForText('[role="dialog"]', '「第三フォルダー」をこのWorkspaceから外します。');
    await clickText(s, '[role="dialog"] button', 'キャンセル');
    await s.waitFor(async () => (await s.findAll('[role="dialog"]')).length === 0, { message: 'dialog closed' });
    check('取消では解除しない', (await workspaces(s)).find((item) => item.name === workspaceName).bindings.some((item) => item.bindingId === third.bindingId));
    await s.waitFor(async () => (await s.execute('return document.activeElement.getAttribute("aria-label");')) === '第三フォルダーを解除', { message: 'focus back' });
    check('取消後は「解除」ボタンへfocusが戻る', true);
    await (await s.find('button[aria-label="第三フォルダーを解除"]')).click();
    await clickText(s, '[role="dialog"] button', '解除する');
    check('利用可能なフォルダーの解除', await notice(s, 'フォルダー「第三フォルダー」を解除しました。フォルダーの中身はそのままです。'));
    const detached = (await workspaces(s)).find((item) => item.name === workspaceName);
    check('解除後は一覧から消える', !detached.bindings.some((item) => item.bindingId === third.bindingId));
    const stale = await invoke(s, 'entries.list', { context: contextOf(detached), ref: { bindingId: third.bindingId, locator: [] } });
    check('IPC：解除したbindingIdではもう読めない', Boolean(stale.err), stale);
    check('フォルダーの中身はそのまま', (await readFile(join(folders.third, 'third.txt'), 'utf8')) === '第三フォルダーの合成ファイルです。\n');
    check('管理フォルダーには「解除」ボタンがない', (await s.findAll('button[aria-label="管理フォルダーを解除"]')).length === 0);
    const managed = await invoke(s, 'directory.detach', { context: contextOf(detached), bindingId: detached.managedBindingId, operationId: randomUUID() });
    check('IPC：管理フォルダーの解除はmanaged_bindingで拒否', managed.err?.reason === 'managed_binding', managed);
    await shot(s, 'detached');
  } finally {
    await quit(s);
  }
});

scenario('二重起動：2つ目のアプリは同じ記録を使わない', async () => {
  const first = await launch(main);
  const secondDriver = await startDriver('second', appEnvironment({ apiOrigin: backend.origin }));
  let second;
  try {
    await gotoLocalWorkspaces(first);
    second = await Session.create(secondDriver.url, binary);
    await second.waitFor(async () => (await second.url()).startsWith('tauri://localhost/'), { message: 'second app' });
    const reply = await invoke(second, 'workspace.list', null);
    check('IPC：2つ目のプロセスのbrokerはinstance_lockedで止まる', reply.err?.reason === 'instance_locked', reply);
    await gotoLocalWorkspaces(second).catch(() => undefined);
    await second.waitForText('h1', 'ローカルWorkspace');
    check('2つ目の画面は理由（別に起動中）を表示', await alertText(second, 'デスクトップ版が別に起動しています。もう一方を終了してから開き直してください。'));
    check('2つ目の画面には作成などの操作が出ない', !(await second.bodyText()).includes('新しいWorkspace'));
    await shot(second, 'second-instance');
    check('1つ目は引き続き利用できる', (await workspaces(first)).some((item) => item.name === workspaceName));
  } finally {
    await second?.delete();
    secondDriver.stop();
    await quit(first);
  }
});

scenario('reduced motion：GTKのアニメーション無効設定がページへ伝わる', async () => {
  const settings = join(home, '.config/gtk-3.0/settings.ini');
  await mkdir(dirname(settings), { recursive: true });
  await writeFile(settings, '[Settings]\ngtk-enable-animations=false\n');
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    const motion = await s.execute('return { reduce: matchMedia("(prefers-reduced-motion: reduce)").matches, fast: getComputedStyle(document.documentElement).getPropertyValue("--motion-fast").trim(), scroll: getComputedStyle(document.documentElement).scrollBehavior };');
    check('prefers-reduced-motion: reduce が成立', motion.reduce === true, motion);
    check('motion tokenが0msになる', motion.fast === '0ms', motion);
    await shot(s, 'reduced-motion');
  } finally {
    await quit(s);
    await rm(settings);
  }
});

scenario('XDGのダウンロード先が無い端末でも、原本は ~/Downloads へ保存される', async () => {
  const bare = join(directory, 'home-without-xdg-dirs');
  for (const path of ['.config', '.local/share', '.cache', '.runtime', 'Downloads']) await mkdir(join(bare, path), { recursive: true, mode: 0o700 });
  const driver = await startDriver('no-xdg-dirs', appEnvironment({ apiOrigin: backend.origin, homeDir: bare }));
  const s = await Session.create(driver.url, binary);
  try {
    await s.waitFor(async () => (await s.url()).startsWith('tauri://localhost/'), { message: 'app' });
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    check('user-dirs.dirsが無い', !(await stat(join(bare, '.config/user-dirs.dirs')).catch(() => undefined)));
    await (await s.waitForText('button', 'ファイルを取得')).click();
    const saved = join(bare, 'Downloads', 'desktop-reference.txt');
    await s.waitFor(async () => (await stat(saved).catch(() => undefined))?.size > 0, { message: 'fallback download' });
    check('~/Downloads に保存され、内容が一致', (await readFile(saved, 'utf8')) === '【合成データ】デスクトップ版の画面確認に使う共有資料です。\n');
    // tauri-driver (and so the app) runs in the repository root.
    check('作業フォルダーには保存しない', !(await readdir(root)).includes('desktop-reference.txt'));
  } finally {
    await s.delete();
    driver.stop();
  }
});

scenario('backend停止中：文書画面は失敗を表示し、ローカル機能は使える', async () => {
  const s = await launch(main);
  try {
    await s.waitForText('[role="row"]', 'デスクトップ確認用資料');
    await backend.stop();
    backend = undefined;
    await s.execute('window.location.assign("/documents")');
    check('API失敗を画面に表示（クラッシュしない）', await s.waitForText('[role="alert"]', '読み込みに失敗しました', { timeout: 30_000 }));
    const api = await pageFetch(s, '/v1/organization/session');
    check('/v1は502 problem（詳細を出さない）', api.status === 502 && !api.body.includes('127.0.0.1'), api);
    await shot(s, 'backend-down');
    await gotoLocalWorkspaces(s);
    check('ローカルWorkspaceはbackend無しで利用できる', await s.waitForText('nav[aria-labelledby="local-workspace-list-title"] button', workspaceName));
  } finally {
    await quit(s);
  }
});

scenario('接続先が未設定のshell：/v1は503で、外部へは出ない', async () => {
  const driver = await startDriver('no-origin', appEnvironment({ apiOrigin: undefined }));
  const s = await launch(driver);
  try {
    const api = await pageFetch(s, '/v1/organization/session');
    check('/v1は503 problem', api.status === 503 && api.body.includes('サーバーの接続先が設定されていません。'), api);
    check('文書画面は失敗を表示', await s.waitForText('[role="alert"]', '読み込みに失敗しました', { timeout: 30_000 }));
    check('起動時にstderrへ1行だけ理由を出す', (await readFile(driver.logFile, 'utf8')).split('\n').filter((line) => line.includes('KNOWLEDGE_PLATFORM_API_ORIGIN is not set')).length === 1);
    await shot(s, 'no-origin');
  } finally {
    await quit(s);
    driver.stop();
  }
});

scenario('接続先の形式が不正なshell（loopback以外・path付き）：/v1は503で、その接続先へは送らない', async () => {
  for (const [name, origin] of [['non-loopback', sentinel.origin.replace('127.0.0.1', 'localhost')], ['with-path', `${sentinel.origin}/v1`]]) {
    const before = sentinel.hits.length;
    const driver = await startDriver(`invalid-origin-${name}`, appEnvironment({ apiOrigin: origin }));
    const s = await launch(driver);
    try {
      const api = await pageFetch(s, '/v1/organization/session');
      check(`${name}：/v1は503で形式を案内`, api.status === 503 && api.body.includes('サーバーの接続先の形式が正しくありません'), api);
      check(`${name}：不正な接続先へは何も送らない`, sentinel.hits.length === before, sentinel.hits.slice(before));
      check(`${name}：起動時にstderrへ形式の理由を出す`, (await readFile(driver.logFile, 'utf8')).includes('must be exactly http://127.0.0.1:<port>'));
      await shot(s, `invalid-origin-${name}`);
    } finally {
      await quit(s);
      driver.stop();
    }
  }
});
