#!/usr/bin/env node
// Test-runner resource only. No product assets, package graph, aliases, or system writes.
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { appendFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const source = 'https://raw.githubusercontent.com/googlefonts/kosugi/75171a2738135ab888549e76a9037e826094f0ce/';
export const FONT_ASSETS = Object.freeze([
  { name: 'Kosugi-Regular.ttf', path: 'fonts/ttf/Kosugi-Regular.ttf', bytes: 2288848, sha256: 'f5e81d6a6b865d9b88c54d2d3c16bcaa3b239dfcefaf2a62976ac9dc7574bab7' },
  { name: 'LICENSE.txt', path: 'LICENSE.txt', bytes: 11358, sha256: 'cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30' },
  { name: 'AUTHORS.txt', path: 'AUTHORS.txt', bytes: 258, sha256: '0805abc728c869f4a4a9936062b24d4ed08773e3ef4321cde14d02dc33612ca2' },
  { name: 'CONTRIBUTORS.txt', path: 'CONTRIBUTORS.txt', bytes: 483, sha256: '57984e9cd756b2e131a3b1a536d7eca92088f4a96ae982088886ea30d685bc8f' },
].map(Object.freeze));

export function assertSingleInstalledFont(listing, file) {
  if (listing !== `${file}\n`) throw Error('Runner font installed identity is ambiguous');
}

export function verifyFontAsset(name, bytes) {
  const asset = FONT_ASSETS.find(asset => asset.name === name);
  if (!asset) throw Error('Unknown runner font asset');
  if ((asset.bytes && bytes.length !== asset.bytes) || createHash('sha256').update(bytes).digest('hex') !== asset.sha256) {
    throw Error('Runner font asset integrity failed');
  }
  return bytes;
}
const xml = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
export function fontconfigXml(root) {
  return `<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <cachedir>${xml(join(root, 'cache/fontconfig'))}</cachedir>
  <include ignore_missing="no">/etc/fonts/fonts.conf</include>
  <dir>${xml(join(root, 'data/fonts'))}</dir>
</fontconfig>
`;
}

export function fontEnvironment(root) {
  // Absolute font/cache paths are already in the config. Global XDG overrides
  // would relocate mise/pnpm/Playwright after their tools have been installed.
  return { FONTCONFIG_FILE: join(root, 'config/fonts.conf') };
}

export async function writeRunnerFontEnvironment(file, environment) {
  await appendFile(file, `FONTCONFIG_FILE=${environment.FONTCONFIG_FILE}\n`);
}

export async function installRunnerFont(parent, fetchAsset = fetch) {
  const assets = [];
  for (const asset of FONT_ASSETS) {
    const response = await fetchAsset(`${source}${asset.path}`, { redirect: 'error', signal: AbortSignal.timeout(30_000) });
    if (!response.ok || !response.body) throw Error('Runner font download failed');
    const chunks = []; let size = 0;
    for await (const chunk of response.body) {
      size += chunk.length;
      if (size > (asset.bytes ?? 64 * 1024)) throw Error('Runner font download size exceeded');
      chunks.push(chunk);
    }
    assets.push({ name: asset.name, bytes: verifyFontAsset(asset.name, Buffer.concat(chunks)) });
  }
  // No installed file exists until font and all attribution bytes have passed.
  const root = await mkdtemp(join(resolve(parent), 'document-poc-font-'));
  try {
    if (/[\r\n]/u.test(root)) throw Error('Invalid runner font directory');
    for (const directory of ['config', 'cache/fontconfig', 'data/fonts', 'notices']) {
      await mkdir(join(root, directory), { recursive: true, mode: 0o700 });
    }
    for (const asset of assets) {
      await writeFile(join(root, asset.name.endsWith('.ttf') ? 'data/fonts' : 'notices', asset.name), asset.bytes, { mode: 0o600, flag: 'wx' });
    }
    const config = join(root, 'config/fonts.conf');
    await writeFile(config, fontconfigXml(root), { mode: 0o600, flag: 'wx' });
    return { root, env: fontEnvironment(root) };
  } catch (error) { await rm(root, { recursive: true, force: true }); throw error; }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 2 || process.platform !== 'linux' || !process.env.RUNNER_TEMP || !process.env.GITHUB_ENV) {
    throw Error('Runner font setup requires the Linux Actions runner environment');
  }
  const installed = await installRunnerFont(process.env.RUNNER_TEMP);
  try {
    await promisify(execFile)('fc-cache', [join(installed.root, 'data/fonts')], { env: { ...process.env, ...installed.env }, timeout: 30_000, maxBuffer: 64 * 1024 });
    const listing = await promisify(execFile)('fc-list', ['--format', '%{file}\n', 'Kosugi'], { env: { ...process.env, ...installed.env }, timeout: 30_000, maxBuffer: 64 * 1024 });
    assertSingleInstalledFont(listing.stdout, join(installed.root, 'data/fonts/Kosugi-Regular.ttf'));
    await writeRunnerFontEnvironment(process.env.GITHUB_ENV, installed.env);
    console.log('Verified Kosugi 4.002 Apache-2.0 runner font and attribution installed; Chromium selection remains a separate required assertion.');
  } catch (error) { await rm(installed.root, { recursive: true, force: true }); throw error; }
}
