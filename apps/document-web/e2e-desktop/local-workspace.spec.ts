import { mkdtemp, mkdir, readdir, readFile, rm, symlink, link, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { Bridge, wireDesktop } from './bridge';

// Browser E2E of the same React frontend with the desktop adapter and the real
// broker behind the shared wire dispatcher. Not Tauri/WebView2/Windows proof.

let root: string;
let bridge: Bridge;

test.beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), 'kp-desktop-e2e-'));
  bridge = new Bridge(join(root, 'state'));
});

test.afterEach(async () => {
  await bridge.stop();
  await rm(root, { recursive: true, force: true });
});

async function createWorkspace(page: import('@playwright/test').Page, name: string) {
  await page.getByRole('button', { name: '新しいWorkspace' }).click();
  const dialog = page.getByRole('dialog', { name: '新しいWorkspace' });
  await dialog.getByRole('textbox', { name: 'Workspace名' }).fill(name);
  await dialog.getByRole('button', { name: '作成する' }).click();
  await expect(dialog).toBeHidden();
}

test('keyboard-only creation makes a managed root unrelated to the name and survives restart', async ({ page }) => {
  await wireDesktop(page, () => bridge);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/local-workspaces');
  await expect(page.getByRole('heading', { name: 'ローカルWorkspace', level: 1 })).toBeVisible();
  await expect(page.getByRole('banner').getByText('デスクトップで実行中')).toBeVisible();
  await expect(page.getByText('デスクトップ版')).toBeVisible();

  const trigger = page.getByRole('button', { name: '新しいWorkspace' });
  await trigger.focus();
  await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: '新しいWorkspace' });
  await expect(dialog.getByRole('textbox', { name: 'Workspace名' })).toBeFocused();
  // Escape cancels without a request and restores focus.
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  expect(bridge.callsOf('workspace.create')).toHaveLength(0);

  await page.keyboard.press('Enter');
  await page.keyboard.type('../見積/2026');
  await page.keyboard.press('Enter');
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(page.getByRole('status')).toHaveText('Workspace「../見積/2026」を作成しました。');
  const managed = await readdir(join(root, 'state', 'managed'));
  expect(managed).toHaveLength(1);
  expect(managed[0]).toMatch(/^b_[0-9a-f]{32}$/);
  expect(await readdir(root)).toEqual(['state']);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--motion-spatial').trim())).toBe('0ms');

  // Restart: a new broker process on the same state restores the workspace.
  await bridge.stop();
  bridge = new Bridge(join(root, 'state'));
  await page.reload();
  await expect(page.getByRole('button', { name: '../見積/2026' })).toHaveAttribute('aria-current', 'true');
  await expect(page.getByRole('list', { name: '../見積/2026のフォルダー' }).getByText('管理フォルダー', { exact: true })).toBeVisible();
});

test('picker cancel sends nothing; a chosen folder is browsed, read and extended without escape', async ({ page }) => {
  const folder = join(root, '営業資料');
  const outside = join(root, 'outside');
  await mkdir(join(folder, 'sub'), { recursive: true });
  await mkdir(outside);
  await writeFile(join(outside, 'secret.txt'), 'secret');
  await writeFile(join(folder, 'readme.txt'), 'こんにちは');
  await writeFile(join(folder, 'sub', 'inner.txt'), 'inner');
  await symlink(outside, join(folder, 'link-out'));
  await link(join(outside, 'secret.txt'), join(folder, 'hard.txt'));
  await wireDesktop(page, () => bridge);
  await page.goto('/local-workspaces');
  await createWorkspace(page, '案件');

  const add = page.getByRole('button', { name: 'フォルダーを追加' });
  await bridge.pick(null);
  await add.click();
  await expect(page.getByRole('status')).toHaveText('フォルダーの選択を取り消しました。変更はありません。');
  expect(bridge.callsOf('directory.attach')).toHaveLength(0);

  await bridge.pick(folder);
  await add.click();
  await expect(page.getByRole('list', { name: '案件のフォルダー' }).getByText('営業資料')).toBeVisible();
  expect(bridge.callsOf('directory.attach')).toHaveLength(1);

  await page.getByRole('button', { name: '営業資料を開く' }).click();
  const table = page.getByRole('table', { name: '営業資料の内容' });
  await expect(table.getByRole('button', { name: 'sub' })).toBeVisible();
  await expect(table.getByText('link-out')).toHaveCount(0);
  await expect(page.getByText('表示できない項目が1件あります（リンク・特殊なファイル・使用できない名前）。')).toBeVisible();
  await table.getByRole('button', { name: 'readme.txt の内容を表示' }).click();
  await expect(page.getByRole('region', { name: 'readme.txt の内容' })).toContainText('こんにちは');
  await table.getByRole('button', { name: 'hard.txt の内容を表示' }).click();
  await expect(page.getByText('複数の場所にリンクされたファイルは安全のため開けません。')).toBeVisible();

  const form = page.getByRole('form', { name: 'この場所にファイルを作成' });
  await form.getByRole('textbox', { name: 'ファイル名' }).fill('readme.txt');
  await form.getByRole('textbox', { name: '内容' }).fill('上書き');
  await form.getByRole('button', { name: '作成する' }).click();
  await expect(form.getByText(/同じ名前のファイルが既にあります/)).toBeVisible();
  expect(await readFile(join(folder, 'readme.txt'), 'utf8')).toBe('こんにちは');
  await form.getByRole('textbox', { name: 'ファイル名' }).fill('メモ.txt');
  await form.getByRole('button', { name: '作成する' }).click();
  await expect(page.getByRole('status')).toHaveText('ファイル「メモ.txt」を作成しました。');
  expect(await readFile(join(folder, 'メモ.txt'), 'utf8')).toBe('上書き');

  await table.getByRole('button', { name: 'sub' }).click();
  await expect(page.getByRole('table', { name: '営業資料 / subの内容' }).getByText('inner.txt')).toBeVisible();
  await page.getByRole('button', { name: '上の階層へ' }).click();
  await expect(page.getByRole('table', { name: '営業資料の内容' })).toBeVisible();
  expect(await readdir(outside)).toEqual(['secret.txt']);
});

test('hostile IPC from page script is refused with safe codes and no path disclosure', async ({ page }) => {
  await writeFile(join(root, 'top-secret.txt'), 'secret');
  await wireDesktop(page, () => bridge);
  await page.goto('/local-workspaces');
  await createWorkspace(page, 'W');
  const results = await page.evaluate(async () => {
    const invoke = (window as unknown as { __TAURI__: { core: { invoke: (n: string, a: unknown) => Promise<unknown> } } }).__TAURI__.core.invoke;
    const call = (command: string, request: unknown) => invoke('local_workspace_runtime', { command, request }).then((ok) => ({ ok }), (err) => ({ err }));
    const [workspace] = await invoke('local_workspace_runtime', { command: 'workspace.list', request: null }) as Array<{ workspaceId: string; effectiveContextRevision: string; managedBindingId: string }>;
    const context = { workspaceId: workspace!.workspaceId, effectiveContextRevision: workspace!.effectiveContextRevision };
    const ref = (locator: string[]) => ({ bindingId: workspace!.managedBindingId, locator });
    return {
      traversal: await call('entries.list', { context, ref: ref(['..', '..']), cursor: null }),
      absolute: await call('file.openRead', { context, ref: ref(['/etc/passwd']), expectedFileIdentity: 'x' }),
      windows: await call('entries.list', { context, ref: ref(['C:\\Windows']), cursor: null }),
      device: await call('file.create', { context, parent: ref([]), name: 'CON', bytesBase64: '', operationId: 'op-dev' }),
      extraField: await call('entries.list', { context, ref: ref([]), cursor: null, path: '/' }),
      shell: await call('shell.open', { path: '/bin/sh' }),
      otherCommand: await invoke('plugin:fs|read_file', { path: '/etc/passwd' }).then((ok) => ({ ok }), (err) => ({ err })),
      stale: await call('entries.list', { context: { ...context, effectiveContextRevision: 'c999' }, ref: ref([]), cursor: null }),
    };
  });
  expect(results).toEqual({
    traversal: { err: { code: 'invalid_locator', reason: 'invalid_name' } },
    absolute: { err: { code: 'invalid_locator', reason: 'invalid_name' } },
    windows: { err: { code: 'invalid_locator', reason: 'invalid_name' } },
    device: { err: { code: 'invalid_locator', reason: 'invalid_name' } },
    extraField: { err: { code: 'invalid_locator', reason: 'invalid_request' } },
    shell: { err: { code: 'unavailable', reason: 'unknown_command' } },
    otherCommand: { err: { code: 'unavailable', reason: 'unknown_command' } },
    stale: { err: { code: 'stale_context' } },
  });
  expect(JSON.stringify(bridge.calls)).not.toContain(root);
});

test('a lost response keeps the operation ID and "結果を確認" never creates a second file or root', async ({ page }) => {
  const folder = join(root, 'docs');
  await mkdir(folder);
  let dropCreate = true;
  let dropWorkspace = true;
  await wireDesktop(page, () => bridge, async (command, route) => {
    if (command === 'workspace.create' && dropWorkspace) { dropWorkspace = false; await route.abort('connectionreset'); return true; }
    if (command === 'file.create' && dropCreate) { dropCreate = false; await route.abort('connectionreset'); return true; }
    return false;
  });
  await page.goto('/local-workspaces');
  await page.getByRole('button', { name: '新しいWorkspace' }).click();
  const dialog = page.getByRole('dialog', { name: '新しいWorkspace' });
  await dialog.getByRole('textbox', { name: 'Workspace名' }).fill('再確認');
  await dialog.getByRole('button', { name: '作成する' }).dblclick();
  await expect(dialog.getByRole('button', { name: '結果を確認' })).toBeVisible();
  await expect(dialog.getByRole('textbox', { name: 'Workspace名' })).toBeDisabled();
  await dialog.getByRole('button', { name: '結果を確認' }).click();
  await expect(dialog).toBeHidden();
  const creates = bridge.callsOf('workspace.create');
  expect(creates).toHaveLength(2);
  expect(creates[0]!.request).toEqual(creates[1]!.request);
  expect(await readdir(join(root, 'state', 'managed'))).toHaveLength(1);

  await bridge.pick(folder);
  await page.getByRole('button', { name: 'フォルダーを追加' }).click();
  await page.getByRole('button', { name: 'docsを開く' }).click();
  const form = page.getByRole('form', { name: 'この場所にファイルを作成' });
  await form.getByRole('textbox', { name: 'ファイル名' }).fill('a.txt');
  await form.getByRole('textbox', { name: '内容' }).fill('one');
  await form.getByRole('button', { name: '作成する' }).click();
  await expect(form.getByRole('button', { name: '結果を確認' })).toBeVisible();
  expect(await readFile(join(folder, 'a.txt'), 'utf8')).toBe('one');
  await form.getByRole('button', { name: '結果を確認' }).click();
  await expect(page.getByRole('status')).toHaveText('ファイル「a.txt」を作成しました。');
  const fileCreates = bridge.callsOf('file.create');
  expect(fileCreates).toHaveLength(2);
  expect((fileCreates[0]!.request as { operationId: string }).operationId).toBe((fileCreates[1]!.request as { operationId: string }).operationId);
  expect(await readdir(folder)).toEqual(['a.txt']);
});

test('the shell keeps Router navigation between existing screens and the desktop screen', async ({ page }) => {
  await wireDesktop(page, () => bridge);
  await page.goto('/local-workspaces');
  await createWorkspace(page, 'ナビ');
  await page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '文書', exact: true }).click();
  await expect(page).toHaveURL(/\/documents\?view=published/);
  await expect(page.getByRole('main', { name: '文書ワークスペース' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: 'ローカルWorkspace' })).toHaveCount(0);
  await page.getByRole('banner').getByRole('link', { name: 'ローカルWorkspace' }).click();
  await expect(page).toHaveURL(/\/local-workspaces$/);
  await expect(page.getByRole('button', { name: 'ナビ' })).toBeVisible();
});

test('browser runtime (no bridge) shows explicit unavailability and no desktop indicator', async ({ page }) => {
  await page.route('**/v1/**', (route) => route.fulfill({ status: 503, json: { type: 'about:blank', title: 'Unavailable', status: 503 } }));
  await page.goto('/local-workspaces');
  await expect(page.getByText(/ブラウザー版ではローカルフォルダーとWorkspaceを利用できません/)).toBeVisible();
  await expect(page.getByRole('banner').getByText('デスクトップで実行中')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '新しいWorkspace' })).toHaveCount(0);
});
