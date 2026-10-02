import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';
import { renderSchema } from './generate-schema.mjs';
const root=resolve(import.meta.dirname,'../..');
const catalog=JSON.parse(readFileSync(resolve(root,'spec/telemetry/audit-event-catalog.json'),'utf8'));
test('schema regenerates byte-for-byte from the catalog',()=>{
  assert.equal(renderSchema(catalog),readFileSync(resolve(root,'spec/telemetry/audit-event.schema.json'),'utf8'));
});
test('duplicate event types cannot produce a schema',()=>{
  const copy=structuredClone(catalog);copy.events.push(copy.events[0]);
  assert.throws(()=>renderSchema(copy),/duplicate/);
});
test('unknown field kinds fail closed during generation',()=>{
  const copy=structuredClone(catalog);copy.events[0].fields.documentId.kind='arbitrary';
  assert.throws(()=>renderSchema(copy),/unknown catalog field/);
});
