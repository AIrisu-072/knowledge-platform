// Read a fixed eight-file source packet once; expose only six runtime assets.
import assert from 'node:assert/strict';
import http from 'node:http';
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';
import { join, parse, resolve, sep } from 'node:path';
export const SOURCE_FILES = Object.freeze(['sales.html', 'office.html', 'prototype.css', 'prototype.js', 'scenarios.json', 'scenarios.js', 'source-design.test.mjs', 'interaction.test.mjs']);
export const SERVED_FILES = Object.freeze(SOURCE_FILES.slice(0, 6));
export const STATES = Object.freeze(['normal', 'newly_assigned', 'returned', 'working_draft', 'handed_off', 'due_soon', 'blocked', 'agent_active', 'evidence_review', 'document_compare']);
export const MODULES = Object.freeze(['evidence', 'history', 'return', 'resources', 'history', 'evidence', 'document', 'agent', 'evidence', 'document']);
export async function regularDirectory(path, privateOwned = false) {
  const absolute = resolve(path), root = parse(absolute).root;
  let current = root;
  for (const part of absolute.slice(root.length).split(sep).filter(Boolean)) {
    current = join(current, part); const s = await lstat(current);
    assert.ok(s.isDirectory() && !s.isSymbolicLink(), 'directory');
  }
  if (privateOwned) { const s = await lstat(absolute); assert.ok((s.mode & 0o077) === 0 && s.uid === process.getuid(), 'directory'); }
  return absolute;
}
export async function regularBytes(directory, name, maxBytes, privateOwned = false) {
  const file = join(directory, name), before = await lstat(file);
  assert.ok(before.isFile() && !before.isSymbolicLink() && before.nlink === 1 && before.size > 0 && before.size <= maxBytes, 'file');
  if (privateOwned) assert.ok(before.uid === process.getuid() && (before.mode & 0o177) === 0, 'file');
  const handle = await open(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const current = await handle.stat();
    assert.ok(current.isFile() && current.ino === before.ino && current.dev === before.dev && current.size === before.size && current.nlink === 1, 'file');
    const bytes = Buffer.alloc(current.size); let offset = 0;
    while (offset < bytes.length) { const read = await handle.read(bytes, offset, bytes.length - offset, offset); assert.ok(read.bytesRead > 0, 'file'); offset += read.bytesRead; }
    const after = await handle.stat(); assert.ok(after.size === before.size && after.mtimeMs === before.mtimeMs && after.ctimeMs === before.ctimeMs, 'file');
    return bytes;
  } finally { await handle.close(); }
}
export async function snapshotSource(root) {
  const directory = await regularDirectory(root), files = new Map(), identity = [];
  for (const name of SOURCE_FILES) {
    const bytes = await regularBytes(directory, name, 128 * 1024);
    files.set(name, bytes);
    identity.push({ name, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), blob: createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex') });
  }
  return { files, identity };
}
export async function startSourceServer(snapshot) {
  const types = { html: 'text/html', js: 'text/javascript', css: 'text/css', json: 'application/json' };
  const server = http.createServer((request, response) => {
    const status = request.headers.host !== `127.0.0.1:${server.address().port}` ? 421 : !['GET', 'HEAD'].includes(request.method) ? 405 : 0;
    const raw = request.url ?? '', path = raw.split('?')[0], name = path.slice(1);
    if (status || !SERVED_FILES.includes(name) || path !== `/${name}` || raw.includes('#')) { response.writeHead(status || 404); response.end(); return; }
    const bytes = snapshot.files.get(name);
    response.writeHead(200, {
      'Content-Type': `${types[name.split('.').at(-1)]}; charset=utf-8`, 'Content-Length': bytes.length,
      'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'Referrer-Policy': 'no-referrer',
      'Content-Security-Policy': "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'none'; font-src 'none'; img-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    });
    response.end(request.method === 'HEAD' ? undefined : bytes);
  });
  server.requestTimeout = 5000; server.headersTimeout = 5000;
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((resolve, reject) => { server.close(error => error ? reject(error) : resolve()); server.closeAllConnections(); }) };
}
