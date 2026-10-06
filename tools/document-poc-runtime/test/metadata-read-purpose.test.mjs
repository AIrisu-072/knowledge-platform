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

test('no-op後の固定診断は元のrequest登録・await・検証の境界にだけ置く', () => {
  const statements = calls('test')[0].arguments[1].body.statements.map(statement => statement.getText(syntax));
  const completion = stage => `completed('${stage}');`;
  const boundaries = [
    ['gui-metadata-list-return-pressed', "await page.getByRole('button', { name: '← 一覧へ戻る', exact: true }).press('Enter');",
      'await verifyCreatedList(page, await fromResponse, exactFrom, [{ documentId, createdAt }]);'],
    ['gui-metadata-created-from-verified', 'await verifyCreatedList(page, await fromResponse, exactFrom, [{ documentId, createdAt }]);',
      "await inputEquals(page.getByLabel('文書名で絞り込み', { exact: true }), listTitle);"],
    ['gui-metadata-unread-list-verified', 'await readUnreadPublishedList(page, documentId, created.documentVersionId, exactFrom);',
      'await page.locator(`[data-document-id="${documentId}"]`).press(\'Enter\');'],
    ['gui-unread-readonly-verified', 'expect(origins).toEqual(new Set([context.human]));',
      "const sourceBefore = (await getDocument({ ...common, path, query: { view: 'published' } })).data;"],
    ['gui-metadata-revision-entry-ready', "page.off('requestfinished', recordMoveReadFinished);",
      'const baseRevision = after.revisions[0]!, targetRevision = after.revisions[1]!;'],
    ['gui-metadata-revision-first-page-verified', "await expect(page.getByRole('button', { name: '正式改訂をさらに表示', exact: true })).toBeHidden();",
      "await page.getByRole('combobox', { name: '基準', exact: true }).selectOption(baseRevision.revisionId);"],
    ['gui-metadata-comparison-pair-verified',
      "await expect.poll(() => new URL(page.url()).searchParams.get('baseRevisionId') === baseRevision.revisionId\n      && new URL(page.url()).searchParams.get('targetRevisionId') === targetRevision.revisionId).toBe(true);",
      "const comparisonResponse = page.waitForResponse(response => {\n      const url = new URL(response.url());\n      return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/revision-comparisons`\n        && response.request().method() === 'POST';\n    });"],
    ['gui-metadata-comparison-tab-pressed', "await page.getByRole('tab', { name: '新旧比較', exact: true }).press('Enter');",
      'const comparisonResult = await comparisonResponse;'],
    ['gui-metadata-comparison-response-verified', 'expect(comparisonResult.status()).toBe(200);',
      "privatelyEqual(comparisonResult.request().postDataJSON(), {\n      baseRevisionId: baseRevision.revisionId, targetRevisionId: targetRevision.revisionId, projection: 'display', pageSize: 50,\n    });"],
    ['gui-metadata-revision-first-comparison-verified', "await expect(page.getByText('同じコンテンツ版のため本文比較なし', { exact: true })).toBeVisible();",
      "const comparisonReloadResponse = page.waitForResponse(response => {\n      const url = new URL(response.url());\n      return url.origin === context.human && url.pathname === `/v1/documents/${documentId}/revision-comparisons`\n        && response.request().method() === 'POST';\n    });"],
    ['gui-metadata-revision-reload-page-verified', "await expect(page.getByRole('button', { name: '正式改訂をさらに表示', exact: true })).toBeHidden();",
      "await inputEquals(page.getByRole('combobox', { name: '基準', exact: true }), baseRevision.revisionId);"],
  ];
  for (const [stage, before, after] of boundaries) {
    const index = statements.indexOf(completion(stage));
    assert.ok(index > 0, `${stage}が既存await間の停止区間を区別する`);
    assert.equal(statements[index - 1], before, `${stage}は元の操作完了後だけ`);
    assert.equal(statements[index + 1], after, `${stage}の次の既存awaitを省略しない`);
  }
  const start = statements.indexOf(completion('gui-metadata-noop-verified'));
  const end = statements.indexOf(completion('gui-document-move-verified'));
  assert.deepEqual(statements.slice(start + 1, end).filter(statement => statement.startsWith('completed(')),
    boundaries.map(([stage]) => completion(stage)));
  const returned = statements.indexOf(completion('gui-metadata-list-return-pressed'));
  assert.equal(statements[returned - 2], 'const fromResponse = waitCreatedList(page, exactFrom);');
  const operations = statements.filter(statement => !statement.startsWith('completed('));
  for (const [response, result, action] of [
    ['revisionResponse', 'revisionResult', "page.getByRole('tab', { name: '版・改訂', exact: true })"],
    ['comparisonResponse', 'comparisonResult', "page.getByRole('tab', { name: '新旧比較', exact: true })"],
    ['revisionRestartResponse', 'revisionRestartResult', "page.getByRole('button', { name: '正式改訂を最初から読み直す', exact: true })"],
  ]) {
    const registered = operations.findIndex(statement => statement.startsWith(`const ${response} = page.waitForResponse(`));
    assert.ok(registered > 0);
    assert.equal(operations[registered + 1], `await ${action}.press('Enter');`);
    assert.equal(operations[registered + 2], `const ${result} = await ${response};`);
  }
  for (const call of calls('completed')) {
    assert.equal(call.arguments.length, 1);
    assert.ok(ts.isStringLiteral(call.arguments[0]), '公開工程には動的値を渡さない');
  }
  for (const testCase of calls('test')) {
    const body = testCase.arguments[1].body;
    assert.ok(calls('completed').filter(call => call.pos > body.pos && call.end < body.end).length < 40,
      '既存annotation上限を増やさず、最終工程も同じ枠に収める');
  }
});

// 実GUI通信の限定記録を判定するspec内関数そのものを純粋に実行する。
// POST後のreset GETやheadersだけ到着したGETでfresh readを誤充足させる変更を検出する。
function specFunction(name) {
  const declaration = nodes.find(node => ts.isFunctionDeclaration(node) && node.name?.text === name);
  assert.ok(declaration, `${name}の実通信guardが必要`);
  const compiled = ts.transpileModule(`const predicate = ${declaration.getText(syntax)};`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText;
  return new Function(`${compiled}\nreturn predicate;`)();
}

test('文書移動のfresh GETは送信操作後に開始し、200のbody完了がPOSTより前である', () => {
  const precedes = specFunction('moveReadPrecedesPost');
  assert.equal(precedes({ started: 11, finished: 12, status: 200 }, 10, 13), true);
  for (const record of [
    { started: 9, finished: 12, status: 200 },
    { started: 11, status: 200 },
    { started: 11, finished: 13, status: 200 },
    { started: 14, finished: 15, status: 200 },
    { started: 11, finished: 12, status: 403 },
  ]) assert.equal(precedes(record, 10, 13), false);
});

test('文書移動の記録はHumanの対象published詳細とcursorなし200件の元行/先childrenだけに限る', () => {
  const route = specFunction('moveReadRoute');
  const classify = (path, method = 'GET', origin = 'http://127.0.0.1:31001') =>
    route(new URL(path, origin), method, 'http://127.0.0.1:31001', 'document', 'root', 'sandbox');
  assert.equal(classify('/v1/documents/document?view=published'), 'document');
  assert.equal(classify('/v1/folders/root/children?pageSize=200'), 'root-children');
  assert.equal(classify('/v1/folders/sandbox/children?pageSize=200'), 'destination-children');
  for (const path of ['/v1/documents/other?view=published', '/v1/documents/document?view=authoring',
    '/v1/folders/root/children?pageSize=200&cursor=next', '/v1/folders/root/children?pageSize=100',
    '/v1/folders/other/children?pageSize=200', '/v1/documents']) assert.equal(classify(path), undefined);
  assert.equal(classify('/v1/documents/document?view=published', 'POST'), undefined);
  assert.equal(classify('/v1/documents/document?view=published', 'GET', 'http://127.0.0.1:31002'), undefined);
});

test('既存metadata journeyの1回移動と再起動後の同一要求replayは別の現在状態を検証する', () => {
  assert.equal(calls('test').length, 2, '既存18+5へcaseを増やさない');
  const replay = calls('moveDocument');
  assert.equal(replay.length, 1, '初回移動はGUI、再起動後のreplayだけSDKを使う');
  assert.deepEqual(properties(replay[0].arguments[0], 'body'), ['state.move.request']);
  assert.match(source, /privatelyEqual\(replay\.data, state\.move\.receipt\)/);
  assert.match(source, /privatelyEqual\(await persistedSnapshot\(context\.human, documentId\), movedSnapshot\)/);
  assert.match(source, /privatelyEqual\(await persistedSnapshot\(context\.agent, documentId\), movedSnapshot\)/);
  assert.match(source, /const movedSnapshot = \{ \.\.\.after, revision: moveReceipt\.resultingRevision \}/);
  assert.match(source, /moveRequests\)\.toBe\(1\)/);
  assert.match(source, /page\.on\('requestfinished', recordMoveReadFinished\)/);
  assert.match(source, /readsAtPost = \[\.\.\.moveReads\.values\(\)\]\.map\(read => \(\{ \.\.\.read \}\)\)/);
  assert.match(source, /moveReadPrecedesPost\(read, sendingBoundary, movePostOrder\)/);
  assert.match(source, /privatelyEqual\(replayedHuman, humanDetail\)/);
  assert.match(source, /privatelyEqual\(replayedAgent, agentDetail\)/);
  assert.match(source, /humanReadState: sourceBefore\.readState, agentReadState: agentBefore\.readState/);
});

test('正式改訂の通常GUI readは両caseのHuman・対象Document・先頭100件と比較POSTだけを待つ', () => {
  const cases = calls('test');
  for (const [index, revisionCount, comparisonCount] of [[0, 2, 3], [1, 1, 2]]) {
    const body = cases[index].arguments[1];
    const waits = calls('page.waitForResponse').filter(call => call.pos > body.pos && call.end < body.end);
    const revisionWaits = waits.filter(call => call.arguments[0].getText(syntax).includes('/revisions`'));
    const comparisonWaits = waits.filter(call => call.arguments[0].getText(syntax).includes('/revision-comparisons`'));
    assert.equal(revisionWaits.length, revisionCount, '初回表示とjourneyの明示再読取が必要');
    assert.equal(comparisonWaits.length, comparisonCount, '明示再読取後は同pairの新比較も必要');
    for (const [kind, reads] of [['revisions', revisionWaits], ['revision-comparisons', comparisonWaits]]) {
      for (const wait of reads) {
        const compiled = ts.transpileModule(`const predicate = ${wait.arguments[0].getText(syntax)};`, {
          compilerOptions: { target: ts.ScriptTarget.ES2022 },
        }).outputText;
        const predicate = new Function('context', 'documentId', 'snapshot', `${compiled}\nreturn predicate;`)(
          { human: 'http://127.0.0.1:31001' }, 'document', { documentId: 'document' });
        const classify = (path, method = kind === 'revisions' ? 'GET' : 'POST', origin = 'http://127.0.0.1:31001') =>
          predicate({ url: () => new URL(path, origin).href, request: () => ({ method: () => method }) });
        const suffix = kind === 'revisions' ? '?pageSize=100' : '';
        assert.equal(classify(`/v1/documents/document/${kind}${suffix}`), true);
        assert.equal(classify(`/v1/documents/other/${kind}${suffix}`), false);
        assert.equal(classify(`/v1/documents/document/${kind}${suffix}`, 'PATCH'), false);
        assert.equal(classify(`/v1/documents/document/${kind}${suffix}`, kind === 'revisions' ? 'POST' : 'GET'), false);
        assert.equal(classify(`/v1/documents/document/${kind}${suffix}`, undefined, 'http://127.0.0.1:31002'), false);
        if (kind === 'revisions') {
          for (const query of ['', '?pageSize=1', '?pageSize=100&cursor=next', '?pageSize=100&cursor=']) {
            assert.equal(classify(`/v1/documents/document/revisions${query}`), false);
          }
        }
      }
    }
  }
  assert.equal(calls('compareDocumentRevisions').length, 0, '比較は通常GUIで読み、SDK readを追加しない');
});

test('比較結果の明示再読取は両phaseで新POSTを待ち、cursorなしの固定pairと一度だけのmetadata表示を保つ', () => {
  for (const testCase of calls('test')) {
    const body = testCase.arguments[1].body;
    const statements = body.statements.map(statement => statement.getText(syntax));
    const registered = statements.findIndex(statement => statement.startsWith('const comparisonReloadResponse = page.waitForResponse('));
    assert.ok(registered > 0, '初回比較後に再読取のHTTP待機を新しく登録する');
    assert.equal(statements[registered + 1], "await page.getByRole('button', { name: '比較結果を最初から読み直す', exact: true }).press('Enter');");
    assert.equal(statements[registered + 2], 'const comparisonReloadResult = await comparisonReloadResponse;');
    assert.equal(statements[registered + 3], 'expect(comparisonReloadResult.status()).toBe(200);');
    assert.equal(statements[registered + 4], "privatelyEqual(comparisonReloadResult.request().postDataJSON(), {\n      baseRevisionId: baseRevision.revisionId, targetRevisionId: targetRevision.revisionId, projection: 'display', pageSize: 50,\n    });");
    const returned = statements.findIndex((statement, index) => index > registered
      && statement === "await page.getByRole('button', { name: '← 版・改訂へ戻る', exact: true }).press('Enter');");
    assert.ok(returned > registered);
    const assertions = statements.slice(registered + 5, returned).join('\n');
    for (const beforeReturn of [
      "privatelyEqual(comparisonReloadBody.metadataChanges, comparisonBody.metadataChanges);",
      "expect(comparisonReloadBody.nextCursor ?? null).toBeNull();",
      "await inputEquals(page.getByRole('combobox', { name: '基準改訂', exact: true }), baseRevision.revisionId);",
      "await inputEquals(page.getByRole('combobox', { name: '比較対象', exact: true }), targetRevision.revisionId);",
      "await expect(metadataChanges).toHaveCount(1);",
      "await expect(metadataChanges.getByRole('listitem')).toHaveCount(comparisonReloadBody.metadataChanges.length);",
      "await expect(comparisonRegion.getByRole('button', { name: '比較結果をさらに表示', exact: true })).toBeHidden();",
    ]) assert.ok(assertions.includes(beforeReturn), `再読取結果を確認してから画面を離れる: ${beforeReturn}`);
    assert.match(assertions, /searchParams\.get\('baseRevisionId'\) === baseRevision\.revisionId/);
    assert.match(assertions, /searchParams\.get\('targetRevisionId'\) === targetRevision\.revisionId/);
    const firstPage = statements.slice(0, registered).join('\n');
    assert.match(firstPage, /expect\(comparisonBody\.nextCursor \?\? null\)\.toBeNull\(\)/);
    assert.match(firstPage, /expect\(comparisonBody\.metadataChanges\.length\)\.toBeGreaterThan\(0\)/);
    assert.match(firstPage, /await expect\(metadataChanges\)\.toHaveCount\(1\)/);
    assert.match(firstPage, /metadataChanges\.getByRole\('listitem'\)\)\.toHaveCount\(comparisonBody\.metadataChanges\.length\)/);
  }
});

test('通常本文比較の既存fixtureはGUIのdisplay50要求と実応答の本文行数・終端を確認する', async () => {
  const runtime = await readFile(new URL('../../../apps/document-web/e2e-runtime/document-runtime.spec.ts', import.meta.url), 'utf8');
  assert.match(runtime, /const comparisonResponse = page\.waitForResponse\(response => \{/);
  assert.match(runtime, /expect\(comparisonResult\.request\(\)\.postDataJSON\(\)\)\.toEqual\(\{ \.\.\.compareBody, pageSize: 50 \}\)/);
  assert.match(runtime, /expect\(comparisonBody\.nextCursor \?\? null\)\.toBeNull\(\)/);
  assert.match(runtime, /bodyChanges\.getByRole\('listitem'\)\)\.toHaveCount\(comparisonBody\.displayItems\.length\)/);
  assert.match(runtime, /comparisonRegion\.getByRole\('button', \{ name: '比較結果をさらに表示', exact: true \}\)\)\.toBeHidden\(\)/);
});

test('正式改訂readはmove listener外と再起動metadata取消後で行い、既存readState・snapshot検査を残す', () => {
  const cases = calls('test').map(call => call.arguments[1].getText(syntax));
  for (const body of cases) {
    assert.match(body, /getByRole\('list', \{ name: '正式改訂一覧', exact: true \}\)/);
    assert.match(body, /getByRole\('heading', \{ name: 'コンテンツ版', exact: true \}\)/);
    assert.match(body, /getByRole\('heading', \{ name: '正式改訂', exact: true \}\)/);
    assert.match(body, /getByRole\('button', \{ name: '正式改訂をさらに表示', exact: true \}\)\)\.toBeHidden\(\)/);
    assert.match(body, /selectOption\(baseRevision\.revisionId\)/);
    assert.match(body, /selectOption\(targetRevision\.revisionId\)/);
    assert.match(body, /baseRevision\.major === 1 && baseRevision\.minor === 1/);
    assert.match(body, /targetRevision\.major === 1 && targetRevision\.minor === 0/);
    assert.match(body, /projection: 'display', pageSize: 50/);
    assert.match(body, /contentComparisonStatus\)\.toBe\('sameAuthoritativeVersion'\)/);
    assert.match(body, /metadataComparisonStatus\)\.toBe\('different'\)/);
    assert.match(body, /locator\('dt'\)\.filter\(\{ hasText: \/\^基準\$\/ \}\)\.locator\('\+ dd'\)\)\.toHaveText\('1\.1'\)/);
    assert.match(body, /locator\('dt'\)\.filter\(\{ hasText: \/\^対象\$\/ \}\)\.locator\('\+ dd'\)\)\.toHaveText\('1\.0'\)/);
    assert.match(body, /searchParams\.get\('baseRevisionId'\) === baseRevision\.revisionId/);
    assert.match(body, /searchParams\.get\('targetRevisionId'\) === targetRevision\.revisionId/);
  }
  const journey = cases[0], persistence = cases[1];
  assert.ok(journey.indexOf("page.off('requestfinished', recordMoveReadFinished)") < journey.indexOf('const revisionResponse'));
  assert.ok(journey.indexOf('const comparisonRestartResponse') < journey.indexOf('const movedHuman'));
  assert.ok(journey.indexOf('const movedHuman') < journey.indexOf("completed('gui-formal-revisions-readonly-verified')"));
  assert.ok(persistence.indexOf("completed('gui-metadata-restart-verified')") < persistence.indexOf('const revisionResponse'));
  assert.ok(persistence.indexOf('const comparisonResponse') < persistence.indexOf('const beforeResponse'));
  assert.ok(persistence.lastIndexOf('privatelyEqual(await persistedSnapshot') < persistence.indexOf("completed('gui-formal-revisions-restart-readonly-verified')"));
});

test('比較専用画面からは既存戻るbuttonを経て通常tabへ移り、再読取と概要へ到達する', () => {
  for (const [index, expectedReturns] of [[0, 2], [1, 1]]) {
    const body = calls('test')[index].arguments[1];
    const navigation = nodes.filter(node => ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && node.expression.name.text === 'press' && node.pos > body.pos && node.end < body.end);
    let comparing = false, returns = 0;
    for (const action of navigation) {
      const locator = action.expression.expression;
      if (!ts.isCallExpression(locator) || locator.expression.getText(syntax) !== 'page.getByRole') continue;
      const role = locator.arguments[0]?.text, name = properties(locator.arguments[1], 'name')[0];
      if (role === 'tab') {
        assert.equal(comparing, false, '比較専用画面には通常tablistがない。既存戻るbuttonを先に使う');
        if (name === "'新旧比較'") comparing = true;
      }
      if (role === 'button' && name === "'← 版・改訂へ戻る'") {
        assert.equal(comparing, true, '比較後の戻る導線だけを追加する');
        comparing = false;
        returns++;
      }
    }
    assert.equal(returns, expectedReturns, 'journeyは2比較後、persistenceは1比較後に戻る');
    assert.equal(comparing, false, '後段の既存概要/readState/snapshot検査へ戻る');
  }
});
