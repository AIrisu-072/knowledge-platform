// Optional DOM-unit harness. Reuse an already qualified jsdom installation;
// never install dependencies for this source-design packet. This is not browser proof.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');
const modulePath = process.env.ORG_DESIGN_JSDOM;
const JSDOM = modulePath ? (await import(pathToFileURL(modulePath).href)).JSDOM : null;
function fixture(archetype, scenario = 'normal') {
  const dom = new JSDOM(read(`${archetype}.html`), { url: `http://source-design.invalid/${archetype}.html?scenario=${scenario}`, runScripts: 'outside-only' });
  const w = dom.window;
  // jsdom does not implement native dialog rendering/focus trapping; tests only
  // exercise our preview handlers and explicit return-focus branch.
  // Geometry is absent too: this narrow shim marks display/hidden controls as
  // unrendered solely to test the edge handler. Hosted Chromium owns real layout.
  w.HTMLElement.prototype.getClientRects = function () {
    for (let node = this; node; node = node.parentElement) {
      if (node.hidden || w.getComputedStyle(node).display === 'none') return [];
    }
    return this.matches('input[type="hidden"]') ? [] : [{}];
  };
  w.HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  w.HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); this.dispatchEvent(new w.Event('close')); };
  w.eval(read('scenarios.js'));
  w.eval(read('prototype.js'));
  return { dom, w, d: w.document };
}
const options = { skip: !JSDOM && 'Set ORG_DESIGN_JSDOM to an existing qualified jsdom api.js; no browser qualification inferred' };

for (const archetype of ['sales', 'office']) {
  function key(w, target, key = 'Tab', modifiers = {}) {
    const event = new w.KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...modifiers });
    target.dispatchEvent(event); return event;
  }
  test(`${archetype}: dialog wraps forward and reverse only at the two-button edges`, options, () => {
    const { dom, w, d } = fixture(archetype);
    d.getElementById('submit-action').click();
    const cancel = d.getElementById('dialog-cancel'), confirm = d.getElementById('dialog-confirm');
    assert.equal(d.activeElement, cancel, 'High-impact initial focus stays on Cancel');
    assert.equal(key(w, cancel).defaultPrevented, false, 'Ordinary forward order remains native');
    confirm.focus();
    assert.equal(key(w, confirm).defaultPrevented, true, 'Last Tab must wrap');
    assert.equal(d.activeElement, cancel);
    assert.equal(key(w, cancel, 'Tab', { shiftKey: true }).defaultPrevented, true, 'First Shift+Tab must wrap');
    assert.equal(d.activeElement, confirm);
    assert.equal(key(w, confirm, 'Tab', { shiftKey: true }).defaultPrevented, false);
    dom.window.close();
  });
  for (const [trigger, fieldId] of [['#return-action', 'return-reason'], ['[data-decision="修正"]', 'adopted-claim']]) {
    test(`${archetype}: ${fieldId} is the first boundary and validation retains input`, options, () => {
      const { dom, w, d } = fixture(archetype);
      d.querySelector(trigger).click();
      const field = d.getElementById(fieldId), cancel = d.getElementById('dialog-cancel'), confirm = d.getElementById('dialog-confirm');
      assert.equal(d.activeElement, cancel);
      assert.equal(key(w, cancel, 'Tab', { shiftKey: true }).defaultPrevented, false, 'Middle reverse order stays native');
      field.focus(); field.value = '  ';
      assert.equal(key(w, field, 'Tab', { shiftKey: true }).defaultPrevented, true);
      assert.equal(d.activeElement, confirm);
      assert.equal(key(w, confirm).defaultPrevented, true);
      assert.equal(d.activeElement, field);
      assert.equal(key(w, field).defaultPrevented, false);
      confirm.click();
      assert.equal(d.getElementById('action-dialog').open, true);
      assert.equal(d.activeElement, field);
      assert.equal(field.value, '  ');
      assert.equal(field.getAttribute('aria-invalid'), 'true');
      dom.window.close();
    });
  }
  test(`${archetype}: Workspace input wraps past disabled Confirm and one eligible control self-wraps`, options, () => {
    const { dom, w, d } = fixture(archetype);
    d.querySelector('[data-module="resources"]').click();
    d.querySelector('[data-action="create-workspace"]').click();
    const field = d.getElementById('workspace-name'), cancel = d.getElementById('dialog-cancel');
    assert.equal(d.getElementById('dialog-confirm').disabled, true);
    assert.equal(key(w, cancel).defaultPrevented, true);
    assert.equal(d.activeElement, field);
    assert.equal(key(w, field, 'Tab', { shiftKey: true }).defaultPrevented, true);
    assert.equal(d.activeElement, cancel);
    field.hidden = true;
    for (const shiftKey of [false, true]) {
      assert.equal(key(w, cancel, 'Tab', { shiftKey }).defaultPrevented, true);
      assert.equal(d.activeElement, cancel);
    }
    dom.window.close();
  });
  test(`${archetype}: dialog boundaries exclude hidden inert disabled and negative-tabindex controls`, options, () => {
    const { dom, w, d } = fixture(archetype);
    d.getElementById('submit-action').click();
    const dialog = d.getElementById('action-dialog'), cancel = d.getElementById('dialog-cancel'), confirm = d.getElementById('dialog-confirm');
    const excluded = '<button hidden>Hidden</button><div style="display:none"><input></div><input type="hidden"><button style="visibility:hidden">Invisible</button><button style="visibility:collapse">Collapsed</button><div inert><button>Inert</button></div><fieldset disabled><input></fieldset><button tabindex="-1">Programmatic</button>';
    dialog.insertAdjacentHTML('afterbegin', excluded); dialog.insertAdjacentHTML('beforeend', excluded);
    confirm.focus(); assert.equal(key(w, confirm).defaultPrevented, true); assert.equal(d.activeElement, cancel);
    assert.equal(key(w, cancel, 'Tab', { shiftKey: true }).defaultPrevented, true); assert.equal(d.activeElement, confirm);
    dom.window.close();
  });
  test(`${archetype}: dialog leaves modified keys non-Tab closed and outside events untouched`, options, () => {
    const { dom, w, d } = fixture(archetype);
    d.getElementById('submit-action').click();
    const confirm = d.getElementById('dialog-confirm'); confirm.focus();
    for (const modifier of ['ctrlKey', 'altKey', 'metaKey']) for (const shiftKey of [false, true]) {
      assert.equal(key(w, confirm, 'Tab', { [modifier]: true, shiftKey }).defaultPrevented, false);
      assert.equal(d.activeElement, confirm);
    }
    for (const other of ['Enter', 'Escape', 'ArrowRight', ' ']) assert.equal(key(w, confirm, other).defaultPrevented, false);
    const prevented = new w.KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    prevented.preventDefault(); confirm.dispatchEvent(prevented); assert.equal(d.activeElement, confirm);
    const outside = d.getElementById('submit-action'); outside.focus();
    assert.equal(key(w, outside).defaultPrevented, false); assert.equal(d.activeElement, outside);
    d.getElementById('dialog-cancel').click(); confirm.focus();
    assert.equal(key(w, confirm).defaultPrevented, false); assert.equal(d.activeElement, confirm);
    dom.window.close();
  });
  test(`${archetype}: native Escape cancellation and Cancel preserve draft scenario and return focus`, options, () => {
    const { dom, w, d } = fixture(archetype);
    const draft = d.getElementById('draft'); draft.value = 'Synthetic preserved private memo'; draft.dispatchEvent(new w.Event('input'));
    const trigger = d.getElementById('submit-action'), dialog = d.getElementById('action-dialog');
    for (const dismissal of ['escape', 'cancel', 'disabled-trigger', 'removed-trigger']) {
      trigger.click();
      if (dismissal === 'escape') {
        assert.equal(key(w, d.activeElement, 'Escape').defaultPrevented, false);
        const event = new w.Event('cancel', { cancelable: true }); dialog.dispatchEvent(event);
        assert.equal(event.defaultPrevented, false, 'Native Escape cancellation is not suppressed');
        dialog.close(); // Emulate native default; jsdom has no keyboard default action.
      } else {
        if (dismissal === 'disabled-trigger') trigger.disabled = true;
        if (dismissal === 'removed-trigger') trigger.remove();
        d.getElementById('dialog-cancel').click();
      }
      assert.equal(dialog.open, false);
      assert.equal(d.activeElement, ['disabled-trigger', 'removed-trigger'].includes(dismissal) ? d.getElementById('work-surface') : trigger);
      assert.equal(draft.value, 'Synthetic preserved private memo');
      assert.equal(d.getElementById('scenario').value, 'normal');
      trigger.disabled = false;
    }
    dom.window.close();
  });
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

// These are presentation regressions, not renderer or backend acceptance.
for (const archetype of ['sales', 'office']) {
  function completedProjection(d) {
    assert.match(d.getElementById('work-title').textContent, /提出済み/);
    const summary = d.querySelector('#work-body > section:first-child');
    assert.match(summary.textContent, /選択中のタスク.*完了/);
    assert.match(summary.textContent, /審査.*ready/);
    assert.match(summary.textContent, /提出内容・履歴を確認.*読み取り専用/);
    assert.doesNotMatch(summary.textContent, /次は審査担当へ提出|確認事項と説明資料をまとめる|今回確認すること/);
    const steps = [...d.querySelectorAll('.steps li')];
    assert.match(steps[1].textContent, /内容確認.*完了/);
    assert.equal(steps[1].classList.contains('current'), false);
    assert.match(steps[2].textContent, /審査.*ready/);
    assert.equal(steps[2].getAttribute('aria-current'), 'step');
    assert.equal(steps[3].classList.contains('future'), true);
    assert.equal(d.getElementById('draft'), null);
    assert.equal(d.getElementById('submit-action').textContent, '提出内容を確認');
    for (const id of ['hold-action', 'return-action', 'assignment-action']) assert.equal(d.getElementById(id).disabled, true);
  }
  for (const origin of ['fixture', 'confirmed-preview']) test(`${archetype}: ${origin} handoff projects completed selection and next-ready context through navigation`, options, () => {
    const { dom, d } = fixture(archetype, origin === 'fixture' ? 'handed_off' : 'normal');
    if (origin === 'confirmed-preview') { d.getElementById('submit-action').click(); d.getElementById('dialog-confirm').click(); }
    completedProjection(d);
    const submitted = d.querySelector('#work-body > section:last-child').textContent;
    for (const module of ['evidence', 'document']) { d.querySelector(`[data-module="${module}"]`).click(); completedProjection(d); }
    d.querySelector('[data-action="compare"]').click(); completedProjection(d);
    d.querySelector('[data-action="close-compare"]').click(); completedProjection(d);
    const selected = d.querySelector('[data-item][aria-pressed="true"]').dataset.item;
    d.querySelectorAll('[data-item]')[1].click(); assert.ok(d.getElementById('draft'));
    d.querySelector(`[data-item="${selected}"]`).click(); completedProjection(d);
    assert.equal(d.querySelector('#work-body > section:last-child').textContent, submitted);
    dom.window.close();
  });
  test(`${archetype}: outgoing return uses completed historical summary without inventing a forward-ready step`, options, () => {
    const { dom, d } = fixture(archetype);
    d.getElementById('return-action').click(); d.getElementById('dialog-confirm').click();
    for (const module of ['return', 'document']) {
      d.querySelector(`[data-module="${module}"]`).click();
      const summary = d.querySelector('#work-body > section:first-child').textContent;
      assert.match(summary, /選択中のタスク.*完了/); assert.match(summary, /差戻指示・過去提出を確認.*読み取り専用/);
      assert.doesNotMatch(summary, /次は審査担当へ提出|確認事項と説明資料をまとめる|今回確認すること|審査.*ready/);
      assert.match(d.querySelector('.steps').textContent, /内容確認.*完了/);
      assert.match(d.querySelector('.steps .current').textContent, /差戻先.*ready.*新しい試行/);
      assert.equal(d.querySelector('.steps .current').getAttribute('aria-current'), 'step');
    }
    d.querySelector('[data-action="compare"]').click();
    assert.equal(d.getElementById('draft'), null);
    dom.window.close();
  });
  test(`${archetype}: normal and received-return keep editable current projection`, options, () => {
    for (const scenario of ['normal', 'returned']) {
      const { dom, d } = fixture(archetype, scenario);
      assert.equal(d.querySelector('.steps .current').textContent, '内容確認');
      assert.equal(d.getElementById('submit-action').disabled, false);
      assert.ok(d.getElementById('draft'));
      assert.match(d.querySelector('#work-body > section:first-child').textContent, archetype === 'sales' ? /確認事項と説明資料をまとめる/ : /今回確認すること/);
      dom.window.close();
    }
  });
  test(`${archetype}: blocked submit has an adjacent associated current reason and remains natively inhibited`, options, () => {
    const { dom, w, d } = fixture(archetype, 'blocked');
    const button = d.getElementById('submit-action'), reason = button.nextElementSibling;
    assert.equal(button.disabled, true);
    assert.equal(reason.id, 'submit-unavailable');
    assert.equal(button.getAttribute('aria-describedby'), reason.id);
    assert.equal(reason.hidden, false);
    assert.match(reason.textContent, /提出不可.*原本の現在権限を確認できません/);
    const input = d.getElementById('draft'); input.focus(); const draft = input.value;
    button.focus(); button.click();
    assert.equal(d.activeElement, input); assert.equal(d.getElementById('action-dialog').open, false); assert.equal(input.value, draft);
    const selector = d.getElementById('scenario'); selector.value = 'normal'; selector.dispatchEvent(new w.Event('change'));
    assert.equal(button.disabled, false); assert.equal(reason.hidden, true); assert.equal(button.hasAttribute('aria-describedby'), false);
    dom.window.close();
  });
  test(`${archetype}: disabled primary Submit and Workspace Confirm use neutral text background and dashed border`, options, () => {
    const { dom, w, d } = fixture(archetype, 'blocked');
    const style = d.createElement('style'); style.textContent = read('prototype.css'); d.head.append(style);
    const check = button => { const css = w.getComputedStyle(button); assert.equal(css.backgroundColor, 'rgb(244, 246, 248)'); assert.equal(css.color, 'rgb(95, 112, 128)'); assert.equal(css.borderTopStyle, 'dashed'); assert.equal(css.cursor, 'not-allowed'); };
    check(d.getElementById('submit-action'));
    d.querySelector('[data-module="resources"]').click(); d.querySelector('[data-action="create-workspace"]').click(); check(d.getElementById('dialog-confirm'));
    dom.window.close();
  });
}

for (const archetype of ['sales', 'office']) test(`${archetype}: real hosted handoff callback rejects a stale current rail or advice`, options, async () => {
  const { assertHandoffPresentation } = await import('../../../tools/organization-d2/browser-checks.mjs');
  const { dom, w, d } = fixture(archetype, 'handed_off');
  const page = { evaluate: async callback => w.eval(`(${callback.toString()})()`) };
  await assertHandoffPresentation(page);
  const current = d.querySelector('.steps [aria-current]'); current.removeAttribute('aria-current');
  await assert.rejects(() => assertHandoffPresentation(page)); current.setAttribute('aria-current', 'step');
  d.querySelector('#work-body > section:first-child').append('確認事項と説明資料をまとめる');
  await assert.rejects(() => assertHandoffPresentation(page));
  dom.window.close();
});

for (const archetype of ['sales', 'office']) test(`${archetype}: blocked then confirmed Return enables only its read-only instruction action`, options, () => {
  const { dom, w, d } = fixture(archetype, 'blocked');
  const button = d.getElementById('submit-action'), unavailable = d.getElementById('submit-unavailable');
  const selected = d.querySelector('[data-item][aria-pressed="true"]').dataset.item;
  const draft = d.getElementById('draft'); draft.value = 'Retained private draft before Return'; draft.dispatchEvent(new w.Event('input'));
  assert.equal(button.disabled, true); button.click();
  assert.equal(d.getElementById('action-dialog').open, false);
  assert.equal(d.getElementById('draft').value, draft.value);
  assert.match(unavailable.textContent, /提出不可.*原本の現在権限を確認できません/);
  d.getElementById('return-action').click(); d.getElementById('return-reason').value = '原本の現在権限を再確認してください'; d.getElementById('dialog-confirm').click();
  const check = () => {
    assert.equal(button.textContent, '差戻指示を確認');
    assert.equal(button.disabled, false, 'Completed read-only inspection is not blocked submission');
    assert.equal(unavailable.hidden, true); assert.equal(unavailable.textContent, '');
    assert.equal(button.hasAttribute('aria-describedby'), false);
    assert.equal(d.getElementById('draft'), null);
    assert.doesNotMatch(d.getElementById('work-body').textContent, /Retained private draft before Return/);
    assert.match(d.getElementById('state-description').textContent, /差戻先の新しい非公開下書きは表示しません/);
    for (const id of ['hold-action', 'return-action', 'assignment-action']) assert.equal(d.getElementById(id).disabled, true);
    assert.equal(d.getElementById('scenario').value, 'blocked');
    assert.equal(d.getElementById('action-dialog').open, false);
  };
  check();
  for (const module of ['evidence', 'history', 'document']) { d.querySelector(`[data-module="${module}"]`).click(); check(); }
  assert.match(d.getElementById('context-body').textContent, /providerの現在認可が不明/);
  assert.equal(d.querySelector('[data-action="open-original"]'), null);
  assert.equal(d.querySelector('[data-action="compare"]'), null);
  button.click(); check();
  assert.equal(d.querySelector('[data-module="return"]').getAttribute('aria-pressed'), 'true');
  assert.match(d.getElementById('context-body').textContent, /原本の現在権限を再確認してください/);
  assert.equal(d.querySelector('[data-action="preview-returned"]'), null);
  d.querySelectorAll('[data-item]')[1].click(); assert.ok(d.getElementById('draft'));
  d.querySelector(`[data-item="${selected}"]`).click(); check();
  dom.window.close();
});
