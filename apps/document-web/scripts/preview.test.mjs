import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { link, mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// These are pure filesystem/handler/factory tests. No HTTP server or socket is
// created, no browser is started, and the injected listener never binds a port.
const modulePath = new URL('./preview.mjs', import.meta.url);
let preview;
try {
  preview = await import(modulePath.href);
} catch (error) {
  if (error.code !== 'ERR_MODULE_NOT_FOUND' || !error.message.includes(fileURLToPath(modulePath))) throw error;
  preview = {};
}
const api = (name) => {
  assert.equal(typeof preview[name], 'function', `preview must implement ${name}`);
  return preview[name];
};
const HTML = '<!doctype html><html lang="ja"><head><link rel="stylesheet" href="/assets/main.12345678.css"></head><body><div id="root"></div><script defer src="/assets/main.12345678.js"></script></body></html>';
const MAIN = 'console.log("production");';

async function fixture(t, additions = {}) {
  const root = await mkdtemp(join(tmpdir(), 'document-preview-unit-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const dist = join(root, 'dist');
  const files = {
    'index.html': HTML,
    'assets/main.12345678.js': MAIN,
    'assets/main.12345678.css': 'body { color: black }',
    'assets/detail.abcdef12.js': 'export const detail = true;',
    ...additions,
  };
  for (const [name, bytes] of Object.entries(files)) {
    await mkdir(dirname(join(dist, name)), { recursive: true });
    await writeFile(join(dist, name), bytes);
  }
  return { root, dist };
}

function request(handler, url = '/index.html', { method = 'GET', headers = {}, rawHeaders, ...other } = {}) {
  const req = {
    url, method,
    headers: { host: '127.0.0.1:8080', ...headers },
    ...other,
  };
  req.rawHeaders = rawHeaders ?? Object.entries(req.headers).flat();
  const result = { status: undefined, headers: {}, body: Buffer.alloc(0) };
  handler(req, {
    writeHead(status, responseHeaders) {
      assert.equal(result.status, undefined, 'writeHead is called once');
      result.status = status;
      result.headers = responseHeaders;
    },
    end(body) { result.body = body === undefined ? Buffer.alloc(0) : Buffer.from(body); },
  });
  assert.ok(Number.isInteger(result.status));
  assert.equal(Number(result.headers['content-length']) >= result.body.length, true);
  return result;
}

async function handlerFixture(t, additions) {
  const { root, dist } = await fixture(t, additions);
  const snapshot = await api('loadSnapshot')(dist);
  return { root, dist, snapshot, handler: api('createPreviewHandler')(snapshot) };
}

test('production defaults fix the dist, loopback address and port', () => {
  assert.equal(preview.HOST, '127.0.0.1');
  assert.equal(preview.PORT, 8080);
  assert.equal(preview.DIST_ROOT, fileURLToPath(new URL('../dist', import.meta.url)));
  assert.ok(Object.isFrozen(preview.LIMITS));
});

test('serves production assets and lazy chunks with explicit MIME types', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const [path, content, type] of [
    ['/index.html', HTML, 'text/html; charset=utf-8'],
    ['/assets/main.12345678.js', MAIN, 'text/javascript; charset=utf-8'],
    ['/assets/detail.abcdef12.js', 'export const detail = true;', 'text/javascript; charset=utf-8'],
    ['/assets/main.12345678.css', 'body { color: black }', 'text/css; charset=utf-8'],
  ]) {
    const response = request(handler, path);
    assert.equal(response.status, 200, path);
    assert.equal(response.body.toString(), content, path);
    assert.equal(response.headers['content-type'], type, path);
    assert.equal(response.headers['content-length'], String(Buffer.byteLength(content)));
  }
});

test('HEAD has the same status and representation headers without any body', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const path of ['/index.html', '/assets/main.12345678.js', '/missing.js', '/v1/session']) {
    const get = request(handler, path);
    const head = request(handler, path, { method: 'HEAD' });
    assert.equal(head.status, get.status, path);
    assert.deepEqual(head.headers, get.headers, path);
    assert.equal(head.body.length, 0, path);
  }
});

test('only explicit HTML extensionless navigation receives the SPA shell', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const path of ['/', '/documents', '/documents/example/versions/working', '/documents/?view=list', '/documents/%E6%96%87%E6%9B%B8']) {
    assert.equal(request(handler, path, { headers: { accept: 'text/html,application/xhtml+xml;q=0.9,*/*;q=0.8' } }).body.toString(), HTML, path);
  }
  for (const accept of [undefined, '*/*', 'application/json', 'text/html;q=0', 'text/html-ish', 'text/html;q=bogus', 'text/html;q=0.0,*/*;q=1']) {
    assert.equal(request(handler, '/documents', { headers: { accept } }).status, 404, String(accept));
  }
  assert.equal(request(handler, '/documents', { headers: { accept: 'TEXT/HTML;q=0.5' } }).status, 200);
  assert.equal(request(handler, '/documents', { method: 'HEAD', headers: { accept: 'text/html' } }).body.length, 0);
});

test('missing assets and asset namespaces never become successful HTML', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const path of ['/missing.js', '/missing.css', '/assets', '/assets/', '/assets/missing', '/documents/missing.json', '/favicon.ico', '/index.html/nope']) {
    const response = request(handler, path, { headers: { accept: 'text/html' } });
    assert.equal(response.status, 404, path);
    assert.notEqual(response.body.toString(), HTML, path);
  }
});

test('API and health namespaces remain unavailable even if dist contains matching files', async (t) => {
  const { handler } = await handlerFixture(t, { 'v1/leak.json': 'secret', 'health/leak.json': 'secret' });
  for (const path of ['/v1', '/v1/', '/v1/session', '/v1/leak.json', '/health', '/health/', '/health/live', '/health/leak.json', '/%76%31/session', '/%68ealth']) {
    const response = request(handler, path, { headers: { accept: 'text/html' } });
    assert.equal(response.status, 404, path);
    assert.equal(response.body.includes('secret'), false);
  }
  assert.equal(request(handler, '/healthcare', { headers: { accept: 'text/html' } }).status, 200);
});

test('maps and dotfiles are excluded without rejecting normal production source maps', async (t) => {
  const { handler, snapshot } = await handlerFixture(t, {
    'assets/main.12345678.js.map': '{"version":3}',
    'assets/main.CSS.MAP': 'sensitive',
    '.env': 'sensitive',
    '.git/config': 'sensitive',
    'assets/.hidden.js': 'sensitive',
  });
  assert.equal(snapshot.fileCount, 4);
  for (const path of ['/assets/main.12345678.js.map', '/assets/main.CSS.MAP', '/.env', '/.git/config', '/assets/.hidden.js', '/assets/%2ehidden.js', '/assets/main.12345678.js.%6dap']) {
    assert.equal(request(handler, path, { headers: { accept: 'text/html' } }).status, 404, path);
  }
});

test('request parser rejects traversal, ambiguous targets and unsafe encodings before lookup', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const path of [
    '/../index.html', '/documents/../index.html', '/./index.html', '//index.html', '/assets//main.js',
    '/%2e%2e/index.html', '/%252e%252e/index.html', '/assets%2fmain.12345678.js', '/assets%5cmain.js',
    '/assets\\main.js', '/%00', '/%01', '/%7f', '/%C0%AF', '/%ED%A0%80', '/%', '/%x0',
    '/index.html#fragment', 'http://127.0.0.1:8080/index.html', '*', 'index.html',
    '/documents\nextra', '/documents?query=one\ntwo', '/documents?query=one\rtwo',
  ]) {
    assert.equal(request(handler, path, { headers: { accept: 'text/html' } }).status, 400, JSON.stringify(path));
  }
  assert.equal(request(handler, `/${'a'.repeat(preview.LIMITS.maxTargetBytes)}`).status, 414);
});

test('only the exact expected Host is admitted and duplicate Host is rejected', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const host of [undefined, '', 'localhost:8080', '127.0.0.1', '127.0.0.1:80', '0.0.0.0:8080', '[::1]:8080', 'example.test:8080', '127.0.0.1:8080.evil', 'user@127.0.0.1:8080', ['127.0.0.1:8080']]) {
    assert.equal(request(handler, '/index.html', { headers: { host } }).status, 421, String(host));
  }
  assert.equal(request(handler, '/index.html', { rawHeaders: ['Host', '127.0.0.1:8080', 'hOSt', 'evil.test'] }).status, 421);
});

test('methods, upgrade and request bodies are refused without reflecting input', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const method of ['POST', 'PUT', 'DELETE', 'OPTIONS', 'CONNECT', 'TRACE', 'PATCH']) {
    const response = request(handler, '/index.html', { method });
    assert.equal(response.status, 405, method);
    assert.equal(response.headers.allow, 'GET, HEAD');
  }
  for (const headers of [
    { upgrade: 'websocket' }, { connection: 'keep-alive, Upgrade' },
    { 'content-length': '1' }, { 'content-length': 'invalid' }, { 'transfer-encoding': 'chunked' },
    { expect: '100-continue' },
  ]) assert.equal(request(handler, '/index.html', { headers }).status, 400, JSON.stringify(headers));
  assert.equal(request(handler, '/index.html', { headers: { 'content-length': '0' } }).status, 200);
});

test('security headers preserve production CSP without eval, CORS or source disclosure', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const response of [request(handler), request(handler, '/missing.js'), request(handler, '/../secret')]) {
    assert.equal(response.headers['content-security-policy'], "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'; base-uri 'self'");
    assert.equal(response.headers['x-content-type-options'], 'nosniff');
    assert.equal(response.headers['referrer-policy'], 'no-referrer');
    assert.equal(response.headers['cache-control'], 'no-store');
    assert.equal(response.headers.connection, 'close');
    assert.equal(response.headers['access-control-allow-origin'], undefined);
    assert.equal(response.headers['x-powered-by'], undefined);
  }
});

test('snapshot survives replacement and removal of dist without serving a mixed generation', async (t) => {
  const { handler, dist, snapshot } = await handlerFixture(t);
  assert.ok(Object.isFrozen(snapshot));
  await writeFile(join(dist, 'index.html'), 'replacement');
  await writeFile(join(dist, 'assets/main.12345678.js'), 'replacement');
  await rm(dist, { recursive: true });
  assert.equal(request(handler).body.toString(), HTML);
  assert.equal(request(handler, '/assets/main.12345678.js').body.toString(), MAIN);
  // A returned response cannot mutate bytes retained by the snapshot.
  const response = request(handler);
  response.body.fill(0);
  assert.equal(request(handler).body.toString(), HTML);
});

test('missing, empty and unbuilt dist fail before any server can be created', async (t) => {
  const { root, dist } = await fixture(t);
  await assert.rejects(() => api('loadSnapshot')(join(root, 'missing')), /dist|ENOENT/);
  await rm(join(dist, 'index.html'));
  await assert.rejects(() => api('loadSnapshot')(dist), /index\.html/);
  await writeFile(join(dist, 'index.html'), '');
  await assert.rejects(() => api('loadSnapshot')(dist), /index\.html|production/);
  await writeFile(join(dist, 'index.html'), '<!doctype html><html><body><div id="root"></div></body></html>');
  await assert.rejects(() => api('loadSnapshot')(dist), /production|script/);
  await writeFile(join(dist, 'index.html'), HTML.replace('main.12345678.js', 'missing.js'));
  await assert.rejects(() => api('loadSnapshot')(dist), /script|asset/);
});

test('symlink roots, ancestors and members fail closed', async (t) => {
  const { root, dist } = await fixture(t);
  const rootAlias = join(root, 'dist-alias');
  await symlink(dist, rootAlias, 'dir');
  await assert.rejects(() => api('loadSnapshot')(rootAlias), /symbolic|symlink/i);
  const ancestor = join(root, 'ancestor');
  await symlink(root, ancestor, 'dir');
  await assert.rejects(() => api('loadSnapshot')(join(ancestor, 'dist')), /symbolic|symlink/i);
  await symlink(join(dist, 'index.html'), join(dist, 'assets', 'linked.js'));
  await assert.rejects(() => api('loadSnapshot')(dist), /symbolic|symlink/i);
  await rm(join(dist, 'assets', 'linked.js'));
  await symlink(join(dist, 'index.html'), join(dist, '.ignored'));
  await assert.rejects(() => api('loadSnapshot')(dist), /symbolic|symlink/i);
});

test('hardlinked assets and source maps fail closed', async (t) => {
  const { dist } = await fixture(t);
  const original = join(dist, 'assets/main.12345678.js');
  await link(original, join(dist, 'assets/linked.js'));
  await assert.rejects(() => api('loadSnapshot')(dist), /hardlink|link count/i);
  await rm(join(dist, 'assets/linked.js'));
  await link(original, join(dist, 'assets/linked.js.map'));
  await assert.rejects(() => api('loadSnapshot')(dist), /hardlink|link count/i);
});

test('snapshot rejects unknown extensions and unsafe names', async (t) => {
  const { dist } = await fixture(t, { 'secret.pem': 'private material' });
  await assert.rejects(() => api('loadSnapshot')(dist), /extension|asset type/i);
  await rm(join(dist, 'secret.pem'));
  await writeFile(join(dist, 'assets', 'bad%name.js'), 'bad');
  await assert.rejects(() => api('loadSnapshot')(dist), /name|path/i);
});

test('file, entry, byte and depth limits bound snapshot resource use', async (t) => {
  const { dist } = await fixture(t);
  for (const [limits, reason] of [
    [{ maxFileBytes: 8 }, /file.*limit|large/i],
    [{ maxTotalBytes: 8 }, /total.*limit|large/i],
    [{ maxFiles: 1 }, /file.*limit/i],
    [{ maxEntries: 1 }, /entr.*limit/i],
    [{ maxDepth: 1 }, /depth.*limit/i],
  ]) await assert.rejects(() => api('loadSnapshot')(dist, limits), reason);
  for (const limits of [{ maxFiles: 0 }, { maxFiles: Infinity }, { maxFiles: -1 }, { maxFiles: 1.5 }, { maxFiles: preview.LIMITS.maxFiles + 1 }, { unknown: 1 }]) {
    await assert.rejects(() => api('loadSnapshot')(dist, limits), /limit/i);
  }
});

class FakeServer extends EventEmitter {
  constructor() {
    super();
    this.listenCalls = [];
    this.closeCalls = 0;
    this.closeAllCalls = 0;
    this.listening = false;
    this.closeCompletes = true;
  }
  listen(options) { this.listenCalls.push(options); return this; }
  ready() { this.listening = true; this.emit('listening'); }
  close(callback) { this.closeCalls++; this.listening = false; if (this.closeCompletes) callback?.(); }
  closeAllConnections() { this.closeAllCalls++; }
  setTimeout(milliseconds, callback) { this.timeout = milliseconds; this.timeoutCallback = callback; }
}

function fakeDependencies() {
  const server = new FakeServer();
  const signals = new EventEmitter();
  const timers = new Map();
  const logs = [];
  let nextTimer = 0;
  const runtime = {
    signals,
    setTimeout(callback, milliseconds) { const token = ++nextTimer; timers.set(token, { callback, milliseconds }); return token; },
    clearTimeout(token) { timers.delete(token); },
    onReady(message) { logs.push(message); },
  };
  return { server, signals, timers, logs, runtime };
}

test('factory fixes HTTP parser, connection and timeout bounds without binding a socket', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const fake = fakeDependencies();
  let options, handler;
  const server = api('createPreviewServer')(snapshot, (receivedOptions, receivedHandler) => {
    options = receivedOptions; handler = receivedHandler; return fake.server;
  });
  assert.equal(server, fake.server);
  assert.deepEqual(options, { maxHeaderSize: preview.LIMITS.maxHeaderBytes, insecureHTTPParser: false });
  assert.equal(server.maxHeadersCount, preview.LIMITS.maxHeaders);
  assert.equal(server.headersTimeout, preview.LIMITS.headersTimeoutMs);
  assert.equal(server.requestTimeout, preview.LIMITS.requestTimeoutMs);
  assert.equal(server.keepAliveTimeout, preview.LIMITS.keepAliveTimeoutMs);
  assert.equal(server.maxConnections, preview.LIMITS.maxConnections);
  assert.equal(server.maxRequestsPerSocket, 1);
  assert.equal(server.timeout, preview.LIMITS.socketTimeoutMs);
  assert.equal(server.listenCalls.length, 0);
  assert.equal(request(handler).status, 200);
  for (const event of ['upgrade', 'connect', 'clientError']) {
    let destroyed = 0;
    const socket = { destroy() { destroyed++; } };
    if (event === 'clientError') server.emit(event, new Error('bad request'), socket);
    else server.emit(event, {}, socket, Buffer.alloc(0));
    assert.equal(destroyed, 1, event);
  }
  let destroyed = 0;
  server.timeoutCallback({ destroy() { destroyed++; } });
  assert.equal(destroyed, 1);
});

test('owned startup becomes ready only for its listener and bounded shutdown closes only it', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const { server, runtime, signals, logs, timers } = fakeDependencies();
  const start = api('startPreview')({ load: async (root) => { assert.equal(root, preview.DIST_ROOT); return snapshot; }, create: () => server, ...runtime });
  await Promise.resolve();
  assert.equal(server.listenCalls.length, 1);
  const { signal, ...listenOptions } = server.listenCalls[0];
  assert.deepEqual(listenOptions, { host: '127.0.0.1', port: 8080, exclusive: true });
  assert.ok(signal instanceof AbortSignal);
  assert.deepEqual(logs, []);
  assert.equal([...timers.values()][0].milliseconds, preview.LIMITS.startupTimeoutMs);
  server.ready();
  const control = await start;
  assert.equal(logs.length, 1);
  assert.match(logs[0], /127\.0\.0\.1:8080/);
  assert.match(logs[0], /rebuild.*restart/i);
  assert.equal(timers.size, 0);
  signals.emit('SIGTERM');
  await control.closed;
  assert.equal(server.closeCalls, 1);
  assert.equal(server.closeAllCalls, 1);
  await control.stop();
  assert.equal(server.closeCalls, 1);
  assert.equal(signals.listenerCount('SIGTERM'), 0);
  assert.equal(signals.listenerCount('SIGINT'), 0);
  assert.equal(timers.size, 0);
});

test('snapshot/build absence prevents listener construction and startup', async () => {
  let created = 0;
  await assert.rejects(() => api('startPreview')({
    load: async () => { throw new Error('production dist unavailable; build first'); },
    create: () => { created++; throw new Error('must not create'); },
  }), /build first/);
  assert.equal(created, 0);
});

test('occupied port is reported without readiness, probing or reusing another process', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const { server, runtime, signals, logs, timers } = fakeDependencies();
  const start = api('startPreview')({ load: async () => snapshot, create: () => server, ...runtime });
  await Promise.resolve();
  server.emit('error', Object.assign(new Error('busy'), { code: 'EADDRINUSE' }));
  await assert.rejects(start, /127\.0\.0\.1:8080.*occupied/i);
  assert.deepEqual(logs, []);
  assert.equal(server.listenCalls.length, 1);
  assert.equal(signals.listenerCount('SIGTERM'), 0);
  assert.equal(timers.size, 0);
});

test('startup timeout and cancellation close the owned pending listener', async (t) => {
  const { snapshot } = await handlerFixture(t);
  for (const cancel of [false, true]) {
    const { server, runtime, signals, timers, logs } = fakeDependencies();
    const start = api('startPreview')({ load: async () => snapshot, create: () => server, ...runtime });
    await Promise.resolve();
    if (cancel) signals.emit('SIGINT');
    else [...timers.values()][0].callback();
    await assert.rejects(start, cancel ? /cancel/i : /startup.*timed out/i);
    assert.equal(server.closeCalls, 1);
    assert.deepEqual(logs, []);
    assert.equal(signals.listenerCount('SIGINT'), 0);
    assert.equal(timers.size, 0);
  }
});

test('shutdown timeout forcibly closes only owned connections and removes hooks', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const { server, runtime, signals, timers } = fakeDependencies();
  server.closeCompletes = false;
  const start = api('startPreview')({ load: async () => snapshot, create: () => server, ...runtime });
  await Promise.resolve();
  server.ready();
  const control = await start;
  const stopped = control.stop();
  assert.equal([...timers.values()][0].milliseconds, preview.LIMITS.shutdownTimeoutMs);
  [...timers.values()][0].callback();
  await stopped;
  await control.closed;
  assert.equal(server.closeCalls, 1);
  assert.equal(server.closeAllCalls >= 1, true);
  assert.equal(signals.listenerCount('SIGTERM'), 0);
  assert.equal(timers.size, 0);
});

test('handler rejects header count and byte ceilings rather than trusting truncated headers', async (t) => {
  const { handler } = await handlerFixture(t);
  const rawHeaders = ['Host', '127.0.0.1:8080'];
  for (let index = 1; index < preview.LIMITS.maxHeaders; index++) rawHeaders.push(`X-${index}`, 'x');
  assert.equal(request(handler, '/index.html', { rawHeaders }).status, 431);
  assert.equal(request(handler, '/index.html', { headers: { 'x-large': 'x'.repeat(preview.LIMITS.maxHeaderBytes) } }).status, 431);
});

test('startup snapshotting has a deadline and cancellation prevents late listener creation', async (t) => {
  const { snapshot } = await handlerFixture(t);
  for (const cancel of [false, true]) {
    const { server, runtime, signals, timers } = fakeDependencies();
    let resolveLoad, created = 0;
    const loading = new Promise((resolve) => { resolveLoad = resolve; });
    const start = api('startPreview')({ load: () => loading, create: () => { created++; return server; }, ...runtime });
    const failure = assert.rejects(start, cancel ? /cancel/i : /startup.*timed out/i);
    assert.equal(timers.size, 1);
    if (cancel) signals.emit('SIGTERM');
    else [...timers.values()][0].callback();
    await failure;
    resolveLoad(snapshot);
    await Promise.resolve();
    assert.equal(created, 0);
    assert.equal(signals.listenerCount('SIGTERM'), 0);
    assert.equal(timers.size, 0);
  }
});

test('cancellation aborts the owned pending listen operation', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const { server, runtime, signals } = fakeDependencies();
  const start = api('startPreview')({ load: async () => snapshot, create: () => server, ...runtime });
  await Promise.resolve();
  assert.ok(server.listenCalls[0].signal instanceof AbortSignal);
  assert.equal(server.listenCalls[0].signal.aborted, false);
  signals.emit('SIGINT');
  await assert.rejects(start, /cancel/i);
  assert.equal(server.listenCalls[0].signal.aborted, true);
});

test('an error after readiness is observable and closes the owned listener', async (t) => {
  const { snapshot } = await handlerFixture(t);
  const { server, runtime, signals } = fakeDependencies();
  const start = api('startPreview')({ load: async () => snapshot, create: () => server, ...runtime });
  await Promise.resolve();
  server.ready();
  const control = await start;
  const closed = assert.rejects(control.closed, /listener failed/);
  server.emit('error', new Error('listener failed'));
  await closed;
  assert.equal(server.closeCalls, 1);
  assert.equal(signals.listenerCount('SIGINT'), 0);
});

test('built index requires every stylesheet and image reference to exist in the snapshot', async (t) => {
  const { dist } = await fixture(t);
  await rm(join(dist, 'assets/main.12345678.css'));
  await assert.rejects(() => api('loadSnapshot')(dist), /asset|reference/i);
  await writeFile(join(dist, 'assets/main.12345678.css'), 'body {}');
  await writeFile(join(dist, 'index.html'), HTML.replace('</body>', '<img src="/assets/missing.png"></body>'));
  await assert.rejects(() => api('loadSnapshot')(dist), /asset|reference/i);
  await writeFile(join(dist, 'index.html'), HTML.replace('/assets/main.12345678.css', 'https://example.invalid/style.css'));
  await assert.rejects(() => api('loadSnapshot')(dist), /asset|reference/i);
});

test('encoded spaces in safe navigation paths and search strings remain usable', async (t) => {
  const { handler } = await handlerFixture(t);
  for (const target of ['/documents/a%20b', '/documents?query=hello%20world']) {
    assert.equal(request(handler, target, { headers: { accept: 'text/html' } }).status, 200, target);
  }
});

test('unrelated sibling writes do not invalidate immutable dist snapshots', async (t) => {
  const additions = Object.fromEntries(Array.from({ length: 64 }, (_, index) => [`assets/chunk-${index}.js`, `export const value = ${index};`]));
  const { root, dist } = await fixture(t, additions);
  const loading = api('loadSnapshot')(dist);
  // /root is outside dist. Concurrent build/test bookkeeping there may change
  // its directory timestamps but cannot change the captured dist generation.
  const writing = (async () => {
    for (let index = 0; index < 32; index++) await writeFile(join(root, `sibling-${index}.txt`), 'unrelated');
  })();
  const [snapshot] = await Promise.all([loading, writing]);
  assert.equal(snapshot.fileCount, 68);
  assert.equal(request(api('createPreviewHandler')(snapshot)).body.toString(), HTML);
});


test('navigation query values stay opaque instead of undergoing filesystem path validation', async (t) => {
  const { handler } = await handlerFixture(t);
  const queries = [
    'titleContains=100%25',
    new URLSearchParams({ titleContains: '100% / \\ ../ %2e ? # & = 日本語' }).toString(),
    'titleContains=%2Fdocuments%5Cdraft&titleContains=%252e%252e',
    'titleContains=%00',
    'titleContains=%',
    'titleContains=%C0%AF',
  ];
  for (const query of queries) {
    const target = `/documents?${query}`;
    const get = request(handler, target, { headers: { accept: 'text/html' } });
    assert.equal(get.status, 200, target);
    assert.equal(get.body.toString(), HTML, target);
    const head = request(handler, target, { method: 'HEAD', headers: { accept: 'text/html' } });
    assert.equal(head.status, 200, target);
    assert.equal(head.body.length, 0, target);
    assert.equal(head.headers['content-length'], String(Buffer.byteLength(HTML)), target);
    assert.equal(request(handler, target).status, 404, target);
  }
});

test('opaque query values cannot change asset lookup, reserved namespaces or path rejection', async (t) => {
  const { handler } = await handlerFixture(t);
  const query = '?titleContains=100%25%2F%5C%252e&path=%2Findex.html';
  assert.equal(request(handler, `/assets/main.12345678.js${query}`).body.toString(), MAIN);
  for (const path of ['/assets/missing.js', '/v1/session', '/health', '/assets/main.12345678.js.map', '/.env']) {
    assert.equal(request(handler, `${path}${query}`, { headers: { accept: 'text/html' } }).status, 404, path);
  }
  for (const path of ['/documents/../index.html', '/%252e%252e/index.html', '/assets%2Fmain.12345678.js', '/assets%5Cmain.12345678.js', '/%25', '/%00', '/%']) {
    assert.equal(request(handler, `${path}${query}`, { headers: { accept: 'text/html' } }).status, 400, path);
  }
});
