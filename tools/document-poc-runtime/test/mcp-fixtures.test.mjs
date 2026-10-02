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

test('the GUI third-Version fixture changes one line against both seeded regulation Versions', () => {
  const runtime = ts.createSourceFile('runtime.ts', readFileSync(new URL('../../../apps/document-web/e2e-runtime/document-runtime.spec.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true);
  const seed = ts.createSourceFile('seed.ts', readFileSync(new URL('../../../tools/document-poc-seed/src/seed.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true);
  function declaration(source, name) {
    const found=[];
    function visit(node) { if(ts.isVariableDeclaration(node)&&node.name.getText(source)===name)found.push(node.initializer);ts.forEachChild(node,visit); }
    visit(source);assert.equal(found.length,1);return found[0];
  }
  const changed = declaration(runtime,'changedContent');
  assert.ok(ts.isCallExpression(changed));assert.equal(changed.expression.getText(runtime),'Buffer.from');
  assert.ok(ts.isStringLiteral(changed.arguments[0]));
  const fixtures = declaration(seed,'FIXTURES');assert.ok(ts.isAsExpression(fixtures));assert.ok(ts.isArrayLiteralExpression(fixtures.expression));
  const values = object => Object.fromEntries(object.properties.filter(ts.isPropertyAssignment)
    .map(property=>[property.name.getText(seed),property.initializer]));
  const regulation = fixtures.expression.elements.map(values).find(value=>value.key.text==='regulation');assert.ok(regulation);
  const target = changed.arguments[0].text.split('\n');
  // This is an explicit content-only fixture contract, not a replacement for
  // TextComparator: retain the shared first two lines and edit only line 3.
  for(const name of ['content','nextContent']) {
    assert.ok(ts.isStringLiteral(regulation[name]));const base=regulation[name].text.split('\n');
    assert.equal(target.length,base.length,`${name}: preserve the existing line count`);
    assert.deepEqual(base.flatMap((line,index)=>line===target[index]?[]:[index]),[2],`${name}: only line 3 may change`);
    assert.equal(target.at(-1),'');
  }
});
