import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readdir, lstat, rm, symlink, chmod, link } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { PNG_NAMES, validatePng, exportPixels } from '../pixels.mjs';
function chunk(type, data) { const body = Buffer.concat([Buffer.from(type), data]), size = Buffer.alloc(4), crc = Buffer.alloc(4); size.writeUInt32BE(data.length); crc.writeUInt32BE(crc32(body)); return Buffer.concat([size, body, crc]); }
// Parser fixtures only. These pixels never qualify a browser or user review.
function png(width = 1440, height = 900, options = {}) { const head = Buffer.alloc(13); head.writeUInt32BE(width); head.writeUInt32BE(height, 4); head[8] = 8; head[9] = 2; const raw = Buffer.alloc((width * 3 + 1) * height); if (options.filter) raw[0] = 5; return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', head), ...(options.metadata ? [chunk('tEXt', Buffer.from('synthetic metadata'))] : []), chunk('IDAT', options.deflate ? Buffer.from('bad') : deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]); }
const names = ['sales', 'office'].flatMap(page => ['normal', 'newly_assigned', 'returned', 'working_draft', 'handed_off', 'due_soon', 'blocked', 'agent_active', 'evidence_review', 'document_compare'].map(state => `${page}-${state}.png`));
test('exact twenty filenames and bounded pixel-only PNG dimensions', () => { assert.deepEqual(PNG_NAMES, names); for (const height of [900, 2400]) assert.deepEqual(validatePng(png(1440, height), names[0]), { width: 1440, height }); for (const [width, height] of [[1280, 900], [1440, 899], [1440, 2401]]) assert.throws(() => validatePng(png(width, height), names[0])); });
test('PNG rejects metadata corruption truncated data unknown names and oversized files', () => { const good = png(), corrupt = Buffer.from(good); corrupt[40] ^= 1; for (const bytes of [Buffer.from('bad'), good.subarray(0, -1), corrupt, Buffer.concat([good, Buffer.from('extra')]), png(1440, 900, { metadata: true }), png(1440, 900, { filter: true }), png(1440, 900, { deflate: true }), Buffer.alloc(8 * 1024 * 1024 + 1)]) assert.throws(() => validatePng(bytes, names[0])); assert.throws(() => validatePng(good, '../sales-normal.png')); });
async function fixture(t) { const root = await mkdtemp(join(tmpdir(), 'org-pixels-')); t.after(() => rm(root, { recursive: true, force: true })); const capture = join(root, 'capture'); await mkdir(capture, { mode: 0o700 }); const bytes = png(); for (const name of names) await writeFile(join(capture, name), bytes, { mode: 0o600 }); return { root, capture, destination: join(root, 'visual-export'), qualified: true, sourceUnchanged: true }; }
test('success exports only twenty fresh private regular PNGs atomically', async t => { const f = await fixture(t); const result = await exportPixels(f); assert.equal(result.count, 20); assert.deepEqual((await readdir(result.directory)).sort(), [...names].sort()); for (const name of names) assert.equal((await lstat(join(result.directory, name))).mode & 0o777, 0o600); await assert.rejects(exportPixels(f)); });
for (const [name, mutate] of [
  ['extra file', f => writeFile(join(f.capture, 'log.txt'), 'never export')], ['missing file', f => rm(join(f.capture, names[0]))],
  ['directory', f => mkdir(join(f.capture, 'logs'))], ['bad final PNG', f => writeFile(join(f.capture, names.at(-1)), 'invalid')],
  ['symlink', async f => { await rm(join(f.capture, names[0])); await symlink(names[1], join(f.capture, names[0])); }],
  ['hardlink', async f => { await rm(join(f.capture, names[0])); await link(join(f.capture, names[1]), join(f.capture, names[0])); }],
  ['public file', f => chmod(join(f.capture, names[0]), 0o644)], ['executable file', f => chmod(join(f.capture, names[0]), 0o700)],
  ['public directory', f => chmod(f.capture, 0o755)], ['qualification failed', f => { f.qualified = false; }], ['changed source', f => { f.sourceUnchanged = false; }],
]) test(`export rejects ${name} before destination exists`, async t => { const f = await fixture(t); await mutate(f); await assert.rejects(exportPixels(f)); await assert.rejects(lstat(f.destination), { code: 'ENOENT' }); });
