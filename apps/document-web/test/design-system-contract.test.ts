import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { expect, test } from '@jest/globals';

const tokensPath = resolve(process.cwd(), 'src/design-system/tokens.css');

test('design tokens preserve the approved canvas, typography, density, and semantic colors', async () => {
  const css = await readFile(tokensPath, 'utf8');

  for (const token of [
    '--font-sans: "Hiragino Sans", "Yu Gothic UI", "Yu Gothic", "Noto Sans JP", system-ui, sans-serif',
    '--color-focus: #0b6fc2',
    '--color-canvas: #f4f6f8',
    '--color-surface: #ffffff',
    '--color-text: #18232e',
    '--color-text-muted: #536474',
    '--color-accent: #1f5f99',
    '--color-success: #26724c',
    '--color-warning: #8a5a00',
    '--color-danger: #aa2f2f',
    '--header-height: 52px',
    '--nav-width: 240px',
    '--context-width: 340px',
    '--row-height: 48px',
    '--control-height: 34px',
    '--motion-instant: 0ms',
    '--motion-fast: 90ms',
    '--motion-standard: 140ms',
    '--motion-spatial: 180ms',
  ]) {
    expect(css).toContain(token);
  }
});

test('reduced motion removes CSS transitions and uses automatic scrolling', async () => {
  const css = await readFile(tokensPath, 'utf8');

  expect(css).toContain('@media (prefers-reduced-motion: reduce)');
  expect(css).toMatch(/--motion-instant:\s*0ms/);
  expect(css).toMatch(/--motion-fast:\s*0ms/);
  expect(css).toMatch(/--motion-standard:\s*0ms/);
  expect(css).toMatch(/--motion-spatial:\s*0ms/);
  expect(css).toMatch(/scroll-behavior:\s*auto/);
});

test('design token source exists at the agreed application path', () => {
  expect(tokensPath).toContain('/apps/document-web/src/design-system/tokens.css');
});

test('global styles keep a visible keyboard focus ring', async () => {
  const css = await readFile(resolve(process.cwd(), 'src/design-system/global.css'), 'utf8');

  expect(css).toMatch(/:focus-visible\s*\{/);
  expect(css).toContain('outline: 2px solid var(--color-focus)');
  expect(css).toContain('outline-offset: 2px');
});
