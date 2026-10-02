// D2-only adaptation of the qualified strict PNG/atomic export boundary.
import assert from 'node:assert/strict';
import { lstat, readdir, mkdtemp, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { crc32, inflateSync } from 'node:zlib';
import { STATES, regularDirectory, regularBytes } from './source-server.mjs';
export const MAX_PNG_BYTES = 8 * 1024 * 1024;
export const PNG_NAMES = Object.freeze(['sales', 'office'].flatMap(page => STATES.map(state => `${page}-${state}.png`)));
export function validatePng(bytes, name) {
  assert.ok(PNG_NAMES.includes(name), 'PNG name invalid');
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
      assert.ok(header.width === 1440 && header.height >= 900 && header.height <= 2400, 'PNG dimensions invalid');
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

export async function exportPixels({ capture, destination, qualified, sourceUnchanged }) {
  assert.ok(qualified === true && sourceUnchanged === true, 'qualification');
  const directory = await regularDirectory(capture, true), parent = await regularDirectory(dirname(resolve(destination)), true);
  assert.equal(resolve(destination), join(parent, 'visual-export'), 'destination');
  try { await lstat(destination); throw Error('destination exists'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  assert.deepEqual((await readdir(directory)).sort(), [...PNG_NAMES].sort(), 'exact twenty PNGs');
  const files = [];
  for (const name of PNG_NAMES) { const bytes = await regularBytes(directory, name, MAX_PNG_BYTES, true); validatePng(bytes, name); files.push({ name, bytes }); }
  const staging = await mkdtemp(join(parent, '.visual-export-'));
  try {
    for (const { name, bytes } of files) await writeFile(join(staging, name), bytes, { flag: 'wx', mode: 0o600 });
    await rename(staging, destination);
  } catch (error) { await rm(staging, { recursive: true, force: true }); throw error; }
  return { directory: destination, count: files.length };
}
