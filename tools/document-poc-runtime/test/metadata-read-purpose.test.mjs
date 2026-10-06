import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

// 実受入specのread用途を検査する純粋なsource guard。Rust/API/browserは実行しない。
const source = await readFile(new URL('../../../apps/document-web/e2e-runtime/metadata-editor.spec.ts', import.meta.url), 'utf8');
const syntax = ts.createSourceFile('metadata-editor.spec.ts', source, ts.ScriptTarget.Latest, true);
const nodes = [];
function visit(node) { nodes.push(node); ts.forEachChild(node, visit); }
visit(syntax);
const calls = name => nodes.filter(node => ts.isCallExpression(node) && node.expression.getText(syntax) === name);
const properties = (node, name) => {
  const found = [];
  function scan(child) {
    if (ts.isPropertyAssignment(child) && child.name.getText(syntax) === name) found.push(child.initializer.getText(syntax));
    if (ts.isShorthandPropertyAssignment(child) && child.name.text === name) found.push(child.name.text);
    ts.forEachChild(child, scan);
  }
  scan(node); return found;
};

test('metadata受入のdetail/version helperはread用途を必須引数で受け、そのままAPIへ渡す', () => {
  for (const [name, parameter, api] of [['detail', 'view', 'getDocument'], ['version', 'purpose', 'getDocumentVersion']]) {
    const declaration = nodes.find(node => ts.isVariableDeclaration(node) && node.name.getText(syntax) === name);
    const helper = declaration.initializer;
    assert.ok(ts.isArrowFunction(helper));
    assert.equal(helper.parameters.length, 1, `${name}は用途を暗黙に固定しない`);
    const input = helper.parameters[0];
    assert.equal(input.name.getText(syntax), parameter);
    assert.equal(input.initializer, undefined, '既定値へfallbackしない');
    assert.equal(input.questionToken, undefined, '用途の省略を許可しない');
    assert.match(input.type.getText(syntax), /'authoring'\s*\|\s*'published'/);
    const request = calls(api).find(call => call.pos > helper.pos && call.end < helper.end);
    assert.deepEqual(properties(request.arguments[0], parameter), [parameter]);
  }
});

test('公開前のWORKINGはauthoring、公開後の同一Versionはpublishedで確認する', async () => {
  const publication = calls('publishVersion');
  assert.equal(publication.length, 1);
  for (const name of ['detail', 'version']) {
    const reads = calls(name);
    assert.equal(reads.length, 5);
    for (const call of reads) {
      const expected = call.pos < publication[0].pos ? 'authoring' : 'published';
      assert.equal(call.arguments.length, 1, `${name}の用途を明示する`);
      assert.ok(ts.isStringLiteral(call.arguments[0]));
      assert.equal(call.arguments[0].text, expected);
    }
  }
  // 公開済み版をauthoringで読むと404という既存契約との対応を固定する。
  const repository = await readFile(new URL('../../../crates/document-repository-postgres/src/document_history.rs', import.meta.url), 'utf8');
  assert.match(repository, /VersionPurpose::Authoring => \{\s*if state != "WORKING" \|\| ended \{\s*return Err\(RepositoryError::DocumentVersionNotFound\)/);
});

test('公開後と再起動後のGUIもpublishedへ明示遷移し、authoring画面を再読込しない', () => {
  const routes = calls('page.goto').map(call => call.arguments[0].getText(syntax));
  assert.equal(routes.length, 3);
  assert.deepEqual(routes.map(route => route.match(/view=(authoring|published)&tab=overview/)?.[1]), ['authoring', 'published', 'published']);
  assert.equal(calls('page.reload').length, 0);
  const publication = calls('publishVersion')[0];
  for (const api of ['listVersionFiles', 'listDocumentVersions']) {
    for (const call of calls(api)) {
      assert.ok(call.pos < publication.pos, `${api}のauthoring取得は公開前だけ`);
      assert.deepEqual(properties(call.arguments[0], 'purpose'), ["'authoring'"]);
    }
  }
});
