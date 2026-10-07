// TEST-ONLY: a private Xvfb display and xdotool/xclip control of the real
// native GTK folder dialog that the desktop shell opens (rfd). No window manager.
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { writeFile } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';

const run = promisify(execFile);
export const DIALOG_TITLE = '追加するフォルダーを選択';

/**
 * Starts a private Xvfb on a display number that Xvfb itself picks as free
 * (-displayfd), so the run never attaches to an X server that already exists.
 */
export async function startXvfb() {
  const child = spawn('Xvfb', ['-displayfd', '3', '-screen', '0', '1440x900x24', '-nolisten', 'tcp'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe'] });
  const stop = () => { if (child.exitCode === null && !child.signalCode) child.kill('SIGTERM'); };
  const number = await new Promise((resolve, reject) => {
    let text = '';
    const timer = setTimeout(() => reject(new Error('Xvfb did not report its display')), 20_000);
    child.stdio[3].on('data', (chunk) => {
      text += chunk;
      const match = /^(\d+)\n/.exec(text);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    });
    child.once('exit', () => { clearTimeout(timer); reject(new Error('Xvfb exited during startup')); });
  }).catch((error) => { stop(); throw error; });
  const display = `:${number}`;
  const env = { ...process.env, DISPLAY: display, LC_ALL: 'C.UTF-8' };
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error('Xvfb exited during startup');
    try { await run('xdotool', ['getdisplaygeometry'], { env }); return { display, env, pid: child.pid, stop }; } catch { await delay(100); }
  }
  stop();
  throw new Error('Xvfb did not become ready');
}

export function x11(env) {
  const xdo = async (...args) => (await run('xdotool', args, { env })).stdout.trim();
  // xclip forks a child that serves the selection until another client owns it
  // (or the private display ends); the parent exits once the text is read.
  const setClipboard = (text) => new Promise((resolve, reject) => {
    const child = spawn('xclip', ['-selection', 'clipboard'], { env, stdio: ['pipe', 'ignore', 'ignore'] });
    child.once('error', reject);
    child.once('exit', (code) => (code === 0 ? resolve() : reject(new Error(`xclip exited ${code}`))));
    child.stdin.end(text);
  });
  const tryXdo = (...args) => xdo(...args).catch(() => '');
  async function windows() {
    const ids = (await tryXdo('search', '--onlyvisible', '--name', '')).split('\n').filter(Boolean);
    const named = [];
    for (const id of ids) named.push({ id, name: await tryXdo('getwindowname', id) });
    return named;
  }
  async function dialog() {
    return (await windows()).find((window) => window.name === DIALOG_TITLE)?.id;
  }
  async function waitDialog(open, timeout = 20_000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      const id = await dialog();
      if (open ? id : !id) return id;
      await delay(150);
    }
    throw new Error(`native folder dialog did not ${open ? 'open' : 'close'}`);
  }
  return {
    windows,
    dialog,
    waitDialog,
    /** Full X screen (includes the native dialog, which WebDriver cannot see). */
    async screenshot(path) {
      const { stdout } = await run('import', ['-window', 'root', 'png:-'], { env, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
      await writeFile(path, stdout);
    },
    /**
     * Selects `folder` in the open GTK chooser through its location entry. The
     * path is pasted, not typed: `xdotool type` can drop a non-ASCII character
     * (it remaps a spare keycode per character), and the chooser would then
     * create and return a different folder.
     */
    async chooseFolder(folder) {
      const id = await waitDialog(true);
      await xdo('windowfocus', '--sync', id);
      // Leave the empty "Recent" view first; there the entry cannot resolve.
      await xdo('key', '--clearmodifiers', 'alt+Home');
      await delay(600);
      await xdo('key', '--clearmodifiers', 'ctrl+l');
      await delay(300);
      await setClipboard(folder);
      await xdo('key', '--clearmodifiers', 'ctrl+a');
      await xdo('key', '--clearmodifiers', 'ctrl+v');
      await delay(500);
      // Drop GTK's inline completion (it would append a child folder).
      await xdo('key', 'Delete');
      await delay(200);
      await xdo('key', 'Return');
      await waitDialog(false);
    },
    async cancelDialog() {
      const id = await waitDialog(true);
      await xdo('windowfocus', '--sync', id);
      await xdo('key', 'Escape');
      await waitDialog(false);
    },
  };
}
