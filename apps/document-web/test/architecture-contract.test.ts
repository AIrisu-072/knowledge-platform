import { readFile, readdir } from 'node:fs/promises';
import { resolve } from 'node:path';
async function sourceFiles(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const path = resolve(directory, entry.name);
    return entry.isDirectory() ? sourceFiles(path) : [path];
  }));
  return nested.flat();
}

test('server state remains in Query and no global state machine is added', async () => {
  const manifest = JSON.parse(await readFile(resolve(process.cwd(), 'package.json'), 'utf8')) as {
    dependencies?: Record<string, string>;
    devDependencies?: Record<string, string>;
  };
  const dependencies = { ...manifest.dependencies, ...manifest.devDependencies };

  expect(dependencies).toHaveProperty('@tanstack/react-query');
  expect(dependencies).not.toHaveProperty('@tanstack/store');
  expect(dependencies).not.toHaveProperty('xstate');
});

test('presentation components do not own transport or lifecycle authorization rules', async () => {
  const directories = ['src/components', 'src/routes'].map((path) => resolve(process.cwd(), path));
  const files = (await Promise.all(directories.map(sourceFiles))).flat();
  expect(files.some((path) => path.endsWith('.tsx'))).toBe(true);

  for (const path of files.filter((entry) => /\.(tsx|ts)$/.test(entry))) {
    const source = await readFile(path, 'utf8');
    expect(source).not.toMatch(/\bfetch\s*\(/);
    expect(source).not.toMatch(/['"`]\/v1\//);
    expect(source).not.toMatch(/from\s+['"][^'"]*(document-api-client|\/api(?:\/|['"]))/);
    expect(source).not.toMatch(/if\s*\([^)]*(?:lifecycleState|\.acl|\.permissions)[^)]*(?:===|!==|&&|\|\|)/);
  }
});
