import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../../../apps/document-web/e2e-runtime/human-agent-consistency.spec.ts', import.meta.url), 'utf8');
const syntax = ts.createSourceFile('consistency.ts', source, ts.ScriptTarget.Latest, true);
function nodes(predicate) {
  const found = [];
  function visit(node) { if (predicate(node)) found.push(node); ts.forEachChild(node, visit); }
  visit(syntax); return found;
}
function property(object, name) {
  assert.ok(ts.isObjectLiteralExpression(object), 'fixture must expose an object literal');
  const entry = object.properties.find(item => ts.isPropertyAssignment(item) && item.name.getText(syntax) === name);
  assert.ok(entry, `missing fixture property ${name}`); return entry.initializer;
}
function literal(node) {
  if (ts.isStringLiteral(node)) return node.text;
  if (ts.isObjectLiteralExpression(node)) return Object.fromEntries(node.properties.map(item => {
    assert.ok(ts.isPropertyAssignment(item), 'metadata fixture must contain explicit literal properties');
    return [item.name.getText(syntax), literal(item.initializer)];
  }));
  if (ts.isArrayLiteralExpression(node)) return node.elements.map(literal);
  assert.fail('metadata fixture must use JSON object/string/array literals');
}

test('ordered metadata fixture obeys the existing patch contract and pins every expected full projection', async () => {
  // Source/configuration guard only: it reads actual harness inputs and the
  // existing Application allowlist; it does not execute the Rust API contract.
  const validation = await readFile(new URL('../../../crates/document-application/src/management_digest.rs', import.meta.url), 'utf8');
  const allowed = [...validation.match(/const ALLOWED: \[&str; 4\] = \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map(match => match[1]);
  const create = nodes(node => ts.isCallExpression(node) && node.expression.getText(syntax).endsWith('.createDocument'))[0];
  const initial = literal(property(property(create.arguments[0], 'request'), 'documentMetadata'));
  const mutation = nodes(node => ts.isVariableDeclaration(node) && node.name.getText(syntax) === 'mutation')[0].initializer;
  const set = literal(property(mutation, 'set')), unset = literal(property(mutation, 'unset'));
  for (const [key, value] of Object.entries(set)) {
    assert.ok(allowed.includes(key), `actual acceptance PATCH uses unsupported key: ${key}`);
    assert.equal(typeof value, key === 'extensions' ? 'object' : 'string');
    assert.ok(value !== null && !Array.isArray(value));
  }
  assert.deepEqual(unset, []);
  assert.deepEqual(initial, { extensions: { c3Stage: 'C3 initial metadata' } });
  const updated = { ...initial, ...set };
  assert.deepEqual(updated, { extensions: { c3Stage: 'C3 changed metadata' } });
  const transitions = nodes(node => ts.isCallExpression(node) && node.expression.getText(syntax) === 'assertRevisionTransition');
  assert.equal(transitions.length, 4);
  assert.deepEqual(transitions.map(call => literal(property(call.arguments[2], 'metadata'))), [initial, updated, updated, updated]);
});

test('revoked-write observation uses readable published detail without weakening the mutation denial', () => {
  // Published visibility requires Read; authoring visibility additionally
  // requires Write. This fixture intentionally keeps Read after revocation.
  const revoked = source.slice(source.indexOf('revoked = true;'), source.indexOf('const responsePromise', source.indexOf('revoked = true;')));
  assert.match(revoked, /getDocument\([\s\S]*?view: 'published'/);
  assert.doesNotMatch(revoked, /view: 'authoring'/);
  assert.match(revoked, /createVersion\.status\)\.toBe\('disabled'\)/);
  assert.match(source, /expect\(response\.status\(\)\)\.toBe\(403\); expect\(\(await response\.json\(\)\)\.code\)\.toBe\('FORBIDDEN'\)/);
});
