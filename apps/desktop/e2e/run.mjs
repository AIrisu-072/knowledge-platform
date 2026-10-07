#!/usr/bin/env node
// TEST-ONLY: drives the REAL desktop app (Tauri shell + WebKitGTK + the bundled
// production React build + the local Workspace broker) through its GUI:
// tauri-driver -> WebKitWebDriver, a private Xvfb display, xdotool for the
// native GTK folder dialog, and a real synthetic backend (PostgreSQL 18.6 +
// organization-server) reached only through the shell's /v1 forwarding.
// Local developer verification only; it is intentionally not a CI job.
// This is Linux evidence. It is NOT Windows/WebView2 evidence.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { spawn, execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { access, link, mkdir, mkdtemp, readFile, readdir, rename, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
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

const base = resolve(process.env.KP_DESKTOP_EVIDENCE_DIR ?? join(root, 'apps/desktop/e2e/.state'));
let directory, display, X, backend, home, fixtures, stateRoot;
const drivers = [];
const report = { schemaVersion: 1, scope: 'Linux desktop GUI (Tauri 2.12.1 + WebKitGTK) against a real synthetic backend; not Windows/WebView2 evidence', scenarios: [] };
let current;

function scenario(name, body) {
  test(name, async () => {
    current = { name, status: 'running', checks: [], screenshots: [] };
    report.scenarios.push(current);
    try {
      await body();
      current.status = 'passed';
    } catch (error) {
      current.status = 'failed';
      current.error = String(error?.stack ?? error).slice(0, 2000);
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

async function saveReport() {
  if (directory) await writeFile(join(directory, 'report.json'), `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });
}

async function shot(session, name) {
  const file = join(directory, `${String(report.scenarios.length).padStart(2, '0')}-${name}.png`);
  await writeFile(file, await session.screenshot());
  current.screenshots.push(file.slice(directory.length + 1));
}

async function rootShot(name) {
  const file = join(directory, `${String(report.scenarios.length).padStart(2, '0')}-${name}-screen.png`);
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
  const log = createWriteStream(join(directory, `driver-${name}.log`), { mode: 0o600 });
  const child = spawn(tauriDriver, ['--port', String(port), '--native-port', String(nativePort), '--native-driver', webkitDriver], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  const url = `http://127.0.0.1:${port}`;
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error(`tauri-driver ${name} exited`);
    try { if ((await fetch(`${url}/status`)).ok) break; } catch { /* starting */ }
    await delay(100);
  }
  const driver = { name, url, stop: () => child.kill('SIGTERM') };
  drivers.push(driver);
  return driver;
}

async function launch(driver) {
  const session = await Session.create(driver.url, binary);
  await session.waitFor(async () => (await session.url()).startsWith('tauri://localhost/'), { message: 'the bundled app URL' });
  return session;
}

/** Ends the session (the app exits) and waits for the broker's instance lock. */
async function quit(session) {
  // End-state evidence for every scenario (also shows the screen on failure).
  await shot(session, 'end').catch(() => undefined);
  await session.delete();
  const lock = join(stateRoot, '.lock');
  for (let i = 0; i < 100; i++) {
    try { await run('flock', ['-n', lock, 'true']); return; } catch { await delay(100); }
  }
  throw new Error('the app did not release its instance lock after exit');
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

function pageFetch(session, url, init = {}) {
  return session.executeAsync(`const done = arguments[arguments.length - 1];
    fetch(arguments[0], arguments[1]).then(async (r) => done({ status: r.status, type: r.headers.get('content-type'),
      nosniff: r.headers.get('x-content-type-options'), cookie: r.headers.get('set-cookie'), body: (await r.text()).slice(0, 400) }),
      (e) => done({ error: String(e) }));`, [url, init]);
}

async function clickText(session, css, text) {
  const element = await session.waitForText(css, text);
  await element.click();
  return element;
}

async function notice(session, text) {
  return session.waitForText('p[role="status"]', text);
}

async function alertText(session, text) {
  return session.waitForText('[role="alert"]', text);
}

async function gotoLocalWorkspaces(session) {
  await clickText(session, 'header a', 'ローカルWorkspace');
  await session.waitForText('h1', 'ローカルWorkspace');
  await session.waitFor(async () => (await session.bodyText()).includes('管理フォルダー') || (await session.bodyText()).includes('Workspaceはまだありません'), { message: 'workspace list' });
}

async function workspaces(session) {
  const reply = await invoke(session, 'workspace.list', null);
  assert.ok(reply.ok, JSON.stringify(reply));
  return reply.ok;
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

async function createFixtures() {
  const folder = join(fixtures, '資料フォルダー');
  const outside = join(fixtures, '外部の場所');
  await mkdir(join(folder, 'sub'), { recursive: true });
  await mkdir(outside, { recursive: true });
  await mkdir(join(fixtures, '第二フォルダー'), { recursive: true });
  await writeFile(join(folder, 'readme.txt'), '合成データの説明文です。\n');
  await writeFile(join(folder, 'sub', 'nested.txt'), '下の階層の合成ファイルです。\n');
  await writeFile(join(folder, '大きいファイル.txt'), 'a'.repeat(1536 * 1024));
  await writeFile(join(outside, 'secret.txt'), '外部の合成ファイル（読めてはいけない）\n');
  await writeFile(join(outside, 'linked-origin.txt'), 'ハードリンク元（合成）\n');
  await link(join(outside, 'linked-origin.txt'), join(folder, '二重リンク.txt'));
  await symlink(outside, join(folder, '外部へのリンク'));
  await writeFile(join(fixtures, '第二フォルダー', '第二の資料.txt'), '第二フォルダーの合成ファイルです。\n');
  return { folder, outside, second: join(fixtures, '第二フォルダー') };
}

let main, folders, workspaceName, docId, replayOp;

before(async () => {
  await mkdir(base, { recursive: true, mode: 0o700 });
  directory = await mkdtemp(join(base, 'run-'));
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
  const driverVersion = (await run(tauriDriver, ['--version']).catch(() => ({ stdout: 'unknown' }))).stdout.trim();
  report.environment = {
    gitHead: head, worktreeDirty: dirty, binarySha256: sha256(await readFile(binary)), webkit2gtk: webkit, tauriDriver: driverVersion,
    webkitWebDriver: webkitDriver, node: process.version, startedAt: new Date().toISOString(),
  };
  display = await startXvfb(`:${90 + Math.floor(Math.random() * 400)}`);
  X = x11(display.env);
  backend = await startBackend({ root, directory, pdfium });
  report.environment.backend = { postgres: backend.postgres, server: 'organization-server (sales-01 synthetic profile)', origin: 'loopback' };
  docId = backend.documentId;
  main = await startDriver('main', appEnvironment({ apiOrigin: backend.origin }));
  await saveReport();
});

after(async () => {
  for (const driver of drivers) driver.stop();
  await backend?.stop().catch(() => undefined);
  display?.stop();
  report.finishedAt = new Date().toISOString();
  report.status = report.scenarios.every((item) => item.status === 'passed') ? 'passed' : 'failed';
  await saveReport();
  console.log(`desktop GUI evidence: ${directory}`);
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

scenario('Router・Query：deep link（/tasks）、タスク・検索・文書のナビゲーション', async () => {
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
    await shot(s, 'tasks');
    await clickText(s, 'nav a', '検索');
    await s.waitFor(async () => new URL(await s.url()).pathname === '/search', { message: '/search' });
    check('検索画面へ遷移（Search内部は変更しない）', await s.waitForText('h1', '検索'));
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
    await s.execute('document.activeElement && document.activeElement.blur(); window.focus();');
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

scenario('Document API転送：上り下りのbody完全性、原本ダウンロード、境界', async () => {
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
    const escape = await pageFetch(s, '/v1/../health/ready');
    check('/v1/../health はshellの外へ転送されない（HTMLが返る）', escape.status === 200 && !escape.body.includes('"status":"ok"') && (escape.type ?? '').includes('text/html'), escape);
    const encoded = await pageFetch(s, '/v1/%2e%2e/health/ready');
    check('%2e%2e による脱出も転送されない', !encoded.body.includes('"status":"ok"'), encoded);
    const options = await pageFetch(s, '/v1/organization/session', { method: 'OPTIONS' });
    check('OPTIONSは405（転送しない）', options.status === 405, options);
    const direct = await pageFetch(s, `${backend.origin}/v1/organization/session`);
    check('ページからbackendへの直接接続はCSPで遮断', Boolean(direct.error), direct);

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

scenario('GUIでの文書登録：ファイル選択→multipart上り（shell経由）→登録結果', async () => {
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
    const documentId = new URL(await s.url()).pathname.split('/').at(-1);
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
    })().then(done, (e) => done({ error: String(e) }));`, [documentId]);
    check('登録した原本をAPIから取得すると内容が一致', roundTrip.found && roundTrip.text === original, { found: roundTrip.found, name: roundTrip.name, error: roundTrip.error, titles: roundTrip.titles, length: roundTrip.text?.length });
  } finally {
    await quit(s);
  }
});

scenario('ローカルWorkspace：作成・名前変更・管理フォルダーへの作成と読み取り', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    check('実行環境がデスクトップ版・各機能が利用可', (await s.bodyText()).includes('デスクトップ版') && !(await s.bodyText()).includes('利用できません'));
    await clickText(s, 'button', '新しいWorkspace');
    await (await s.waitFor(() => s.find('input[aria-label="Workspace名"]'))).type('案件A（合成）');
    await clickText(s, 'button[type="submit"]', '作成する');
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
    await (await s.waitFor(() => s.find('button[aria-label="管理フォルダーを開く"]'), { message: '管理フォルダーを開く' })).click();
    await s.waitForText('p', 'このフォルダーには表示できる項目がありません。');
    await (await s.find('input[aria-label="ファイル名"]')).type('メモ.txt');
    await (await s.find('textarea[aria-label="内容"]')).type('合成のメモです。');
    await clickText(s, 'form[aria-label="この場所にファイルを作成"] button[type="submit"]', '作成する');
    check('ファイル作成の通知', await notice(s, 'ファイル「メモ.txt」を作成しました。'));
    const onDisk = await readFile(join(stateRoot, 'managed', managedDirs[0], 'メモ.txt'), 'utf8');
    check('ディスク上の管理フォルダーに同じ内容で作成', onDisk === '合成のメモです。', onDisk);
    await (await s.waitFor(() => s.find('button[aria-label="メモ.txt の内容を表示"]'))).click();
    check('内容の表示（限定read）', await s.waitForText('[role="region"] pre', '合成のメモです。'));
    await (await s.find('input[aria-label="ファイル名"]')).type('メモ.txt');
    await clickText(s, 'form[aria-label="この場所にファイルを作成"] button[type="submit"]', '作成する');
    check('同名は上書きせず拒否', await alertText(s, '同じ名前のファイルが既にあります'));
    for (const bad of ['../escape.txt', 'CON', 'a/b.txt']) {
      const input = await s.find('input[aria-label="ファイル名"]');
      await input.clear();
      await input.type(bad);
      await clickText(s, 'form[aria-label="この場所にファイルを作成"] button[type="submit"]', '作成する');
      check(`不正な名前「${bad}」を拒否`, await alertText(s, '名前に使えない文字または予約名が含まれています。'), bad);
    }
    check('管理フォルダーの外にファイルが作られていない', !(await readdir(dirname(stateRoot))).includes('escape.txt'));
    check('画面に絶対pathが出ない', !(await s.bodyText()).includes(home), workspace?.workspaceId);
    await shot(s, 'managed-folder');
  } finally {
    await quit(s);
  }
});

scenario('ネイティブのフォルダー選択（実GTKダイアログ）：取消・選択・二重選択・同じフォルダー', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    await s.waitForText('h2', workspaceName);
    const before = (await workspaces(s)).find((item) => item.name === workspaceName);
    await clickText(s, 'button', 'フォルダーを追加');
    await X.waitDialog(true);
    await rootShot('native-dialog');
    const busy = await invoke(s, 'directory.choose', { context: { workspaceId: before.workspaceId, effectiveContextRevision: before.effectiveContextRevision } });
    check('ダイアログ表示中の2つ目の選択要求はpicker_busy', busy.err?.reason === 'picker_busy', busy);
    await X.cancelDialog();
    check('取消は変更なしの通知', await notice(s, 'フォルダーの選択を取り消しました。変更はありません。'));
    check('取消後もフォルダー数は不変', (await workspaces(s)).find((item) => item.name === workspaceName).bindings.length === before.bindings.length);
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

scenario('追加フォルダーの閲覧：一覧・階層移動・1MiBまでの表示・リンク拒否・作成', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    await (await s.waitFor(() => s.find('button[aria-label="資料フォルダーを開く"]'), { message: '資料フォルダーを開く' })).click();
    const table = await s.waitForText('table', 'readme.txt');
    const text = await table.text();
    check('通常ファイルとフォルダーを一覧', text.includes('readme.txt') && text.includes('sub') && text.includes('大きいファイル.txt'), text);
    check('symlinkは一覧に出さない', !text.includes('外部へのリンク'), text);
    check('表示できない項目の件数を表示', await s.waitForText('p', '表示できない項目が'));
    await (await s.find('button[aria-label="readme.txt の内容を表示"]')).click();
    check('テキストを表示', await s.waitForText('[role="region"] pre', '合成データの説明文です。'));
    await (await s.find('button[aria-label="大きいファイル.txt の内容を表示"]')).click();
    check('1MiBを超えるファイルは先頭1MiBのみ', await s.waitForText('[role="region"] p', '（先頭1MiBのみ表示）'));
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
    await clickText(s, 'form[aria-label="この場所にファイルを作成"] button[type="submit"]', '作成する');
    await notice(s, 'ファイル「デスクトップから作成.txt」を作成しました。');
    check('選択フォルダー内に作成', (await readFile(join(folders.folder, 'デスクトップから作成.txt'), 'utf8')) === '選択フォルダーへの作成（合成）');
    check('外部の場所には何も作られていない', (await readdir(folders.outside)).sort().join(',') === 'linked-origin.txt,secret.txt');
    await shot(s, 'browse');
  } finally {
    await quit(s);
  }
});

scenario('ページscriptからの不正なIPC・遷移・新規ウィンドウ', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
    const workspace = (await workspaces(s)).find((item) => item.name === workspaceName);
    const context = { workspaceId: workspace.workspaceId, effectiveContextRevision: workspace.effectiveContextRevision };
    const explicit = workspace.bindings.find((item) => item.source === 'explicit');
    for (const locator of [['..'], ['/etc'], ['a/b'], ['C:\\Windows'], ['sub', '..', '..'], ['']]) {
      const reply = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator } });
      check(`locator ${JSON.stringify(locator)} は拒否`, reply.err?.code === 'invalid_locator', reply);
    }
    const link = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator: ['外部へのリンク'] } });
    check('symlinkを名指ししても辿らない', link.err?.reason === 'symbolic_link', link);
    const openLink = await invoke(s, 'file.openRead', { context, ref: { bindingId: explicit.bindingId, locator: ['外部へのリンク', 'secret.txt'] }, expectedFileIdentity: 'x' });
    check('symlink配下のファイルも開けない', Boolean(openLink.err) && !JSON.stringify(openLink).includes('外部の合成ファイル'), openLink);
    const forged = await invoke(s, 'entries.list', { context, ref: { bindingId: 'b_forged', locator: [] } });
    check('偽のbindingIdは拒否', Boolean(forged.err), forged);
    const unknown = await invoke(s, 'shell.execute', { program: '/bin/sh' });
    check('未知のcommandは拒否', unknown.err?.reason === 'unknown_command', unknown);
    const extra = await invoke(s, 'entries.list', { context, ref: { bindingId: explicit.bindingId, locator: [] }, path: '/etc' });
    check('余分なfield（path等）は拒否', extra.err?.reason === 'invalid_request', extra);
    for (const [name, args] of [['plugin:fs|read_text_file', { path: '/etc/passwd' }], ['plugin:shell|execute', { program: 'sh' }], ['plugin:window|create', { options: { label: 'x' } }], ['plugin:webview|create_webview_window', { options: { label: 'x', url: 'https://example.com' } }]]) {
      const reply = await invokeRaw(s, name, args);
      check(`${name} は利用できない`, Boolean(reply.err), reply);
    }
    const opened = await s.execute('return window.open("https://example.com/") === null;');
    await delay(500);
    check('window.openで新しいウィンドウは開かない', opened === true && (await s.handles()).length === 1, await s.handles());
    await s.execute('window.location.assign("https://example.com/");');
    await delay(1500);
    check('外部URLへの遷移は拒否され、アプリに留まる', (await s.url()).startsWith('tauri://localhost/'), await s.url());
    check('画面とIPC応答に絶対pathが出ない', !(await s.bodyText()).includes(fixtures));
  } finally {
    await quit(s);
  }
});

scenario('置き換え・脱出への対応と解除（中身は残る）', async () => {
  const s = await launch(main);
  try {
    await gotoLocalWorkspaces(s);
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

scenario('再起動後の復元と、同じ操作IDの再送（結果不明からの確認）', async () => {
  let s = await launch(main);
  let first;
  try {
    await gotoLocalWorkspaces(s);
    const workspace = (await workspaces(s)).find((item) => item.name === workspaceName);
    const context = { workspaceId: workspace.workspaceId, effectiveContextRevision: workspace.effectiveContextRevision };
    replayOp = randomUUID();
    const request = { context, parent: { bindingId: workspace.managedBindingId, locator: [] }, name: '再送確認.txt', bytesBase64: Buffer.from('同じ操作（合成）').toString('base64'), operationId: replayOp };
    first = await invoke(s, 'file.create', request);
    check('作成（応答を受け取れなかった想定の1回目）', Boolean(first.ok), first);
    const createOp = randomUUID();
    const [a, b] = await Promise.all([invoke(s, 'workspace.create', { name: '同時作成（合成）', operationId: createOp }), invoke(s, 'workspace.create', { name: '同時作成（合成）', operationId: createOp })]);
    check('同じ操作IDの同時作成は1つのWorkspaceに収束', a.ok && b.ok && a.ok.workspace.workspaceId === b.ok.workspace.workspaceId, { a, b });
    check('同時作成で重複Workspaceはできない', (await workspaces(s)).filter((item) => item.name === '同時作成（合成）').length === 1);
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
    const context = { workspaceId: workspace.workspaceId, effectiveContextRevision: workspace.effectiveContextRevision };
    const request = { context, parent: { bindingId: workspace.managedBindingId, locator: [] }, name: '再送確認.txt', bytesBase64: Buffer.from('同じ操作（合成）').toString('base64'), operationId: replayOp };
    const replay = await invoke(s, 'file.create', request);
    check('再起動後に同じ操作IDで再送すると同じ結果（二重作成なし）', replay.ok && replay.ok.fileIdentity === first.ok.fileIdentity && replay.ok.sha256 === first.ok.sha256, { replay, first });
    const mismatch = await invoke(s, 'file.create', { ...request, bytesBase64: Buffer.from('別の内容').toString('base64') });
    check('同じ操作IDで内容が違えばoperation_mismatch', mismatch.err?.reason === 'operation_mismatch', mismatch);
    const managed = (await readdir(join(stateRoot, 'managed')));
    const files = (await Promise.all(managed.map((dir) => readdir(join(stateRoot, 'managed', dir))))).flat();
    check('ディスク上も1ファイルだけ', files.filter((name) => name === '再送確認.txt').length === 1, files);
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
    check('2つ目のプロセスのbrokerはinstance_lockedで止まる', reply.err?.reason === 'instance_locked', reply);
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
    await shot(s, 'no-origin');
  } finally {
    await quit(s);
    driver.stop();
  }
});
