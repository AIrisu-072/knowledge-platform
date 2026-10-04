import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { FONT_ASSETS, assertSingleInstalledFont, fontEnvironment, fontconfigXml, installRunnerFont, verifyFontAsset, writeRunnerFontEnvironment } from '../runner-font.mjs';

const notices = ['LICENSE', 'AUTHORS', 'CONTRIBUTORS'];
test('fixed upstream notices remain byte-for-byte approved and tampering fails closed', async () => {
  assert.equal(FONT_ASSETS.length, 4);
  assert.equal(FONT_ASSETS[0].sha256, 'f5e81d6a6b865d9b88c54d2d3c16bcaa3b239dfcefaf2a62976ac9dc7574bab7');
  assert.equal(FONT_ASSETS[0].bytes, 2288848);
  for (const notice of notices) {
    const bytes = await readFile(new URL(`../fonts/${notice}.kosugi.txt`, import.meta.url));
    assert.equal(verifyFontAsset(`${notice}.txt`, bytes), bytes);
    assert.throws(() => verifyFontAsset(`${notice}.txt`, Buffer.concat([bytes, Buffer.from('tampered')])), /integrity/);
  }
  assert.throws(() => verifyFontAsset('elsewhere.ttf', Buffer.alloc(0)), /Unknown/);
  assert.throws(() => verifyFontAsset('Kosugi-Regular.ttf', Buffer.alloc(2288848)), /integrity/);
});

test('private Fontconfig preserves system discovery without aliases and puts its cache first', () => {
  const config = fontconfigXml('/tmp/private & <font>');
  assert.match(config, /<cachedir>\/tmp\/private &amp; &lt;font&gt;\/cache\/fontconfig<\/cachedir>/);
  assert.match(config, /<dir>\/tmp\/private &amp; &lt;font&gt;\/data\/fonts<\/dir>/);
  assert.ok(config.indexOf('<cachedir>') < config.indexOf('<include'));
  assert.match(config, /<include ignore_missing="no">\/etc\/fonts\/fonts.conf<\/include>/);
  assert.doesNotMatch(config, /<alias|<match|<edit/);
});

test('unverified upstream bytes never create a font installation', async () => {
  const parent = await mkdtemp(join(tmpdir(), 'runner-font-reject-'));
  try {
    await assert.rejects(installRunnerFont(parent, async () => new Response('tampered font')), /integrity/);
    assert.deepEqual(await readdir(parent), []);
  } finally { await rm(parent, { recursive: true, force: true }); }
});

test('download is immutable, bounded, redirect-free, and unsuccessful responses fail closed', async () => {
  const parent = await mkdtemp(join(tmpdir(), 'runner-font-http-'));
  try {
    let observed;
    await assert.rejects(installRunnerFont(parent, async (url, init) => {
      observed = { url, init };
      return new Response('not found', { status: 404 });
    }), /download/);
    assert.match(observed.url, /^https:\/\/raw\.githubusercontent\.com\/googlefonts\/kosugi\/75171a2738135ab888549e76a9037e826094f0ce\/fonts\/ttf\/Kosugi-Regular.ttf$/);
    assert.equal(observed.init.redirect, 'error');
    assert.ok(observed.init.signal instanceof AbortSignal);
    await assert.rejects(installRunnerFont(parent, async () => new Response(Buffer.alloc(2288849))), /size/);
    assert.deepEqual(await readdir(parent), []);
  } finally { await rm(parent, { recursive: true, force: true }); }
});


test('a same-named system font cannot impersonate the pinned installed face', () => {
  const file = '/tmp/private/data/fonts/Kosugi-Regular.ttf';
  assert.doesNotThrow(() => assertSingleInstalledFont(`${file}\n`, file));
  for (const listing of ['', '/usr/share/fonts/Kosugi-Regular.ttf\n', `${file}\n${file}\n`, `${file}\n/usr/share/fonts/another.ttf\n`]) {
    assert.throws(() => assertSingleInstalledFont(listing, file), /identity/);
  }
});


test('job font environment preserves installed mise, pnpm and Playwright roots with default or custom XDG', () => {
  const root = '/tmp/runner/document-poc-font-abc';
  const exported = fontEnvironment(root);
  for (const inherited of [
    { HOME: '/home/runner', PATH: '/home/runner/.local/share/mise/shims:/usr/bin' },
    { HOME: '/home/runner', PATH: '/tools/mise/shims:/usr/bin', XDG_CONFIG_HOME: '/original/config', XDG_CACHE_HOME: '/original/cache', XDG_DATA_HOME: '/original/data',
      MISE_DATA_DIR: '/tools/mise', MISE_CACHE_DIR: '/tools/mise-cache', MISE_CONFIG_DIR: '/tools/mise-config', PNPM_HOME: '/tools/pnpm', PLAYWRIGHT_BROWSERS_PATH: '/tools/pw' },
  ]) {
    const next = { ...inherited, ...exported };
    assert.equal(next.FONTCONFIG_FILE, `${root}/config/fonts.conf`);
    delete next.FONTCONFIG_FILE;
    assert.deepEqual(next, inherited, 'Only font selection may change; tool installation and discovery roots must survive');
  }
  assert.deepEqual(Object.keys(exported), ['FONTCONFIG_FILE']);
});


test('Actions environment output exports only the font selector, never XDG or toolchain paths', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'runner-font-env-'));
  try {
    const output = join(directory, 'github-env');
    await writeRunnerFontEnvironment(output, { ...fontEnvironment('/tmp/private-font'), XDG_DATA_HOME: '/must-not-export', PATH: '/must-not-export' });
    assert.equal(await readFile(output, 'utf8'), 'FONTCONFIG_FILE=/tmp/private-font/config/fonts.conf\n');
  } finally { await rm(directory, { recursive: true, force: true }); }
});
