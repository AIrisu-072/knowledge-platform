import assert from 'node:assert/strict';
import type { Page } from '@playwright/test';

type PlatformFont = { familyName: string; postScriptName: string; isCustomFont: boolean; glyphCount: number };
export function assertJapanesePlatformFont(fonts: PlatformFont[], text: string): void {
  assert.ok(/^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}ー]+$/u.test(text), 'Japanese font probe requires only Japanese glyphs');
  assert.ok(fonts.length === 1 && fonts[0]!.familyName === 'Kosugi' && fonts[0]!.postScriptName === 'Kosugi-Regular'
    && fonts[0]!.isCustomFont === false && fonts[0]!.glyphCount === [...text].length, 'Japanese platform font or rendered glyph coverage differs');
}

// Reads existing application DOM under its unchanged stylesheet. A font availability
// API, cmap, or synthetic test element cannot substitute for actual glyph selection.
export async function assertApplicationJapaneseFonts(page: Page): Promise<void> {
  const client = await page.context().newCDPSession(page);
  try {
    await client.send('DOM.enable');
    await client.send('CSS.enable');
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const { root } = await client.send('DOM.getDocument');
    for (const probe of [
      { selector: 'section[aria-label="フォルダー"] > h2', text: 'フォルダー', weight: '700' },
      { selector: 'section[aria-label="フォルダー"] > label', text: '配下も含める', weight: '400' },
    ]) {
      const element = page.locator(probe.selector);
      assert.ok(await element.isVisible(), 'Japanese application font probe is not visible');
      const style = await element.evaluate(node => ({ text: node.textContent?.trim(), weight: getComputedStyle(node).fontWeight, synthesis: getComputedStyle(node).fontSynthesis }));
      assert.ok(style.text === probe.text && style.weight === probe.weight && style.synthesis === 'none', 'Japanese application typography contract differs');
      const { nodeId } = await client.send('DOM.querySelector', { nodeId: root.nodeId, selector: probe.selector });
      assert.ok(nodeId > 0, 'Japanese application DOM node is unavailable');
      const { fonts } = await client.send('CSS.getPlatformFontsForNode', { nodeId });
      assertJapanesePlatformFont(fonts, probe.text);
    }
  } finally { await client.detach(); }
}
