import { readFile, readdir } from 'node:fs/promises';
import { relative, resolve } from 'node:path';

async function sourceFiles(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const path = resolve(directory, entry.name);
    return entry.isDirectory() ? sourceFiles(path) : [path];
  }));
  return nested.flat();
}

test('only the runtime adapter layer knows about the desktop host bridge', async () => {
  const root = resolve(process.cwd(), 'src');
  const files = (await sourceFiles(root)).filter((path) => /\.(tsx?|jsx?)$/.test(path));
  for (const path of files) {
    const name = relative(root, path);
    const source = await readFile(path, 'utf8');
    if (!name.startsWith('runtime/')) {
      expect({ name, bridge: /__TAURI|@tauri-apps|tauri:\/\/|ipc\.localhost/.test(source) }).toEqual({ name, bridge: false });
    }
    if (name.startsWith('components/') || name.startsWith('routes/')) {
      expect({ name, adapter: /runtime\/(desktop-runtime|select-runtime|browser-runtime)/.test(source) }).toEqual({ name, adapter: false });
    }
  }
});

test('the frontend never asks the runtime for paths, shell or process access', async () => {
  const files = await sourceFiles(resolve(process.cwd(), 'src/runtime'));
  for (const path of files) {
    const source = await readFile(path, 'utf8');
    expect(source).not.toMatch(/\b(absolutePath|physicalPath|shell\.|process\.spawn|Command\()/);
  }
  const desktop = await readFile(resolve(process.cwd(), 'src/runtime/desktop-runtime.ts'), 'utf8');
  const commands = [...desktop.matchAll(/(?:read|mutate)\('([a-zA-Z.]+)'/g)].map((match) => match[1]).sort();
  expect(commands).toEqual([
    'capabilities', 'directory.attach', 'directory.choose', 'directory.detach', 'entries.list', 'file.closeRead', 'file.create',
    'file.openRead', 'file.read', 'workspace.create', 'workspace.list', 'workspace.recover', 'workspace.rename',
  ]);
  // The broker's fixed command list is the same set (single source checked from both sides).
  const wire = await readFile(resolve(process.cwd(), '../../crates/local-workspace-runtime/src/wire.rs'), 'utf8');
  const block = wire.slice(wire.indexOf('pub const COMMANDS'), wire.indexOf('];', wire.indexOf('pub const COMMANDS')));
  expect([...block.matchAll(/"([a-zA-Z.]+)"/g)].map((match) => match[1]).sort()).toEqual(commands);
  expect(wire).toContain('pub const IPC_COMMAND: &str = "local_workspace_runtime";');
  expect(desktop).toContain("RUNTIME_IPC_COMMAND = 'local_workspace_runtime'");
});
