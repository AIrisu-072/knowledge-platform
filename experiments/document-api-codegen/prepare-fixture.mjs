import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const repository = resolve(import.meta.dirname, '../..');
const destination = resolve(process.argv[2] ?? '');
if (process.argv.length !== 3) {
  throw new Error('usage: node prepare-fixture.mjs OUTPUT_JSON');
}
const temporary = mkdtempSync(join(tmpdir(), 'document-api-codegen-'));
let api;
try {
  const bundled = join(temporary, 'api.json');
  execFileSync(process.execPath, [
    join(repository, 'node_modules/@redocly/cli/bin/cli.js'),
    'bundle', join(repository, 'spec/api/openapi.yaml'), '--ext', 'json', '--output', bundled,
  ], { cwd: repository, stdio: 'pipe' });
  api = JSON.parse(readFileSync(bundled, 'utf8'));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

function schemaRefs(value) {
  if (Array.isArray(value)) return value.map(schemaRefs);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(Object.entries(value).map(([key, child]) => [
    key,
    key === '$ref' && typeof child === 'string'
      ? child.replace(/^#\/components\/schemas\//, '#/$defs/')
      : schemaRefs(child),
  ]));
}

const fixture = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  title: 'DocumentApiContractFixture',
  type: 'object',
  additionalProperties: false,
  required: ['policy', 'comparison', 'createMultipart', 'versionMultipart', 'problem'],
  properties: {
    policy: { $ref: '#/$defs/SetAccessPolicy' },
    comparison: { $ref: '#/$defs/ComparisonResponse' },
    createMultipart: schemaRefs(api.components.requestBodies.CreateDocumentMultipart.content['multipart/form-data'].schema),
    versionMultipart: schemaRefs(api.components.requestBodies.VersionMultipart.content['multipart/form-data'].schema),
    problem: { $ref: '#/$defs/Problem' },
  },
  $defs: schemaRefs(api.components.schemas),
};
writeFileSync(destination, `${JSON.stringify(fixture, null, 2)}\n`);
process.stdout.write(`${destination}\n`);
