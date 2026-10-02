import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

// A source-wiring regression, not real-runtime qualification. Pair it with the
// actual GUI and stdio tests; never reimplement the production alignment rule.
test('the changed regulation upload retains the seeded logical path and explicit text MIME', () => {
  const path = new URL('../../../apps/document-web/e2e-runtime/document-runtime.spec.ts', import.meta.url);
  const source = ts.createSourceFile(path.pathname, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true);
  const uploads = [];
  function visit(node) {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && node.expression.name.text === 'setInputFiles') uploads.push(node.arguments[0]);
    ts.forEachChild(node, visit);
  }
  visit(source);
  assert.ok(ts.isObjectLiteralExpression(uploads[0]));
  const properties = Object.fromEntries(uploads[0].properties.filter(ts.isPropertyAssignment)
    .map(property => [property.name.getText(source), property.initializer]));
  assert.ok(ts.isStringLiteral(properties.name));
  assert.equal(properties.name.text, 'primary', 'A content edit must retain the initial/seed logical path, not also rename it');
  assert.equal(properties.mimeType.text, 'text/plain');
  assert.equal(properties.buffer.getText(source), 'changedContent');
});
