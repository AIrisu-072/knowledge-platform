// Optional DOM-unit harness. Reuse an already qualified jsdom installation;
// never install dependencies for this source-design packet. This is not browser proof.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');
const modulePath = process.env.ORG_DESIGN_JSDOM;
const JSDOM = modulePath ? (await import(pathToFileURL(modulePath).href)).JSDOM : null;
function fixture(archetype) {
  const dom = new JSDOM(read(`${archetype}.html`), { url: `http://source-design.invalid/${archetype}.html`, runScripts: 'outside-only' });
  const w = dom.window;
  // jsdom does not implement native dialog rendering/focus trapping; tests only
  // exercise our preview handlers and explicit return-focus branch.
  w.HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  w.HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); this.dispatchEvent(new w.Event('close')); };
  w.eval(read('scenarios.js'));
  w.eval(read('prototype.js'));
  return { dom, w, d: w.document };
}
const options = { skip: !JSDOM && 'Set ORG_DESIGN_JSDOM to an existing qualified jsdom api.js; no browser qualification inferred' };

for (const archetype of ['sales', 'office']) {
  test(`${archetype}: all ten scenario states render the same semantic surfaces`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const control = d.getElementById('scenario');
    for (const state of JSON.parse(read('scenarios.json')).scenarios) {
      control.value = state.id; control.dispatchEvent(new w.Event('change'));
      assert.equal(d.getElementById('attention-label').textContent, state.attention);
      assert.ok(d.getElementById('work-surface') && d.getElementById('context-surface'));
      assert.equal(d.querySelectorAll('.global-nav a').length, 3);
      assert.equal(d.querySelector(`[data-module="${state.module}"]`).getAttribute('aria-pressed'), 'true');
    }
    dom.window.close();
  });
  test(`${archetype}: module changes preserve draft; selected task changes clear candidates`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const draft = d.getElementById('draft'); draft.value = 'Synthetic private input'; draft.dispatchEvent(new w.Event('input'));
    d.querySelector('[data-module="agent"]').click();
    d.querySelector('[data-module="evidence"]').click();
    assert.equal(d.getElementById('draft').value, 'Synthetic private input');
    d.querySelector('[data-decision="採用"]').click(); d.getElementById('dialog-confirm').click();
    assert.match(d.getElementById('context-body').textContent, /HumanDecision · 採用/);
    const buttons = d.querySelectorAll('[data-item]'); const original = buttons[0].dataset.item;
    buttons[1].click();
    assert.doesNotMatch(d.getElementById('context-body').textContent, /HumanDecision · 採用/);
    assert.notEqual(d.getElementById('draft').value, 'Synthetic private input');
    d.querySelector(`[data-item="${original}"]`).click();
    assert.equal(d.getElementById('draft').value, 'Synthetic private input');
    assert.equal(d.activeElement.dataset.item, original);
    dom.window.close();
  });
  test(`${archetype}: eligible queue does not render private details and claim is distinct`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const filter = d.getElementById('collection-filter'); filter.selectedIndex = 1; filter.dispatchEvent(new w.Event('change'));
    assert.equal(d.getElementById('draft'), null);
    assert.equal(d.querySelector('[data-decision]'), null);
    assert.ok([...d.querySelectorAll('[data-module]')].every(b => b.disabled));
    assert.equal(d.getElementById('submit-action').textContent, '担当する');
    d.getElementById('submit-action').click(); d.getElementById('dialog-cancel').click();
    assert.equal(d.getElementById('draft'), null);
    assert.equal(d.activeElement.id, 'submit-action');
    d.getElementById('submit-action').click(); d.getElementById('dialog-confirm').click();
    assert.match(d.getElementById('attention-label').textContent, /新しい担当/);
    assert.ok(d.getElementById('draft'));
    dom.window.close();
  });
  test(`${archetype}: submit is a confirmed preview and immutable handoff replaces editor`, options, () => {
    const { dom, d } = fixture(archetype);
    d.getElementById('submit-action').click();
    assert.ok(d.getElementById('draft'));
    assert.match(d.getElementById('dialog-body').textContent, /Backend成功後/);
    d.getElementById('dialog-confirm').click();
    assert.equal(d.getElementById('draft'), null);
    assert.match(d.getElementById('work-body').textContent, /handoff/);
    assert.match(d.getElementById('notice').textContent, /実際の業務は送信していません/);
    dom.window.close();
  });
  test(`${archetype}: modified HumanDecision preserves separate evidence/candidate`, options, () => {
    const { dom, d } = fixture(archetype);
    d.querySelector('[data-decision="修正"]').click();
    d.getElementById('adopted-claim').value = 'Modified synthetic claim';
    d.getElementById('dialog-confirm').click();
    const text = d.getElementById('context-body').textContent;
    assert.match(text, /HumanDecision · 修正/); assert.match(text, /Modified synthetic claim/);
    assert.match(text, /Finding · 判断候補/); assert.match(text, /EvidenceRecord · 人間が登録/);
    dom.window.close();
  });
  test(`${archetype}: claiming a non-first eligible item retains exact identity and label`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const filter = d.getElementById('collection-filter'); filter.selectedIndex = 1; filter.dispatchEvent(new w.Event('change'));
    d.querySelectorAll('[data-item]')[1].click();
    const before = d.querySelector('[data-item][aria-pressed="true"]');
    const id = before.dataset.item; const label = before.querySelector('strong').textContent;
    d.getElementById('submit-action').click();
    assert.ok(d.getElementById('dialog-body').textContent.includes(label));
    d.getElementById('dialog-confirm').click();
    assert.equal(d.querySelector('[data-item][aria-pressed="true"]').dataset.item, id);
    assert.ok(d.getElementById('work-title').textContent.includes(label));
    dom.window.close();
  });
  test(`${archetype}: handoff applies only to its target and returns to its own state`, options, () => {
    const { dom, d } = fixture(archetype);
    const originalId = d.querySelector('[data-item][aria-pressed="true"]').dataset.item;
    d.getElementById('submit-action').click(); d.getElementById('dialog-confirm').click();
    d.querySelectorAll('[data-item]')[1].click();
    assert.doesNotMatch(d.getElementById('attention-label').textContent, /提出済み/);
    assert.ok(d.getElementById('draft'));
    d.querySelector(`[data-item="${originalId}"]`).click();
    assert.match(d.getElementById('attention-label').textContent, /提出済み/);
    assert.equal(d.getElementById('draft'), null);
    dom.window.close();
  });
  test(`${archetype}: replaced decision and Agent result controls retain explicit focus`, options, () => {
    const { dom, d } = fixture(archetype);
    const accept = d.querySelector('[data-decision="採用"]'); accept.focus(); accept.click();
    d.getElementById('dialog-confirm').click();
    assert.equal(d.activeElement.dataset.decision, '採用');
    d.querySelector('[data-module="agent"]').click();
    const result = d.querySelector('[data-action="agent-result"]'); result.focus(); result.click();
    assert.equal(d.activeElement.dataset.action, 'agent-result');
    d.querySelector('[data-module="document"]').click();
    const compare = d.querySelector('[data-action="compare"]'); compare.focus(); compare.click();
    assert.equal(d.activeElement.dataset.action, 'compare');
    dom.window.close();
  });
  test(`${archetype}: required confirmation input is validated and exact return reason retained`, options, () => {
    const { dom, d } = fixture(archetype);
    d.querySelector('[data-decision="修正"]').click();
    d.getElementById('adopted-claim').value = '  ';
    d.getElementById('dialog-confirm').click();
    assert.ok(d.getElementById('action-dialog').hasAttribute('open'));
    assert.ok(d.querySelector('#dialog-body [role="alert"]'));
    assert.doesNotMatch(d.getElementById('context-body').textContent, /HumanDecision · 修正/);
    d.getElementById('dialog-cancel').click();
    d.getElementById('return-action').click();
    d.getElementById('return-reason').value = '原本の照合日を追加確認してください。';
    d.getElementById('dialog-confirm').click();
    assert.ok(d.getElementById('context-body').textContent.includes('原本の照合日を追加確認してください。'));
    dom.window.close();
  });
  test(`${archetype}: submitted membership contains only the actual chosen HumanDecision`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const draft = d.getElementById('draft'); draft.value = 'Pinned submitted draft'; draft.dispatchEvent(new w.Event('input'));
    d.getElementById('submit-action').click();
    assert.match(d.getElementById('dialog-body').textContent, /人間の判断0件/);
    d.getElementById('dialog-confirm').click();
    assert.match(d.getElementById('work-body').textContent, /人間の判断0件/);
    assert.doesNotMatch(d.getElementById('work-body').textContent, /人間の判断1件/);
    assert.match(d.getElementById('work-body').textContent, /Pinned submitted draft/);
    dom.window.close();
  });
  test(`${archetype}: comparison never reopens submitted work or clears received return context`, options, () => {
    const { dom, w, d } = fixture(archetype);
    d.getElementById('submit-action').click(); d.getElementById('dialog-confirm').click();
    d.querySelector('[data-module="document"]').click(); d.querySelector('[data-action="compare"]').click();
    assert.equal(d.getElementById('draft'), null);
    assert.match(d.getElementById('attention-label').textContent, /提出済み/);
    d.querySelector('[data-module="evidence"]').click();
    assert.ok([...d.querySelectorAll('[data-decision]')].every(b => b.disabled));
    const scenario = d.getElementById('scenario'); scenario.value = 'returned'; scenario.dispatchEvent(new w.Event('change'));
    d.querySelector('[data-module="document"]').click(); d.querySelector('[data-action="compare"]').click();
    d.querySelector('[data-module="return"]').click();
    assert.match(d.getElementById('context-body').textContent, /試行2/);
    assert.doesNotMatch(d.getElementById('context-body').textContent, /指示はありません/);
    dom.window.close();
  });
  test(`${archetype}: outbound return closes own work without exposing receiving private draft`, options, () => {
    const { dom, d } = fixture(archetype);
    d.getElementById('return-action').click();
    d.getElementById('return-reason').value = '返送先で原本を再確認してください';
    d.getElementById('dialog-confirm').click();
    assert.equal(d.getElementById('draft'), null);
    assert.match(d.getElementById('attention-label').textContent, /差戻指示済み/);
    assert.match(d.getElementById('context-body').textContent, /返送先で原本を再確認してください/);
    assert.equal(d.querySelector('[data-action="preview-returned"]'), null);
    dom.window.close();
  });
}
