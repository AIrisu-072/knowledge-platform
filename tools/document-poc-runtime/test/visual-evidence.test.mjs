import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, readdir, lstat, rm, symlink, chmod, link } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import * as api from '../visual-evidence.mjs';
const exportEvidence = input => api.exportVisualEvidence({ currentSource: { gitHead: input.report.gitHead, gitDirty: false }, ...input });
const names = [
  '01-list-context-1440.png', '02-list-focus-return-1280.png', '03-detail-overview-1440.png',
  '04-revision-version-1440.png', '05-comparison-1440.png', '06-version-file-selected-1440.png',
  '07-publication-ready-1440.png', '08-publication-confirm-focus-1440.png', '09-publication-success-1440.png',
  '10-access-policy-effective-draft-1440.png', '11-occ-conflict-1440.png',
  '12-permission-denied-file-retained-1440.png', '13-permission-restored-retry-success-1440.png',
];
function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type), data]);
  const header = Buffer.alloc(4); header.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body));
  return Buffer.concat([header, body, crc]);
}
// Generated pixels are parser TEST FIXTURES ONLY, never browser acceptance evidence.
function png(width = 1440, height = 900, options = {}) {
  const header = Buffer.alloc(13); header.writeUInt32BE(width); header.writeUInt32BE(height, 4);
  header[8] = 8; header[9] = 2;
  const raw = Buffer.alloc((width * 3 + 1) * height); if (options.invalidFilter) raw[0] = 5;
  const compressed = deflateSync(raw);
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', header),
    ...(options.metadata ? [chunk('tEXt', Buffer.from('sensitive'))] : []),
    chunk('IDAT', options.badDeflate ? Buffer.from('invalid') : compressed), chunk('IEND', Buffer.alloc(0))]);
}
async function fixture(fn) {
  const root = await mkdtemp(join(tmpdir(), 'visual-evidence-unit-'));
  const run = join(root, 'run-test'); await mkdir(run, { mode: 0o700 });
  const capture = join(run, 'visual-checkpoints'); await mkdir(capture, { mode: 0o700 });
  const report = { runId: '12345678-1234-4234-8234-123456789abc', gitHead: 'a'.repeat(40), gitDirty: false,
    acceptanceQualified: true, status: 'passed', buildMode: 'built-in-this-run', database: { ownership: 'harness-owned' } };
  const context = { runId: report.runId, human: 'http://127.0.0.1:1234', agent: 'http://127.0.0.1:1235',
    visualCapture: { directory: capture, ownership: 'synthetic-owned-runtime', database: 'harness-owned-disposable-loopback' } };
  for (const name of names) await writeFile(join(capture, name), png(name.includes('1280') ? 1280 : 1440), { mode: 0o600 });
  try { await fn({ root, run, capture, report, context }); } finally { await rm(root, { recursive: true, force: true }); }
}
test('visual API exposes exactly the approved 13 checkpoint basenames and dimensions', () => {
  assert.equal(typeof api.validatePng, 'function');
  assert.deepEqual(api.VISUAL_CHECKPOINTS.map(item => item.name), names);
  for (const item of api.VISUAL_CHECKPOINTS) assert.deepEqual([item.width, item.height], [item.name.includes('1280') ? 1280 : 1440, 900]);
});
test('native PNG validation accepts bounded truecolor pixels and exact CSS dimensions', () => {
  assert.deepEqual(api.validatePng(png(), names[0]), { width: 1440, height: 900 });
  assert.deepEqual(api.validatePng(png(1280), names[1]), { width: 1280, height: 900 });
});
test('PNG validator rejects malformed, truncated, corrupt CRC, trailing or metadata data', () => {
  const valid = png(), corrupt = Buffer.from(valid); corrupt[40] ^= 1;
  for (const input of [Buffer.from('not png'), valid.subarray(0, -1), corrupt, Buffer.concat([valid, Buffer.from('SECRET')]), png(1440, 900, { metadata: true }), png(1440, 900, { badDeflate: true }), png(1440, 900, { invalidFilter: true })]) {
    assert.throws(() => api.validatePng(input, names[0]), /PNG/);
  }
});
test('PNG validator rejects wrong viewport, unknown basename and oversized bytes', () => {
  assert.throws(() => api.validatePng(png(1280), names[0]), /dimensions/);
  assert.throws(() => api.validatePng(png(), '../01-list-context-1440.png'), /checkpoint/);
  assert.throws(() => api.validatePng(Buffer.alloc(api.MAX_PNG_BYTES + 1), names[0]), /size/);
});
test('export is exactly 13 newly created regular private PNGs, never reports or source directories', () => fixture(async ({ root, run, report, context }) => {
  await writeFile(join(run, 'failure.png'), 'not evidence'); await writeFile(join(run, 'runtime-context.json'), 'private context');
  const result = await exportEvidence({ runDirectory: run, report, context });
  assert.equal(result.directory, join(root, 'visual-export'));
  assert.deepEqual((await readdir(result.directory)).sort(), [...names].sort());
  assert.equal((await lstat(result.directory)).mode & 0o777, 0o700);
  for (const name of names) { const stat = await lstat(join(result.directory, name)); assert.ok(stat.isFile()); assert.equal(stat.mode & 0o777, 0o600); }
}));
test('export requires a successful clean-head built owned runtime with matching run identity', async () => {
  for (const change of [{ acceptanceQualified: false }, { status: 'failed' }, { gitDirty: true }, { gitHead: 'bad' }, { buildMode: 'prebuilt-unverified-source-correspondence' }, { database: { ownership: 'unknown' } }, { runId: 'different' }]) {
    await fixture(async ({ root, run, report, context }) => {
      await assert.rejects(exportEvidence({ runDirectory: run, report: { ...report, ...change }, context }));
      await assert.rejects(lstat(join(root, 'visual-export')), { code: 'ENOENT' });
    });
  }
});
test('export rejects extra/missing PNGs or any unexpected directory without creating export', async () => {
  for (const mutation of [c => writeFile(join(c, 'failure.png'), png()), c => rm(join(c, names[12])), c => mkdir(join(c, 'logs'))]) await fixture(async ({ root, run, capture, report, context }) => {
    await mutation(capture); await assert.rejects(exportEvidence({ runDirectory: run, report, context }));
    await assert.rejects(lstat(join(root, 'visual-export')), { code: 'ENOENT' });
  });
});
test('export rejects symlink files, hardlinks, executable/private-mode violations and symlink directory ancestry', async () => {
  for (const mutate of [
    async c => { await rm(join(c, names[0])); await symlink(names[1], join(c, names[0])); },
    async c => { await rm(join(c, names[0])); await link(join(c, names[2]), join(c, names[0])); },
    c => chmod(join(c, names[0]), 0o700), c => chmod(join(c, names[0]), 0o644),
  ]) await fixture(async ({ run, capture, report, context }) => { await mutate(capture); await assert.rejects(exportEvidence({ runDirectory: run, report, context })); });
  await fixture(async ({ root, run, report, context }) => {
    const alias = join(root, 'alias'); await symlink(run, alias);
    await assert.rejects(exportEvidence({ runDirectory: alias, report, context }));
  });
});
test('export preserves an existing destination and rejects malformed last image atomically', () => fixture(async ({ root, run, capture, report, context }) => {
  await writeFile(join(capture, names[12]), 'invalid');
  await assert.rejects(exportEvidence({ runDirectory: run, report, context }));
  await assert.rejects(lstat(join(root, 'visual-export')), { code: 'ENOENT' });
  await mkdir(join(root, 'visual-export')); await writeFile(join(root, 'visual-export', 'keep'), 'existing');
  await assert.rejects(exportEvidence({ runDirectory: run, report, context }));
  assert.equal(await readFile(join(root, 'visual-export', 'keep'), 'utf8'), 'existing');
}));
test('capture refuses unowned, remote, persistence and wrong-viewport pages before collecting pixels', () => fixture(async ({ run, context }) => {
  const page = { url: () => context.human + '/documents', viewportSize: () => ({ width: 1440, height: 900 }), screenshot: () => { throw Error('pixels must not be requested'); } };
  for (const overrides of [{ phase: 'persistence' }, { context: { ...context, visualCapture: undefined } }, { context: { ...context, human: 'https://example.com' } }, { page: { ...page, url: () => 'http://127.0.0.1:9999/documents' } }, { page: { ...page, viewportSize: () => ({ width: 1280, height: 720 }) } }]) {
    await assert.rejects(api.captureVisualCheckpoint({ runDirectory: run, context, phase: 'journey', page, name: names[0], ...overrides }), error => !error.message.includes('pixels must'));
  }
}));
test('capture writes actual supplied screenshot bytes exclusively with fixed viewport options', () => fixture(async ({ run, capture, context }) => {
  await rm(join(capture, names[0]));
  let options;
  const bytes = png();
  const page = { url: () => context.human + '/documents', viewportSize: () => ({ width: 1440, height: 900 }), screenshot: async input => { options = input; return bytes; } };
  await api.captureVisualCheckpoint({ runDirectory: run, context, phase: 'journey', page, name: names[0] });
  assert.deepEqual(options, { type: 'png', fullPage: false, scale: 'css' });
  assert.deepEqual(await readFile(join(capture, names[0])), bytes);
  await assert.rejects(api.captureVisualCheckpoint({ runDirectory: run, context, phase: 'journey', page, name: names[0] }), { code: 'EEXIST' });
}));
test('PNG rejects non-ASCII chunk identifiers even when low seven bits resemble allowed chunks', () => {
  const valid = png();
  valid[12] |= 0x80; valid.writeUInt32BE(crc32(valid.subarray(12, 29)), 29);
  assert.throws(() => api.validatePng(valid, names[0]), /PNG/);
});
test('PNG accepts many ordinary Chromium IDAT chunks within its byte bound', () => {
  const input = png(), size = input.readUInt32BE(33), data = input.subarray(41, 41 + size), chunks = [];
  for (let offset = 0; offset < data.length; offset += 16) chunks.push(chunk('IDAT', data.subarray(offset, offset + 16)));
  const many = Buffer.concat([input.subarray(0, 33), ...chunks, chunk('IEND', Buffer.alloc(0))]);
  assert.ok(chunks.length > 128);
  assert.deepEqual(api.validatePng(many, names[0]), { width: 1440, height: 900 });
});
test('export refuses a changed HEAD or dirty/missing final source observation', async () => {
  for (const currentSource of [undefined, { gitHead: 'b'.repeat(40), gitDirty: false }, { gitHead: 'a'.repeat(40), gitDirty: true }]) {
    await fixture(async ({ root, run, report, context }) => {
      await assert.rejects(exportEvidence({ runDirectory: run, report, context, currentSource }));
      await assert.rejects(lstat(join(root, 'visual-export')), { code: 'ENOENT' });
    });
  }
});
test('visual capture rejects every caller database input before harness setup', () => {
  assert.equal(typeof api.assertOwnedVisualDatabaseInput, 'function');
  assert.doesNotThrow(() => api.assertOwnedVisualDatabaseInput({}));
  for (const TEST_DATABASE_URL of ['', 'postgres://localhost/synthetic', 'postgres://127.0.0.1:5432/acknowledged']) {
    assert.throws(() => api.assertOwnedVisualDatabaseInput({ TEST_DATABASE_URL, KP_POC_DISPOSABLE_DATABASE: 'true' }), /harness-owned/);
  }
});
test('visual export independently refuses caller-asserted disposable database evidence', () => fixture(async ({ root, run, report, context }) => {
  report.database.ownership = 'caller-asserted-disposable';
  await assert.rejects(exportEvidence({ runDirectory: run, report, context }), /harness-owned/);
  await assert.rejects(lstat(join(root, 'visual-export')), { code: 'ENOENT' });
}));
test('visual capture refuses the former caller-compatible database proof before requesting pixels', () => fixture(async ({ run, capture, context }) => {
  await rm(join(capture, names[0]));
  const oldContext = { ...context, visualCapture: { ...context.visualCapture, database: 'disposable-loopback' } };
  let requested = false;
  const page = { url: () => context.human + '/documents', viewportSize: () => ({ width: 1440, height: 900 }), screenshot: async () => { requested = true; return png(); } };
  await assert.rejects(api.captureVisualCheckpoint({ runDirectory: run, context: oldContext, phase: 'journey', page, name: names[0] }), /harness-owned/);
  assert.equal(requested, false);
}));
test('early external-database rejection never echoes connection strings or credentials', () => {
  const TEST_DATABASE_URL = 'postgres://synthetic-user:synthetic-secret@127.0.0.1:5432/private-name';
  assert.throws(() => api.assertOwnedVisualDatabaseInput({ TEST_DATABASE_URL }), error => {
    assert.ok(!JSON.stringify(error).includes(TEST_DATABASE_URL));
    assert.ok(!String(error).includes('synthetic-secret'));
    return true;
  });
});
