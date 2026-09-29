import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

if (process.argv.length !== 4) {
  throw new Error('usage: node check-shapes.mjs OPENAPI_TS JSON_SCHEMA_TS');
}
const openapi = readFileSync(process.argv[2], 'utf8');
const schema = readFileSync(process.argv[3], 'utf8');

for (const output of [openapi, schema]) {
  assert.match(output, /mode: "inherit";/);
  assert.match(output, /mode: "explicit";/);
  assert.match(output, /coverage: "full" \| "partial" \| "none";/);
  assert.match(output, /file: string;/);
  assert.match(output, /files: (string\[\]|\[string, \.\.\.string\[\]\]);/);
}
assert.match(openapi, /downloadVersionFile:/);
assert.match(openapi, /policyId: string \| null;/);
assert.match(schema, /export type SetAccessPolicy = PolicyInherit \| PolicyExplicit;/);
process.stdout.write('Expected actual-contract shapes observed; binary parts remain string in both TypeScript outputs. No production promotion.\n');
