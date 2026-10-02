// No renderer: this exercises the exact real helper sequence and fault reporting.
import test from 'node:test';
import assert from 'node:assert/strict';
import { assertKeyboard, safeFailure, KEYBOARD_STAGES } from '../browser-checks.mjs';
const sequence = [
  ['keyboard-skip-tab', 'press:Tab'],
  ['keyboard-skip-focus', 'evaluate:.skip', true],
  ['keyboard-skip-visible', 'evaluate:.skip', { style: 'solid', width: '3px', y: 8 }],
  ['keyboard-skip-enter', 'press:Enter'],
  ['keyboard-work-focus', 'evaluate:#work-surface', true],
  ['keyboard-submit-focus', 'focus:#submit-action'],
  ['keyboard-submit-enter', 'press:Enter'],
  ['keyboard-dialog-open', 'evaluate:#action-dialog', true],
  ...[1, 2, 3, 4, 5].flatMap(index => [[`keyboard-dialog-tab-${index}`, 'press:Tab'], [`keyboard-dialog-tab-${index}`, 'evaluate:#action-dialog', true]]),
  ['keyboard-dialog-escape', 'press:Escape'],
  ['keyboard-dialog-closed', 'evaluate:#action-dialog', false],
  ['keyboard-return-focus', 'evaluate:#submit-action', true],
  ['keyboard-draft-read', 'inputValue:#draft', 'Synthetic private draft'],
  ['keyboard-repeat-enter', 'press:Enter'],
  ['keyboard-cancel-click', 'click:#dialog-cancel'],
  ['keyboard-draft-preserved', 'inputValue:#draft', 'Synthetic private draft'],
  ['keyboard-scenario-preserved', 'inputValue:#scenario', 'normal'],
];
const stages = [...new Set(sequence.map(step => step[0]))];
function fakePage({ failAt = -1, wrongAt = -1, diagnostic } = {}) {
  let index = 0, diagnosticPending = false; const actual = [];
  async function step(operation, callback) {
    if (diagnosticPending) {
      diagnosticPending = false; actual.push(operation);
      assert.equal(operation, 'evaluate:#action-dialog', 'Failure diagnostic is one bounded observation');
      if (!diagnostic) throw Error('DYNAMIC_DIAGNOSTIC_ERROR /private/synthetic/path');
      return diagnostic(callback);
    }
    const current = index++, expected = sequence[current]; actual.push(operation);
    assert.equal(operation, expected?.[1], 'The original keyboard operation sequence must not change');
    if (current === failAt) throw Error('DYNAMIC_DOM_BODY /private/synthetic/path fake-credential-value');
    if (current === wrongAt) {
      diagnosticPending = expected[0].startsWith('keyboard-dialog-tab-');
      return expected[2] === true ? false : 'unexpected';
    }
    return expected[2];
  }
  return { actual, page: { keyboard: { press: key => step(`press:${key}`) }, locator: selector => ({
    evaluate: callback => step(`evaluate:${selector}`, callback), focus: () => step(`focus:${selector}`),
    inputValue: () => step(`inputValue:${selector}`), click: () => step(`click:${selector}`),
  }) } };
}
test('keyboard diagnostics preserve every operation and assertion in the original sequence', async () => {
  const { page, actual } = fakePage(), reported = [];
  await assertKeyboard(page, stage => reported.push(stage));
  assert.deepEqual(actual, sequence.map(step => step[1])); assert.deepEqual(reported, stages);
  assert.deepEqual(KEYBOARD_STAGES, stages);
});
for (const stage of stages) test(`keyboard fault reports fixed stage ${stage}`, async () => {
  const failAt = sequence.findIndex(step => step[0] === stage), { page } = fakePage({ failAt }); let category = 'keyboard';
  await assert.rejects(assertKeyboard(page, value => { category = value; }));
  assert.equal(safeFailure(category), `Organization D2 qualification failed: ${stage}`);
});
for (const stage of ['keyboard-skip-focus', 'keyboard-work-focus', 'keyboard-dialog-open', 'keyboard-dialog-tab-1', 'keyboard-dialog-closed', 'keyboard-return-focus', 'keyboard-draft-preserved', 'keyboard-scenario-preserved']) test(`keyboard assertion still fails at ${stage}`, async () => {
  const wrongAt = sequence.findIndex(step => step[0] === stage && (step[1].startsWith('evaluate:') || step[1].startsWith('inputValue:')));
  const { page } = fakePage({ wrongAt }); let category = 'keyboard';
  await assert.rejects(assertKeyboard(page, value => { category = value; }));
  assert.equal(safeFailure(category), `Organization D2 qualification failed: ${stage}`);
});
test('unknown or dynamic keyboard diagnostic text cannot escape the fixed allowlist', () => {
  for (const category of ['keyboard-skip-focus DYNAMIC_DOM_BODY', 'keyboard-dialog-tab-6', '/private/synthetic/path', new Error('DYNAMIC_DOM_BODY'), { stage: 'keyboard-return-focus' }, undefined, null]) {
    assert.equal(safeFailure(category), 'Organization D2 qualification failed: internal');
  }
});

const activeCategories = ['none', 'body', 'root', 'dialog', 'inside', 'outside'];
// Invoke the actual browser callback against identities only, without rendering.
// Arbitrary node properties throw, proving no tag, ID, text or value is needed.
function observe(callback, activeCategory, focused) {
  const opaque = () => new Proxy({}, { get() { throw Error('PRIVATE_NODE_PROPERTY'); } });
  const body = opaque(), root = opaque(), inside = opaque(), outside = opaque();
  const dialog = { contains: node => node === dialog || node === inside };
  const active = { none: null, body, root, dialog, inside, outside }[activeCategory];
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    body, documentElement: root, activeElement: active, hasFocus: () => focused,
  } });
  try { return callback(dialog); } finally {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else delete globalThis.document;
  }
}
for (const tab of [1, 2, 3, 4, 5]) for (const active of activeCategories) for (const focused of [true, false]) {
  test(`failed Tab ${tab} reports only ${active} / document focused ${focused}`, async () => {
    const stage = `keyboard-dialog-tab-${tab}`;
    const wrongAt = sequence.findIndex(step => step[0] === stage && step[1].startsWith('evaluate:'));
    const { page, actual } = fakePage({ wrongAt, diagnostic: callback => observe(callback, active, focused) });
    const reported = []; let failure;
    try { await assertKeyboard(page, value => reported.push(value)); } catch (error) { failure = error; }
    assert.equal(failure?.code, 'ERR_ASSERTION');
    assert.equal(failure.actual, false); assert.equal(failure.expected, true);
    const combined = `${stage}-active-${active}-document-${focused ? 'focused' : 'unfocused'}`;
    assert.equal(reported.at(-1), combined);
    assert.equal(safeFailure(combined), `Organization D2 qualification failed: ${combined}`);
    assert.deepEqual(actual, [...sequence.slice(0, wrongAt + 1).map(step => step[1]), 'evaluate:#action-dialog']);
  });
}
for (const value of ['body', 'body-document-focused DYNAMIC_DOM_BODY', '/private/synthetic/path', null, undefined, {}, new Error('DYNAMIC_DOM_BODY')]) {
  test('unknown observed category preserves failed containment without leaking payload', async () => {
    const stage = 'keyboard-dialog-tab-2';
    const wrongAt = sequence.findIndex(step => step[0] === stage && step[1].startsWith('evaluate:'));
    const { page } = fakePage({ wrongAt, diagnostic: () => value }); const reported = []; let failure;
    try { await assertKeyboard(page, item => reported.push(item)); } catch (error) { failure = error; }
    assert.equal(failure?.code, 'ERR_ASSERTION'); assert.equal(failure.actual, false);
    assert.equal(reported.at(-1), stage);
    assert.equal(safeFailure(`${stage}-active-${typeof value === 'string' ? value : 'unknown'}`), 'Organization D2 qualification failed: internal');
  });
}
test('combined focus categories are allowed only for the five dialog Tab stages', () => {
  for (const stage of ['keyboard-skip-focus', 'keyboard-dialog-tab-0', 'keyboard-dialog-tab-6', 'keyboard-dialog-escape']) {
    assert.equal(safeFailure(`${stage}-active-body-document-focused`), 'Organization D2 qualification failed: internal');
  }
});
