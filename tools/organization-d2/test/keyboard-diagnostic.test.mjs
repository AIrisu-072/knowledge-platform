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
function fakePage({ failAt = -1, wrongAt = -1 } = {}) {
  let index = 0; const actual = [];
  async function step(operation) {
    const current = index++, expected = sequence[current]; actual.push(operation);
    assert.equal(operation, expected?.[1], 'The original keyboard operation sequence must not change');
    if (current === failAt) throw Error('DYNAMIC_DOM_BODY /private/synthetic/path fake-credential-value');
    if (current === wrongAt) return expected[2] === true ? false : 'unexpected';
    return expected[2];
  }
  return { actual, page: { keyboard: { press: key => step(`press:${key}`) }, locator: selector => ({
    evaluate: () => step(`evaluate:${selector}`), focus: () => step(`focus:${selector}`),
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
