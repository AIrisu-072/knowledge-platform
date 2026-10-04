import { constants } from 'node:fs';
import { lstat, open, opendir, realpath } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, extname, join, parse, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Development/test preview only. The production Rust server owns real APIs.
// No command-line or environment override can change the bind address or dist.
export const HOST = '127.0.0.1';
export const PORT = 8080;
export const DIST_ROOT = fileURLToPath(new URL('../dist', import.meta.url));
export const LIMITS = Object.freeze({
  maxFileBytes: 16 * 1024 * 1024,
  maxTotalBytes: 64 * 1024 * 1024,
  maxFiles: 512,
  maxEntries: 1024,
  maxDepth: 8,
  maxTargetBytes: 4096,
  maxHeaderBytes: 8192,
  maxHeaders: 64,
  maxConnections: 32,
  headersTimeoutMs: 5000,
  requestTimeoutMs: 10000,
  socketTimeoutMs: 5000,
  keepAliveTimeoutMs: 1000,
  startupTimeoutMs: 5000,
  shutdownTimeoutMs: 2000,
});
const SECURITY_HEADERS = Object.freeze({
  'content-security-policy': "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'; base-uri 'self'",
  'x-content-type-options': 'nosniff',
  'referrer-policy': 'no-referrer',
  'cache-control': 'no-store',
  connection: 'close',
});
const MIME_TYPES = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.txt', 'text/plain; charset=utf-8'],
  ['.svg', 'image/svg+xml'], ['.png', 'image/png'], ['.jpg', 'image/jpeg'],
  ['.jpeg', 'image/jpeg'], ['.gif', 'image/gif'], ['.webp', 'image/webp'],
  ['.avif', 'image/avif'], ['.ico', 'image/x-icon'],
  ['.woff', 'font/woff'], ['.woff2', 'font/woff2'], ['.ttf', 'font/ttf'],
  ['.otf', 'font/otf'], ['.wasm', 'application/wasm'],
]);
const snapshots = new WeakMap();
const unsafeCharacters = /[\u0000-\u0020\u007f-\u009f\\]/u;
const unsafeDecodedCharacters = /[\u0000-\u001f\u007f-\u009f\\]/u;
const reserved = (parts) => parts[0] === 'v1' || parts[0] === 'health';
const excluded = (parts) => parts.some((part) => part.startsWith('.')) || parts.at(-1).toLowerCase().endsWith('.map');

function sameEntry(a, b) {
  return ['dev', 'ino', 'mode', 'nlink', 'size', 'mtimeMs', 'ctimeMs'].every((key) => a[key] === b[key]);
}

async function checkedEntry(path) {
  const entry = await lstat(path);
  if (entry.isSymbolicLink()) throw new Error('Preview dist contains a symbolic link');
  if (!entry.isDirectory() && !entry.isFile()) throw new Error('Preview dist requires regular files and directories');
  if (entry.isFile() && entry.nlink !== 1) throw new Error('Preview dist contains a hardlink');
  return entry;
}

async function checkedAncestors(root) {
  const ancestors = new Map();
  for (let path = root; ; path = dirname(path)) {
    const entry = await checkedEntry(path);
    if (!entry.isDirectory()) throw new Error('Preview dist ancestor is not a directory');
    ancestors.set(path, entry);
    if (path === parse(path).root) return ancestors;
  }
}

async function boundedRead(path, expected) {
  // O_NOFOLLOW protects the last component; inode identity plus the canonical
  // ancestor and post-read checks also reject replacement while snapshotting.
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const before = await file.stat();
    if (!before.isFile() || before.nlink !== 1 || !sameEntry(before, expected)) {
      throw new Error('Preview asset changed during snapshot');
    }
    const bytes = Buffer.alloc(expected.size);
    let offset = 0;
    while (offset < bytes.length) {
      const { bytesRead } = await file.read(bytes, offset, bytes.length - offset, offset);
      if (bytesRead === 0) throw new Error('Preview asset changed during snapshot');
      offset += bytesRead;
    }
    const extra = await file.read(Buffer.alloc(1), 0, 1, offset);
    if (extra.bytesRead !== 0 || !sameEntry(expected, await file.stat())) {
      throw new Error('Preview asset changed during snapshot');
    }
    return bytes;
  } finally {
    await file.close();
  }
}

// The explicit root/limit arguments are filesystem-unit-test seams. The only
// executable entrypoint below always loads DIST_ROOT with the fixed bounds.
export async function loadSnapshot(root = DIST_ROOT, overrides = {}) {
  const configurable = ['maxFileBytes', 'maxTotalBytes', 'maxFiles', 'maxEntries', 'maxDepth'];
  for (const [key, value] of Object.entries(overrides)) {
    if (!configurable.includes(key) || !Number.isSafeInteger(value) || value < 1 || value > LIMITS[key]) {
      throw new Error('Invalid preview snapshot limit');
    }
  }
  const limits = { ...LIMITS, ...overrides };
  root = resolve(root);
  const ancestors = await checkedAncestors(root);
  if (await realpath(root) !== root) throw new Error('Preview dist has a symbolic ancestor');
  const observed = new Map();
  const assets = new Map();
  let entries = 0, files = 0, totalBytes = 0;

  async function walk(directory, parts) {
    const directoryEntry = await checkedEntry(directory);
    observed.set(directory, directoryEntry);
    const listing = await opendir(directory);
    for await (const child of listing) {
      if (++entries > limits.maxEntries) throw new Error('Preview entry count limit exceeded');
      if (parts.length + 1 > limits.maxDepth) throw new Error('Preview depth limit exceeded');
      if (!/^[A-Za-z0-9._-]+$/.test(child.name) || child.name === '.' || child.name === '..') {
        throw new Error('Unsafe preview asset name');
      }
      const childParts = [...parts, child.name];
      const path = join(directory, child.name);
      const entry = await checkedEntry(path);
      if (await realpath(path) !== path) throw new Error('Preview asset has a symbolic ancestor');
      observed.set(path, entry);
      if (entry.isDirectory()) {
        await walk(path, childParts);
        continue;
      }
      if (++files > limits.maxFiles) throw new Error('Preview file count limit exceeded');
      // Production source maps remain on disk, but never enter the served view.
      // Even excluded entries must be regular and singly linked.
      if (excluded(childParts) || reserved(childParts)) continue;
      const type = MIME_TYPES.get(extname(child.name).toLowerCase());
      if (!type) throw new Error('Unsupported preview asset extension');
      if (entry.size > limits.maxFileBytes) throw new Error('Preview file byte limit exceeded');
      if (totalBytes + entry.size > limits.maxTotalBytes) throw new Error('Preview total byte limit exceeded');
      totalBytes += entry.size;
      assets.set(`/${childParts.join('/')}`, { type, bytes: await boundedRead(path, entry) });
    }
  }
  await walk(root, []);
  for (const [path, entry] of observed) {
    if (!sameEntry(entry, await checkedEntry(path)) || await realpath(path) !== path) {
      throw new Error('Preview dist changed during snapshot; rebuild and restart');
    }
  }
  // Parent directory content can change independently (e.g. another /tmp test);
  // identity, type and canonical path must remain fixed, not parent timestamps.
  for (const [path, entry] of ancestors) {
    const current = await checkedEntry(path);
    if (current.dev !== entry.dev || current.ino !== entry.ino || !current.isDirectory()) {
      throw new Error('Preview dist ancestor changed during snapshot');
    }
  }
  const index = assets.get('/index.html');
  if (!index || !index.bytes.length) throw new Error('Built index.html is missing; run the production build first');
  const html = index.bytes.toString('utf8');
  // The unchanged build emits script/link/img tags. Scan bounded tag spans
  // without evaluating HTML or introducing an HTML-parser dependency.
  const references = [];
  let scriptCount = 0;
  for (const [tag] of html.matchAll(/<[^<>]*>/g)) {
    const element = /^<(script|link|img)\b/i.exec(tag)?.[1].toLowerCase();
    if (!element) continue;
    const attribute = element === 'link' ? 'href' : 'src';
    for (const match of tag.matchAll(/\s(src|href)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'<>`=]+))/gi)) {
      if (match[1].toLowerCase() !== attribute) continue;
      const reference = match[2] ?? match[3] ?? match[4];
      references.push(reference);
      if (element === 'script') {
        if (!/^\/assets\/[A-Za-z0-9._-]+\.js$/.test(reference)) {
          throw new Error('index.html requires production script assets');
        }
        scriptCount++;
      }
    }
  }
  if (!scriptCount || references.some((reference) => !assets.has(reference))) {
    throw new Error('index.html requires existing production asset references; build first');
  }
  const snapshot = Object.freeze({ fileCount: assets.size, totalBytes });
  snapshots.set(snapshot, assets);
  return snapshot;
}

function parseTarget(target) {
  if (typeof target !== 'string' || !target.startsWith('/') || target.startsWith('//') || target.includes('#') || unsafeCharacters.test(target)) return null;
  const question = target.indexOf('?');
  const encodedPath = question < 0 ? target : target.slice(0, question);
  if (/%(?:2f|5c)/i.test(encodedPath)) return null;
  // Query data is opaque to this asset server. Only the pathname participates
  // in filesystem lookup and needs decoding/traversal checks; the application
  // owns search-value validation. Raw framing/length checks still cover target.
  let path;
  try {
    path = decodeURIComponent(encodedPath);
  } catch { return null; }
  if (unsafeDecodedCharacters.test(path) || path.includes('%')) return null;
  const parts = path.slice(1).split('/');
  if (parts.at(-1) === '') parts.pop();
  if (parts.some((part) => !part || part === '.' || part === '..' || part.includes('?') || part.includes('#'))) return null;
  return { path, parts };
}

function acceptsHTML(value) {
  if (typeof value !== 'string') return false;
  return value.split(',').some((range) => {
    const [type, ...parameters] = range.trim().toLowerCase().split(';').map((part) => part.trim());
    if (type !== 'text/html') return false;
    let quality = 1;
    let seenQuality = false;
    for (const parameter of parameters) {
      if (!parameter.startsWith('q=')) continue;
      if (seenQuality || !/^q=(?:0(?:\.\d{0,3})?|1(?:\.0{0,3})?)$/.test(parameter)) return false;
      quality = Number(parameter.slice(2));
      seenQuality = true;
    }
    return quality > 0;
  });
}

export function createPreviewHandler(snapshot) {
  const assets = snapshots.get(snapshot);
  if (!assets) throw new Error('Expected a validated preview snapshot');
  return (req, res) => {
    function send(status, bytes = Buffer.from(`${status}\n`), type = 'text/plain; charset=utf-8', extra = {}) {
      res.writeHead(status, { ...SECURITY_HEADERS, 'content-type': type, 'content-length': String(bytes.length), ...extra });
      // Copy outgoing bytes, retaining the private immutable snapshot generation.
      res.end(req.method === 'HEAD' ? undefined : Buffer.from(bytes));
    }
    const headers = req.headers ?? {};
    const raw = req.rawHeaders ?? [];
    const hosts = raw.filter((_, index) => index % 2 === 0 && typeof raw[index] === 'string' && raw[index].toLowerCase() === 'host');
    if (headers.host !== `${HOST}:${PORT}` || hosts.length !== 1) return send(421);
    // Node can truncate the parsed header list at maxHeadersCount. Reject at
    // that ceiling rather than trusting a possibly incomplete Host inventory.
    if (raw.length >= LIMITS.maxHeaders * 2 || raw.reduce((bytes, value) => bytes + Buffer.byteLength(String(value)) + 2, 0) > LIMITS.maxHeaderBytes) return send(431);
    if (req.method !== 'GET' && req.method !== 'HEAD') return send(405, undefined, undefined, { allow: 'GET, HEAD' });
    if (headers.upgrade !== undefined || /(?:^|,)\s*upgrade\s*(?:,|$)/i.test(headers.connection ?? '') ||
        headers['transfer-encoding'] !== undefined || headers.expect !== undefined ||
        (headers['content-length'] !== undefined && headers['content-length'] !== '0')) return send(400);
    if (typeof req.url === 'string' && Buffer.byteLength(req.url) > LIMITS.maxTargetBytes) return send(414);
    const target = parseTarget(req.url);
    if (!target) return send(400);
    const { path, parts } = target;
    if (reserved(parts) || (parts.length && excluded(parts))) return send(404);
    let asset = assets.get(path);
    if (!asset && parts[0] !== 'assets' && parts.every((part) => !part.includes('.')) && acceptsHTML(headers.accept)) {
      asset = assets.get('/index.html');
    }
    if (!asset) return send(404);
    send(200, asset.bytes, asset.type);
  };
}

// Injectable factory permits qualification of parser/timeout policy with no socket.
export function createPreviewServer(snapshot, factory = createServer) {
  const server = factory({ maxHeaderSize: LIMITS.maxHeaderBytes, insecureHTTPParser: false }, createPreviewHandler(snapshot));
  server.maxHeadersCount = LIMITS.maxHeaders;
  server.maxConnections = LIMITS.maxConnections;
  server.maxRequestsPerSocket = 1;
  server.headersTimeout = LIMITS.headersTimeoutMs;
  server.requestTimeout = LIMITS.requestTimeoutMs;
  server.keepAliveTimeout = LIMITS.keepAliveTimeoutMs;
  server.setTimeout(LIMITS.socketTimeoutMs, (socket) => socket.destroy());
  server.on('upgrade', (_req, socket) => socket.destroy());
  server.on('connect', (_req, socket) => socket.destroy());
  server.on('clientError', (_error, socket) => socket.destroy());
  // An Expect header must not cause Node to send an automatic 100 Continue.
  server.on('checkContinue', createPreviewHandler(snapshot));
  server.on('checkExpectation', createPreviewHandler(snapshot));
  return server;
}

export function startPreview({
  load = loadSnapshot,
  create = createPreviewServer,
  signals = process,
  setTimeout: schedule = globalThis.setTimeout,
  clearTimeout: unschedule = globalThis.clearTimeout,
  onReady = console.log,
} = {}) {
  return new Promise((ready, failed) => {
    let state = 'starting';
    let server;
    let startupTimer;
    let shutdownTimer;
    let terminalError;
    let finishClosed, rejectClosed;
    const abort = new AbortController();
    const closed = new Promise((done, reject) => { finishClosed = done; rejectClosed = reject; });
    // Startup failures never expose the controller. Runtime failures remain
    // observable through closed, including for the executable entrypoint.
    closed.catch(() => {});
    const removeHooks = () => {
      signals.removeListener('SIGINT', onSignal);
      signals.removeListener('SIGTERM', onSignal);
      server?.removeListener('listening', onListening);
    };
    const finish = () => {
      if (state === 'closed') return;
      state = 'closed';
      unschedule(startupTimer);
      unschedule(shutdownTimer);
      removeHooks();
      if (terminalError) rejectClosed(terminalError);
      else finishClosed();
    };
    const stop = (error) => {
      if (state === 'closing' || state === 'closed') return closed;
      if (error) terminalError = error;
      state = 'closing';
      unschedule(startupTimer);
      removeHooks();
      abort.abort();
      if (!server) { finish(); return closed; }
      shutdownTimer = schedule(() => {
        server.closeAllConnections();
        finish();
      }, LIMITS.shutdownTimeoutMs);
      // Only this factory's server is closed. No port probe, PID lookup or kill.
      server.close(finish);
      server.closeAllConnections();
      return closed;
    };
    const failStartup = (error) => { failed(error); stop(); };
    const onSignal = () => {
      if (state === 'starting') failStartup(new Error('Preview startup cancelled'));
      else stop();
    };
    const onListening = () => {
      if (state !== 'starting') return;
      try {
        onReady(`Document production preview: http://${HOST}:${PORT}; rebuild and restart after changes`);
      } catch (error) { failStartup(error); return; }
      state = 'ready';
      unschedule(startupTimer);
      ready(Object.freeze({ stop: () => stop(), closed }));
    };
    signals.once('SIGINT', onSignal);
    signals.once('SIGTERM', onSignal);
    startupTimer = schedule(() => failStartup(new Error('Preview startup timed out')), LIMITS.startupTimeoutMs);
    // The build launcher uses &&. Missing/unbuilt dist also fails before a
    // listener is constructed. Cancellation/deadline covers snapshotting too.
    let loading;
    try { loading = load(DIST_ROOT); }
    catch (error) { failStartup(error); return; }
    Promise.resolve(loading).then((snapshot) => {
      if (state !== 'starting') return;
      try {
        server = create(snapshot);
        server.on('error', (error) => {
          const problem = error.code === 'EADDRINUSE'
            ? new Error(`Preview ${HOST}:${PORT} is occupied; stop its owner yourself or retry later`, { cause: error })
            : error;
          if (state === 'starting') failStartup(problem);
          else if (state === 'ready') stop(problem);
        });
        server.once('listening', onListening);
        // Aborting cancels a pending listen as well as a ready listener.
        server.listen({ host: HOST, port: PORT, exclusive: true, signal: abort.signal });
      } catch (error) { failStartup(error); }
    }, (error) => { if (state === 'starting') failStartup(error); });
  });
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    if (process.argv.length !== 2) throw new Error('Preview accepts no arguments; build the fixed document-web dist first');
    const preview = await startPreview();
    await preview.closed;
  } catch (error) {
    console.error(`Document preview failed: ${error.message}`);
    process.exitCode = 1;
  }
}
