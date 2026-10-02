import { fileURLToPath } from 'node:url';
import { normalizeBaseUrl, runSeed, SeedSafetyError } from './seed';

try {
  if (process.env.KP_RUNTIME_MODE !== 'poc') throw new SeedSafetyError('Explicit KP_RUNTIME_MODE=poc is required for synthetic seeding');
  const baseUrl = normalizeBaseUrl(process.env.KP_DOCUMENT_API_BASE_URL ?? 'http://127.0.0.1:8080');
  // This path resolves from the emitted entrypoint into the tool-local ignored directory.
  const manifestPath = process.env.KP_POC_SEED_MANIFEST ?? fileURLToPath(new URL('../../../../.state/manifest.json', import.meta.url));
  const manifest = await runSeed({ baseUrl, manifestPath });
  process.stdout.write(JSON.stringify({ status: 'ok', fixtureHash: manifest.fixtureHash,
    rootFolderId: manifest.rootFolderId,
    folders: Object.fromEntries(Object.entries(manifest.folders).map(([key, folder]) => [key, folder.folderId])),
    documents: Object.fromEntries(Object.entries(manifest.documents).map(([key, document]) => [key, document.create?.result])),
  }, null, 2) + '\n');
} catch (error) {
  // Do not dump request objects, response payloads, URLs, credentials, or local paths.
  const message = error instanceof SeedSafetyError ? error.message : 'Common API request failed; the manifest preserves retry state';
  process.stderr.write(`Document PoC seed failed: ${message}\n`);
  process.exitCode = 1;
}
