// Browser assertions never attach Playwright traces, reports, console or DOM bodies.
import assert from 'node:assert/strict';
import { SERVED_FILES, STATES, MODULES } from './source-server.mjs';
export function requestAllowed(origin, input, method) {
  try { const url = new URL(input); return method === 'GET' && url.origin === origin && !url.username && !url.password && SERVED_FILES.includes(url.pathname.slice(1)); } catch { return false; }
}
export function assertPlatformFont(fonts, text) {
  assert.ok(/^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}ー]+$/u.test(text), 'font');
  assert.ok(fonts.length === 1 && fonts[0].familyName === 'Kosugi' && fonts[0].postScriptName === 'Kosugi-Regular'
    && fonts[0].isCustomFont === false && fonts[0].glyphCount === [...text].length, 'font');
}
export const screenshotOptions = () => ({ type: 'png', fullPage: true, scale: 'css' });
export const KEYBOARD_STAGES = Object.freeze([
  'keyboard-skip-tab', 'keyboard-skip-focus', 'keyboard-skip-visible',
  'keyboard-skip-enter', 'keyboard-work-focus', 'keyboard-submit-focus',
  'keyboard-submit-enter', 'keyboard-dialog-open',
  'keyboard-dialog-tab-1', 'keyboard-dialog-tab-2', 'keyboard-dialog-tab-3',
  'keyboard-dialog-tab-4', 'keyboard-dialog-tab-5', 'keyboard-dialog-escape',
  'keyboard-dialog-closed', 'keyboard-return-focus', 'keyboard-draft-read',
  'keyboard-repeat-enter', 'keyboard-cancel-click', 'keyboard-draft-preserved',
  'keyboard-scenario-preserved',
  'keyboard-reverse-open', 'keyboard-reverse-initial', 'keyboard-reverse-wrap',
  'keyboard-forward-wrap', 'keyboard-reverse-cancel', 'keyboard-required-open',
  'keyboard-required-initial', 'keyboard-required-next', 'keyboard-required-forward',
  'keyboard-required-reverse', 'keyboard-required-empty', 'keyboard-required-validate',
  'keyboard-required-draft', 'keyboard-required-preserved', 'keyboard-required-escape',
  'keyboard-required-closed', 'keyboard-required-return', 'keyboard-workspace-module',
  'keyboard-workspace-open', 'keyboard-workspace-disabled', 'keyboard-workspace-initial',
  'keyboard-workspace-forward', 'keyboard-workspace-reverse', 'keyboard-workspace-escape',
  'keyboard-workspace-closed', 'keyboard-workspace-return', 'keyboard-module-restore',
  'keyboard-final-draft', 'keyboard-final-scenario', 'keyboard-final-closed',
]);
const FOCUS_CATEGORIES = Object.freeze(['none', 'body', 'root', 'dialog', 'inside', 'outside']
  .flatMap(active => [`${active}-document-focused`, `${active}-document-unfocused`]));
const CONTAINMENT_STAGES = Object.freeze([1, 2, 3, 4, 5]
  .flatMap(tab => FOCUS_CATEGORIES.map(focus => `keyboard-dialog-tab-${tab}-active-${focus}`)));
export const safeFailure = category => `Organization D2 qualification failed: ${['environment', 'source', 'gate', 'prerequisites', 'browser', 'network', 'geometry', 'keyboard', 'font', 'pixels', 'export', 'cleanup', ...KEYBOARD_STAGES, ...CONTAINMENT_STAGES].includes(category) ? category : 'internal'}`;
export async function assertFonts(page, archetype) {
  const client = await page.context().newCDPSession(page);
  try {
    await client.send('DOM.enable'); await client.send('CSS.enable');
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const { root } = await client.send('DOM.getDocument');
    const probes = [
      { selector: '#collection-title', text: archetype === 'sales' ? '担当する文脈' : '処理するタスク', weight: '700' },
      { selector: '#work-body > section:first-child .definition dt:first-child', text: archetype === 'sales' ? '現在' : '確認対象', weight: '400' },
    ];
    for (const probe of probes) {
      const element = page.locator(probe.selector); assert.ok(await element.isVisible(), 'font');
      const actual = await element.evaluate(node => ({ text: node.textContent.trim(), weight: getComputedStyle(node).fontWeight, synthesis: getComputedStyle(node).fontSynthesis }));
      assert.deepEqual(actual, { text: probe.text, weight: probe.weight, synthesis: 'none' }, 'font');
      const { nodeId } = await client.send('DOM.querySelector', { nodeId: root.nodeId, selector: probe.selector }); assert.ok(nodeId > 0, 'font');
      const { fonts } = await client.send('CSS.getPlatformFontsForNode', { nodeId }); assertPlatformFont(fonts, probe.text);
    }
  } finally { await client.detach(); }
}
// Read current rendered state. Only booleans cross these assertion boundaries;
// no DOM/body/style payload is added to public receipts or failure categories.
export async function assertHandoffPresentation(page) {
  assert.equal(await page.evaluate(() => {
    const summary = document.querySelector('#work-body > section:first-child')?.textContent ?? '';
    const steps = [...document.querySelectorAll('.steps li')];
    return /提出済み/.test(document.querySelector('#work-title')?.textContent ?? '')
      && /選択中のタスク.*完了/.test(summary) && /審査.*ready/.test(summary)
      && /提出内容・履歴を確認.*読み取り専用/.test(summary)
      && !/次は審査担当へ提出|確認事項と説明資料をまとめる|今回確認すること/.test(summary)
      && steps.length === 4 && /内容確認.*完了/.test(steps[1].textContent)
      && !steps[1].classList.contains('current') && /審査.*ready/.test(steps[2].textContent)
      && steps[2].getAttribute('aria-current') === 'step' && steps[3].classList.contains('future')
      && !document.querySelector('#draft')
      && document.querySelector('#submit-action')?.textContent === '提出内容を確認'
      && ['hold-action', 'return-action', 'assignment-action'].every(id => document.getElementById(id)?.disabled);
  }), true, 'source');
}
export async function assertDisabledPrimary(page, selector) {
  const button = page.locator(selector);
  const disabledStyle = node => {
    const style = getComputedStyle(node);
    return node.disabled && style.backgroundColor === 'rgb(244, 246, 248)'
      && style.color === 'rgb(95, 112, 128)' && style.borderTopStyle === 'dashed'
      && style.borderTopColor === 'rgb(138, 153, 167)' && style.cursor === 'not-allowed';
  };
  assert.equal(await button.evaluate(disabledStyle), true, 'source');
  await button.hover();
  assert.equal(await button.evaluate(disabledStyle), true, 'source');
  await button.focus();
  assert.equal(await button.evaluate(node => node.disabled && node !== document.activeElement), true, 'source');
}
export async function assertStateTransitions(page) {
  // Runs on each normal non-recording archetype/width after the unchanged keyboard
  // assertions. No new screenshots, retry, wait or state-machine semantics.
  await page.locator('[data-module="resources"]').click();
  await page.locator('[data-action="create-workspace"]').click();
  await assertDisabledPrimary(page, '#dialog-confirm');
  await page.locator('#dialog-cancel').click();
  await page.locator('#submit-action').click();
  await page.locator('#dialog-confirm').click();
  await assertHandoffPresentation(page);
  const membership = await page.locator('#work-body > section:last-child').textContent();
  await page.locator('[data-module="document"]').click();
  await page.locator('[data-action="compare"]').click();
  await assertHandoffPresentation(page);
  await page.locator('[data-module="evidence"]').click();
  await assertHandoffPresentation(page);
  await page.locator('[data-action="close-compare"]').click();
  await assertHandoffPresentation(page);
  assert.equal(await page.locator('#work-body > section:last-child').textContent() === membership, true, 'source');
  await page.locator('#scenario').selectOption('normal');
  assert.equal(await page.locator('#draft').count(), 1, 'source');
}
export async function selectScenario(page, state, scenarios) {
  assert.ok(STATES.includes(state), 'source');
  await page.locator('#scenario').selectOption(state);
  const expected = scenarios.find(scenario => scenario.id === state);
  assert.ok(expected && expected.module === MODULES[STATES.indexOf(state)], 'source');
  assert.equal(await page.locator('#scenario').inputValue(), state, 'source');
  assert.equal(await page.locator('#attention-label').textContent(), expected.attention, 'source');
  assert.equal(await page.locator('#state-description').textContent(), expected.work, 'source');
  assert.equal(await page.locator('[data-module][aria-pressed="true"]').count(), 1, 'source');
  assert.equal(await page.locator(`[data-module="${expected.module}"]`).getAttribute('aria-pressed'), 'true', 'source');
  const url = new URL(page.url()); assert.equal(url.searchParams.get('scenario'), state, 'source'); assert.equal(url.searchParams.get('module'), expected.module, 'source');
  assert.equal(await page.locator('.global-nav a').count(), 3, 'source');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'source');
  if (state === 'handed_off') await assertHandoffPresentation(page);
  if (state === 'blocked') {
    assert.equal(await page.evaluate(() => {
      const button = document.getElementById('submit-action'), reason = button.nextElementSibling;
      return button.disabled && reason.id === 'submit-unavailable' && !reason.hidden
        && button.getAttribute('aria-describedby') === reason.id
        && /提出不可.*原本の現在権限を確認できません/.test(reason.textContent)
        && reason.getClientRects().length > 0;
    }), true, 'source');
    await assertDisabledPrimary(page, '#submit-action');
    await page.locator('#work-title').hover();
    assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'source');
  }
}
export async function assertGeometry(page, origin, width) {
  assert.equal(new URL(page.url()).origin, origin, 'network');
  assert.deepEqual(page.viewportSize(), { width, height: 900 }, 'geometry');
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]') && !document.getAnimations().some(animation => animation.pending || animation.playState === 'running'), undefined, { timeout: 5000 });
  const geometry = await page.evaluate(() => {
    const root = document.documentElement;
    const regions = ['.global-nav', '#task-collection', '#work-surface', '#context-surface'].map(selector => {
      const el = document.querySelector(selector), r = el.getBoundingClientRect(); return { left: r.left, right: r.right, width: r.width };
    });
    const selectors = ['#work-title', '#attention-label', '#state-description', '#action-surface', '#context-body', '#collection-title', '.module-switch button', '#action-surface button'];
    const contained = selectors.every(selector => [...document.querySelectorAll(selector)].every(el => {
      const r = el.getBoundingClientRect(); return r.width > 0 && r.left >= 0 && r.right <= innerWidth + 1 && r.bottom + scrollY <= root.scrollHeight + 1 && el.scrollWidth <= el.clientWidth + 1;
    }));
    return { width: root.scrollWidth, height: root.scrollHeight, regions, contained };
  });
  assert.ok(geometry.width <= width && geometry.height >= 900 && geometry.height <= 2400 && geometry.contained, 'geometry');
  for (let i = 1; i < geometry.regions.length; i++) assert.ok(geometry.regions[i - 1].right <= geometry.regions[i].left + 1 && geometry.regions[i].width > 0, 'geometry');
  return geometry.height;
}
export async function assertKeyboard(page, onStage = () => {}) {
  // Preserve the original sequence before the appended edge/field checks.
  // Only allowlisted focus categories may describe a failed original Tab.
  onStage('keyboard-skip-tab');
  await page.keyboard.press('Tab');
  onStage('keyboard-skip-focus');
  assert.equal(await page.locator('.skip').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-skip-visible');
  const focus = await page.locator('.skip').evaluate(node => ({ style: getComputedStyle(node).outlineStyle, width: getComputedStyle(node).outlineWidth, y: node.getBoundingClientRect().y }));
  assert.ok(focus.style !== 'none' && parseFloat(focus.width) >= 2 && focus.y >= 0, 'keyboard');
  onStage('keyboard-skip-enter');
  await page.keyboard.press('Enter');
  onStage('keyboard-work-focus');
  assert.equal(await page.locator('#work-surface').evaluate(node => node === document.activeElement), true, 'keyboard');
  const submit = page.locator('#submit-action');
  onStage('keyboard-submit-focus');
  await submit.focus();
  onStage('keyboard-submit-enter');
  await page.keyboard.press('Enter');
  onStage('keyboard-dialog-open');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), true, 'keyboard');
  for (let i = 0; i < 5; i++) {
    onStage(`keyboard-dialog-tab-${i + 1}`);
    await page.keyboard.press('Tab');
    const contained = await page.locator('#action-dialog').evaluate(node => node.contains(document.activeElement));
    if (contained !== true) {
      // One post-failure snapshot, never a retry or substitute for containment.
      // Unknown values/errors retain the original fixed stage; no payload escapes.
      try {
        const focus = await page.locator('#action-dialog').evaluate(node => {
          const active = document.activeElement;
          const category = !active ? 'none' : active === document.body ? 'body'
            : active === document.documentElement ? 'root' : active === node ? 'dialog'
              : node.contains(active) ? 'inside' : 'outside';
          return `${category}-document-${document.hasFocus() ? 'focused' : 'unfocused'}`;
        });
        if (FOCUS_CATEGORIES.includes(focus)) onStage(`keyboard-dialog-tab-${i + 1}-active-${focus}`);
      } catch { /* The original assertion below must still fail. */ }
    }
    assert.equal(contained, true, 'keyboard');
  }
  onStage('keyboard-dialog-escape');
  await page.keyboard.press('Escape');
  onStage('keyboard-dialog-closed');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'keyboard');
  onStage('keyboard-return-focus');
  assert.equal(await submit.evaluate(node => node === document.activeElement), true, 'keyboard');
  // Repeat/cancel must preserve the private input and scenario rather than commit.
  onStage('keyboard-draft-read');
  const before = await page.locator('#draft').inputValue();
  onStage('keyboard-repeat-enter');
  await page.keyboard.press('Enter');
  onStage('keyboard-cancel-click');
  await page.locator('#dialog-cancel').click();
  onStage('keyboard-draft-preserved');
  assert.equal(await page.locator('#draft').inputValue(), before, 'keyboard');
  onStage('keyboard-scenario-preserved');
  assert.equal(await page.locator('#scenario').inputValue(), 'normal', 'keyboard');
  // New checks use real native key presses, never a synthetic Tab implementation.
  onStage('keyboard-reverse-open');
  await page.keyboard.press('Enter');
  onStage('keyboard-reverse-initial');
  assert.equal(await page.locator('#dialog-cancel').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-reverse-wrap');
  await page.keyboard.press('Shift+Tab');
  assert.equal(await page.locator('#dialog-confirm').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-forward-wrap');
  await page.keyboard.press('Tab');
  assert.equal(await page.locator('#dialog-cancel').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-reverse-cancel');
  await page.locator('#dialog-cancel').click();

  onStage('keyboard-required-open');
  await page.locator('#return-action').click();
  onStage('keyboard-required-initial');
  assert.equal(await page.locator('#dialog-cancel').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-required-next');
  await page.keyboard.press('Tab');
  assert.equal(await page.locator('#dialog-confirm').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-required-forward');
  await page.keyboard.press('Tab');
  assert.equal(await page.locator('#return-reason').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-required-reverse');
  await page.keyboard.press('Shift+Tab');
  assert.equal(await page.locator('#dialog-confirm').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-required-empty');
  await page.locator('#return-reason').fill('');
  onStage('keyboard-required-validate');
  await page.locator('#dialog-confirm').click();
  assert.equal(await page.locator('#return-reason').evaluate(node => node === document.activeElement && node.getAttribute('aria-invalid') === 'true' && node.closest('dialog').open), true, 'keyboard');
  onStage('keyboard-required-draft');
  await page.locator('#return-reason').fill('Synthetic uncommitted reason');
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Tab');
  onStage('keyboard-required-preserved');
  assert.equal(await page.locator('#return-reason').inputValue(), 'Synthetic uncommitted reason', 'keyboard');
  onStage('keyboard-required-escape');
  await page.keyboard.press('Escape');
  onStage('keyboard-required-closed');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'keyboard');
  onStage('keyboard-required-return');
  assert.equal(await page.locator('#return-action').evaluate(node => node === document.activeElement), true, 'keyboard');

  onStage('keyboard-workspace-module');
  await page.locator('[data-module="resources"]').click();
  onStage('keyboard-workspace-open');
  await page.locator('[data-action="create-workspace"]').click();
  onStage('keyboard-workspace-disabled');
  assert.equal(await page.locator('#dialog-confirm').evaluate(node => node.disabled), true, 'keyboard');
  onStage('keyboard-workspace-initial');
  assert.equal(await page.locator('#dialog-cancel').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-workspace-forward');
  await page.keyboard.press('Tab');
  assert.equal(await page.locator('#workspace-name').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-workspace-reverse');
  await page.keyboard.press('Shift+Tab');
  assert.equal(await page.locator('#dialog-cancel').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-workspace-escape');
  await page.keyboard.press('Escape');
  onStage('keyboard-workspace-closed');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'keyboard');
  onStage('keyboard-workspace-return');
  assert.equal(await page.locator('[data-action="create-workspace"]').evaluate(node => node === document.activeElement), true, 'keyboard');
  onStage('keyboard-module-restore');
  await page.locator('[data-module="evidence"]').click();
  onStage('keyboard-final-draft');
  assert.equal(await page.locator('#draft').inputValue(), before, 'keyboard');
  onStage('keyboard-final-scenario');
  assert.equal(await page.locator('#scenario').inputValue(), 'normal', 'keyboard');
  onStage('keyboard-final-closed');
  assert.equal(await page.locator('#action-dialog').evaluate(node => node.open), false, 'keyboard');
}

export async function assertReducedMotion(page) {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  assert.equal(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches), true, 'geometry');
  assert.equal(await page.evaluate(() => [...document.querySelectorAll('*')].every(node => {
    const style = getComputedStyle(node); return style.animationName === 'none' && style.transitionProperty === 'none' && style.scrollBehavior === 'auto';
  })), true, 'geometry');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
}
