// Compile against the already qualified workspace compiler and generated client.
// Only emitted local imports and this one workspace alias are rewritten for Node ESM.
import { execFileSync } from 'node:child_process';
import { readdir, readFile, writeFile, stat, rm } from 'node:fs/promises';
import { dirname, resolve, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const output = resolve(here, '.build');
await rm(output, { recursive: true, force: true });
execFileSync(process.execPath, [resolve(here, '../../node_modules/typescript/bin/tsc'), '-p', resolve(here, 'tsconfig.json')], { stdio: 'inherit' });
async function walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) { await walk(path); continue; }
    if (!entry.name.endsWith('.js')) continue;
    const original = await readFile(path, 'utf8');
    const rewritten = original.replace(/(from\s*|import\s*)['"]([^'"]+)['"]/g, (match, prefix, specifier) => {
      if (specifier === '@knowledge-platform/document-api-client') {
        let local = relative(dirname(path), resolve(output, 'packages/document-api-client/src/index.js')).split(sep).join('/');
        return `${prefix}'${local.startsWith('.') ? local : `./${local}`}'`;
      }
      if (!specifier.startsWith('.')) return match;
      return `${prefix}'${specifier.endsWith('.js') ? specifier : `${specifier}.js`}'`;
    });
    // Generated clients use extensionless directory imports as well as files.
    const imports = [...rewritten.matchAll(/(?:from\s*|import\s*)['"](\.[^'"]+\.js)['"]/g)];
    let result = rewritten;
    for (const [, specifier] of imports) {
      const exists = await stat(resolve(dirname(path), specifier)).then(() => true, () => false);
      if (!exists) {
        const directory = specifier.slice(0, -3) + '/index.js';
        await stat(resolve(dirname(path), directory));
        result = result.replaceAll(`'${specifier}'`, `'${directory}'`);
      }
    }
    await writeFile(path, result);
  }
}
await walk(output);
await writeFile(resolve(output, 'package.json'), '{"type":"module"}\n');
