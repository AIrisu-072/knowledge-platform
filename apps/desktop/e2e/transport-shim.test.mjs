// Unit test of the desktop shell's transport shim (src-tauri/src/transport-shim.js)
// in Node's fetch implementation: `node --test apps/desktop/e2e/transport-shim.test.mjs`.
// Uses the Windows-style http app origin, because Node gives custom schemes an
// opaque origin (WebKit does not; the real-GUI run covers tauri://).
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

const source = await readFile(new URL('../src-tauri/src/transport-shim.js', import.meta.url), 'utf8');
const APP = 'http://tauri.localhost';

function install() {
  const calls = [];
  const window = {
    location: { href: `${APP}/documents`, origin: APP },
    // Like the platform fetch: builds a Request from what it receives (and so
    // rejects a Request whose body was already used).
    fetch: async (input, init) => {
      const request = new Request(input, init);
      calls.push({ request, body: request.body ? new Uint8Array(await request.arrayBuffer()) : null, raw: { input, init } });
      return new Response('ok');
    },
  };
  new Function('window', source)(window);
  return { fetch: window.fetch, calls };
}

test('same-origin multipart bodies reach the platform as bytes with the same boundary and content', async () => {
  const { fetch, calls } = install();
  const form = new FormData();
  form.append('request', new Blob(['{"title":"合成"}'], { type: 'application/json' }));
  form.append('file', new Blob([new Uint8Array([0, 1, 2, 255])], { type: 'application/octet-stream' }), 'a.bin');
  const expected = new Request(`${APP}/v1/documents`, { method: 'POST', body: form });
  const expectedBytes = new Uint8Array(await expected.clone().arrayBuffer());
  await fetch(`${APP}/v1/documents`, { method: 'POST', body: form });
  const [call] = calls;
  assert.ok(call.raw.init.body instanceof ArrayBuffer, 'body is materialized before the platform sees it');
  assert.equal(call.request.method, 'POST');
  assert.match(call.request.headers.get('content-type'), /^multipart\/form-data; boundary=/);
  const boundary = call.request.headers.get('content-type').split('boundary=')[1];
  assert.ok(new TextDecoder().decode(call.body).includes(boundary), 'the header boundary is the one in the body');
  assert.equal(call.body.length, expectedBytes.length);
});

test('same-origin Request objects (generated client) keep method, headers and bytes', async () => {
  const { fetch, calls } = install();
  await fetch(new Request(`${APP}/v1/organization/tasks/1/claim`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"a":1}' }));
  assert.equal(calls[0].request.method, 'POST');
  assert.equal(calls[0].request.headers.get('content-type'), 'application/json');
  assert.equal(new TextDecoder().decode(calls[0].body), '{"a":1}');
});

test('cross-origin requests with a body pass through unconsumed (Request input)', async () => {
  const { fetch, calls } = install();
  const response = await fetch(new Request('ipc://localhost/local_workspace_runtime', { method: 'POST', body: '{"command":"capabilities"}' }));
  assert.equal(await response.text(), 'ok');
  assert.equal(new TextDecoder().decode(calls[0].body), '{"command":"capabilities"}');
});

test('cross-origin and GET requests are not rewritten', async () => {
  const { fetch, calls } = install();
  await fetch('https://elsewhere.example/x', { method: 'POST', body: 'x' });
  await fetch(`${APP}/v1/organization/session`);
  assert.ok(!(calls[0].raw.init?.body instanceof ArrayBuffer), 'cross-origin body is not materialized by the shim');
  assert.equal(calls[0].request.url, 'https://elsewhere.example/x');
  assert.equal(new TextDecoder().decode(calls[0].body), 'x');
  assert.equal(calls[1].request.method, 'GET');
  assert.equal(calls[1].body, null);
});

test('an aborted signal still aborts a materialized same-origin request', async () => {
  const { fetch, calls } = install();
  const controller = new AbortController();
  controller.abort();
  await fetch(`${APP}/v1/documents`, { method: 'POST', body: new Blob(['x']), signal: controller.signal }).catch(() => undefined);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].request.signal.aborted, true, 'the platform fetch receives the aborted signal');
});
