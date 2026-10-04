import test from 'node:test';
import assert from 'node:assert/strict';
import { assertJapanesePlatformFont } from '../../../apps/document-web/e2e-runtime/japanese-font.ts';
const correct = [{ familyName: 'Kosugi', postScriptName: 'Kosugi-Regular', isCustomFont: false, glyphCount: 5 }];
test('actual rendered Japanese requires the exact regular system font and every expected glyph', () => {
  assert.doesNotThrow(() => assertJapanesePlatformFont(correct, 'フォルダー'));
  for (const fonts of [[], [{ ...correct[0], glyphCount: 0 }], [{ ...correct[0], glyphCount: 4 }],
    [{ ...correct[0], familyName: 'Arial' }], [{ ...correct[0], postScriptName: '.notdef' }],
    [{ ...correct[0], isCustomFont: true }], [...correct, { ...correct[0], familyName: 'DejaVu Sans', glyphCount: 1 }]]) {
    assert.throws(() => assertJapanesePlatformFont(fonts, 'フォルダー'), /Japanese/);
  }
  assert.throws(() => assertJapanesePlatformFont(correct, ''), /Japanese/);
  assert.throws(() => assertJapanesePlatformFont(correct, 'Ascii'), /Japanese/);
});

test('hosted normal journey installs before the browser and requires actual application selection before capture', async () => {
  const { readFile } = await import('node:fs/promises');
  const workflow = await readFile(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const spec = await readFile(new URL('../../../apps/document-web/e2e-runtime/document-runtime.spec.ts', import.meta.url), 'utf8');
  assert.match(workflow, /- name: Install verified Japanese runner font\n\s+run: node tools\/document-poc-runtime\/runner-font.mjs/);
  assert.ok(workflow.indexOf('Install verified Japanese runner font') < workflow.indexOf('Real composition-root acceptance'));
  assert.match(spec, /completed\('gui-loaded'\);\n  await assertApplicationJapaneseFonts\(page\);\n  test\.info\(\)\.annotations\.push\(\{ type: 'runtime-font', description: 'kosugi-regular-japanese-heading-body' \}\);/);
  assert.ok(spec.indexOf('await assertApplicationJapaneseFonts(page)') < spec.indexOf("await visualCheckpoint(page, '01-list-context-1440.png')"));
});
