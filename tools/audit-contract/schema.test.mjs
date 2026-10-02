import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const root = resolve(import.meta.dirname, '../..');
const schema = resolve(root, 'spec/telemetry/audit-event.schema.json');
const catalog = resolve(root, 'spec/telemetry/audit-event-catalog.json');

test('normative audit schema and unique event catalog exist', () => {
  assert.ok(existsSync(schema), 'versioned Audit schema is absent');
  assert.ok(existsSync(catalog), 'Audit event catalog is absent');
  const c = JSON.parse(readFileSync(catalog, 'utf8'));
  assert.equal(c.version, 1);
  const types = c.events.map(e => e.type);
  assert.equal(new Set(types).size, types.length);
  for (const required of ['document.created','document.version.withdrawn','authorization.denied','document.diff.result_access_granted']) {
    assert.ok(types.includes(required), required);
  }
});
