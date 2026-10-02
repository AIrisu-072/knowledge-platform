// Fixed synthetic screenshot boundary. This module never uploads or reads reports/logs.
import assert from 'node:assert/strict';
import { constants } from 'node:fs';
import { lstat, open, readdir, mkdtemp, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join, parse, resolve, sep } from 'node:path';
import { crc32, inflateSync } from 'node:zlib';

// A disposable acknowledgement is not proof that an external DB is synthetic-only.
// Visual mode permits only the fresh database created by this harness invocation.
export function assertOwnedVisualDatabaseInput(env) {
  assert.ok(env.TEST_DATABASE_URL === undefined, 'Visual capture requires a harness-owned disposable database; external database input is forbidden');
}

export const MAX_PNG_BYTES = 8 * 1024 * 1024;
export const VISUAL_CHECKPOINTS = Object.freeze([
  '01-list-context-1440.png', '02-list-focus-return-1280.png', '03-detail-overview-1440.png',
  '04-revision-version-1440.png', '05-comparison-1440.png', '06-version-file-selected-1440.png',
  '07-publication-ready-1440.png', '08-publication-confirm-focus-1440.png', '09-publication-success-1440.png',
  '10-access-policy-effective-draft-1440.png', '11-occ-conflict-1440.png',
  '12-permission-denied-file-retained-1440.png', '13-permission-restored-retry-success-1440.png',
].map(name => Object.freeze({ name, width: name.includes('1280') ? 1280 : 1440, height: 900 })));
function checkpoint(name) {
  const item = VISUAL_CHECKPOINTS.find(item => item.name === name);
  assert.ok(item, 'Unknown visual checkpoint'); return item;
}

// Native zlib does bounded decompression/CRC. Only the simple 8-bit non-interlaced
// RGB/RGBA PNG form emitted by Chromium is admitted; ancillary metadata is refused.
export function validatePng(bytes, name) {
  const expected = checkpoint(name);
  assert.ok(Buffer.isBuffer(bytes) && bytes.length >= 57 && bytes.length <= MAX_PNG_BYTES, 'PNG size invalid');
  assert.ok(bytes.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex')), 'PNG signature invalid');
  let offset = 8, header, ended = false;
  const compressed = [];
  for (let count = 0; offset < bytes.length; count++) {
    assert.ok(count < 4096 && offset + 12 <= bytes.length, 'PNG chunk bounds invalid');
    const size = bytes.readUInt32BE(offset), end = offset + 12 + size;
    assert.ok(end <= bytes.length, 'PNG chunk bounds invalid');
    const type = bytes.toString('latin1', offset + 4, offset + 8), data = bytes.subarray(offset + 8, end - 4);
    assert.equal(crc32(bytes.subarray(offset + 4, end - 4)), bytes.readUInt32BE(end - 4), 'PNG CRC invalid');
    if (type === 'IHDR') {
      assert.ok(!header && count === 0 && size === 13, 'PNG header invalid');
      header = { width: data.readUInt32BE(0), height: data.readUInt32BE(4), channels: data[9] === 2 ? 3 : 4 };
      assert.ok(header.width === expected.width && header.height === expected.height, 'PNG dimensions invalid');
      assert.ok(data[8] === 8 && [2, 6].includes(data[9]) && data[10] === 0 && data[11] === 0 && data[12] === 0, 'PNG encoding invalid');
    } else if (type === 'IDAT') {
      assert.ok(header && !ended && size > 0, 'PNG image data invalid'); compressed.push(data);
    } else if (type === 'IEND') {
      assert.ok(header && compressed.length > 0 && size === 0 && end === bytes.length, 'PNG ending invalid'); ended = true;
    } else throw Error('PNG non-pixel chunk rejected');
    offset = end;
  }
  assert.ok(header && ended, 'PNG incomplete');
  const rowSize = header.width * header.channels + 1, expectedBytes = rowSize * header.height;
  const input = Buffer.concat(compressed);
  let decoded;
  try { decoded = inflateSync(input, { maxOutputLength: expectedBytes, info: true }); }
  catch { throw Error('PNG compressed pixels invalid'); }
  assert.equal(decoded.buffer.length, expectedBytes, 'PNG decoded size invalid');
  assert.equal(decoded.engine.bytesWritten, input.length, 'PNG trailing compressed data rejected');
  for (let row = 0; row < header.height; row++) assert.ok(decoded.buffer[row * rowSize] <= 4, 'PNG scanline filter invalid');
  return { width: header.width, height: header.height };
}

async function privateDirectory(path) {
  const absolute = resolve(path), root = parse(absolute).root;
  let current = root;
  // Reject symlinks in every parent component as well as the final directory.
  for (const component of absolute.slice(root.length).split(sep).filter(Boolean)) {
    current = join(current, component);
    const stat = await lstat(current); assert.ok(stat.isDirectory() && !stat.isSymbolicLink(), 'Visual directory must be regular');
  }
  const stat = await lstat(absolute);
  assert.equal(stat.mode & 0o077, 0, 'Visual directory must be private');
  assert.equal(stat.uid, process.getuid(), 'Visual directory must be owned');
  return absolute;
}
function loopback(origin) {
  const url = new URL(origin);
  assert.ok(url.protocol === 'http:' && url.hostname === '127.0.0.1' && url.port && !url.username && !url.password && url.pathname === '/' && !url.search && !url.hash, 'Visual origin must be loopback');
}
async function captureDirectory(runDirectory, context) {
  const run = await privateDirectory(runDirectory);
  assert.match(context.runId, /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/);
  loopback(context.human); loopback(context.agent); assert.notEqual(context.human, context.agent);
  const capture = context.visualCapture;
  assert.ok(capture?.ownership === 'synthetic-owned-runtime' && capture.database === 'harness-owned-disposable-loopback', 'Owned synthetic capture context with harness-owned database required');
  assert.equal(capture.directory, join(run, 'visual-checkpoints'), 'Fixed owned checkpoint directory required');
  return privateDirectory(capture.directory);
}
async function readPng(directory, name) {
  const path = join(directory, name), before = await lstat(path);
  assert.ok(before.isFile() && !before.isSymbolicLink() && before.nlink === 1 && (before.mode & 0o177) === 0 && before.uid === process.getuid(), 'PNG must be a private non-executable unlinked regular file');
  assert.ok(before.size >= 57 && before.size <= MAX_PNG_BYTES, 'PNG size invalid');
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const current = await handle.stat();
    assert.ok(current.isFile() && current.ino === before.ino && current.dev === before.dev && current.size === before.size && current.nlink === 1, 'PNG file changed');
    const bytes = Buffer.alloc(current.size);
    let offset = 0;
    while (offset < bytes.length) {
      const { bytesRead } = await handle.read(bytes, offset, bytes.length - offset, offset);
      assert.ok(bytesRead > 0, 'PNG truncated'); offset += bytesRead;
    }
    const after = await handle.stat(); assert.ok(after.size === before.size && after.mtimeMs === before.mtimeMs && after.ctimeMs === before.ctimeMs, 'PNG file changed');
    validatePng(bytes, name); return bytes;
  } finally { await handle.close(); }
}

export async function captureVisualCheckpoint({ runDirectory, context, phase, page, name }) {
  assert.equal(phase, 'journey', 'Visual capture requires successful journey checkpoints');
  const expected = checkpoint(name), directory = await captureDirectory(runDirectory, context);
  assert.equal(new URL(page.url()).origin, context.human, 'Visual page must belong to owned Human runtime');
  assert.deepEqual(page.viewportSize(), { width: expected.width, height: expected.height }, 'Visual viewport must be explicit');
  const bytes = await page.screenshot({ type: 'png', fullPage: false, scale: 'css' });
  validatePng(bytes, name);
  await writeFile(join(directory, name), bytes, { flag: 'wx', mode: 0o600 });
}

export async function exportVisualEvidence({ runDirectory, report, context, currentSource }) {
  assert.ok(report.acceptanceQualified === true && report.status === 'passed' && report.gitDirty === false && report.buildMode === 'built-in-this-run', 'Successful clean built runtime required');
  assert.ok(currentSource?.gitDirty === false && currentSource.gitHead === report.gitHead, 'Final source must remain clean at the exact starting head');
  assert.match(report.gitHead, /^[a-f0-9]{40}$/); assert.equal(report.runId, context.runId, 'Visual run identity mismatch');
  assert.equal(report.database?.ownership, 'harness-owned', 'Visual export requires a harness-owned disposable database');
  const directory = await captureDirectory(runDirectory, context), parent = await privateDirectory(dirname(resolve(runDirectory)));
  const destination = join(parent, 'visual-export');
  try { await lstat(destination); throw Error('Visual export already exists; never reuse stale evidence'); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  assert.deepEqual((await readdir(directory)).sort(), VISUAL_CHECKPOINTS.map(item => item.name).sort(), 'Exactly 13 checkpoints required');
  // Read and validate everything before any export exists. Copy only validated bytes,
  // not paths, so a replaced input cannot redirect what the export contains.
  const files = [];
  for (const { name } of VISUAL_CHECKPOINTS) files.push({ name, bytes: await readPng(directory, name) });
  const staging = await mkdtemp(join(parent, '.visual-export-'));
  try {
    for (const { name, bytes } of files) await writeFile(join(staging, name), bytes, { flag: 'wx', mode: 0o600 });
    await rename(staging, destination);
  } catch (error) { await rm(staging, { recursive: true, force: true }); throw error; }
  return { directory: destination, count: files.length, review: 'NOT RUN' };
}
