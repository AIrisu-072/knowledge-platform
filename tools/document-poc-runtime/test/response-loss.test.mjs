import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import * as harness from '../harness.mjs';

test('response-loss control sends exact payload to real upstream then drops response without an automatic retry', async t => {
  let requests = 0, observed;
  const upstream = createServer(async (req, res) => {
    requests++; let bytes = ''; for await (const chunk of req) bytes += chunk;
    observed = { method: req.method, path: req.url, body: JSON.parse(bytes) };
    res.writeHead(200, { 'content-type': 'application/json' }); res.end('{"operationId":"same","changed":true}');
  });
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => upstream.close(resolve)));
  const body = { operationId: 'same', expectedDocumentRevision: 2, set: { synthetic: true }, unset: [] };
  const result = await harness.interruptMutationResponse(`http://127.0.0.1:${upstream.address().port}`, '/v1/documents/synthetic/metadata', body);
  assert.equal(result.upstreamStatus, 200); assert.equal(result.responseLost, true);
  assert.equal(requests, 1);
  assert.deepEqual(observed, { method: 'PATCH', path: '/v1/documents/synthetic/metadata', body });
  assert.match(result.payloadSha256, /^[a-f0-9]{64}$/);
  assert.deepEqual(Object.keys(result).sort(), ['payloadSha256', 'responseLost', 'upstreamStatus']);
});
test('response-loss control refuses off-loopback destinations and unexpected paths', async () => {
  for (const [origin, path] of [['https://external.invalid', '/v1/documents/x/metadata'], ['http://127.0.0.1:1', '//external.invalid']]) {
    await assert.rejects(harness.interruptMutationResponse(origin, path, {}), /loopback|metadata/);
  }
});

test('response-loss no-dispatch rejection and stalled upstream both settle and close the owned listener', async () => {
  // A child bounds RED failures too: a broken controller cannot hang this suite
  // or leak its private listener into a later acceptance run.
  const { execFile } = await import('node:child_process');
  const { promisify } = await import('node:util');
  const run = promisify(execFile);
  const module = new URL('../response-loss.mjs', import.meta.url).href;
  for (const mode of ['no-dispatch', 'stalled-upstream']) {
    const source = `
      import assert from 'node:assert/strict';
      import { createServer } from 'node:http';
      import { interruptMutationResponse } from ${JSON.stringify(module)};
      const mode = ${JSON.stringify(mode)};
      let upstream, dispatched = false, origin = 'http://127.0.0.1:1';
      if (mode === 'no-dispatch') globalThis.fetch = async () => { throw Error('pre-dispatch rejection'); };
      else {
        upstream = createServer(() => { dispatched = true; });
        await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
        origin = 'http://127.0.0.1:' + upstream.address().port;
      }
      try {
        await assert.rejects(interruptMutationResponse(origin, '/v1/documents/synthetic/metadata', {}, { timeoutMs: 200 }));
        if (mode === 'stalled-upstream') assert.equal(dispatched, true);
      } finally {
        if (upstream) { upstream.closeAllConnections(); await new Promise(resolve => upstream.close(resolve)); }
      }
      // libuv closes the resource after close callbacks, on the following turn.
      await new Promise(resolve => setImmediate(resolve));
      await new Promise(resolve => setImmediate(resolve));
      assert.equal(process.getActiveResourcesInfo().filter(name => name === 'TCPServerWrap').length, 0);
      console.log('closed');
    `;
    const result = await run(process.execPath, ['--input-type=module', '-e', source], { timeout: 1500 });
    assert.equal(result.stdout.trim(), 'closed', mode);
  }
});

// These explicitly named pure cases never create a listener, socket, or browser.
import * as workingLoss from '../response-loss.mjs';
const humanOrigin = 'http://127.0.0.1:41001';
const documentId = '00000000-0000-4000-8000-000000000001';
const versionId = '00000000-0000-4000-8000-000000000002';
const versionPath = `/v1/documents/${documentId}/versions`;
const multipartType = 'multipart/form-data; boundary=synthetic-fixed';
const multipart = Buffer.from('--synthetic-fixed\r\nsynthetic bytes\r\n--synthetic-fixed--\r\n');
const mutationResult = { documentId, targetVersionId: versionId, operationId: 'synthetic', resultingRevision: 2 };
const makeGuard = () => workingLoss.workingResponseLossGuard(humanOrigin);
const sendMutation = (guard, method = 'POST', path = versionPath, body = multipart, contentType = multipartType) => {
  const kind = guard.receive(method, humanOrigin + path, { 'content-type': contentType });
  guard.dispatch(kind, body, contentType); return kind;
};
const recover = guard => {
  guard.arm({ method: 'POST', path: versionPath });
  guard.complete(sendMutation(guard), 201, mutationResult);
  guard.allowRetry(); guard.complete(sendMutation(guard), 201, mutationResult);
};

test('pure working response-loss guard preserves bytes and counts only an explicitly armed retry', () => {
  assert.equal(typeof workingLoss.workingResponseLossGuard, 'function');
  const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
  assert.equal(guard.receive('GET', `${humanOrigin}/assets/app.js`, {}), 'read');
  guard.complete(sendMutation(guard), 201, mutationResult);
  const dropped = guard.receipt();
  assert.deepEqual([dropped.received, dropped.dispatched, dropped.dropped, dropped.unexpected], [1, 1, 1, 0]);
  assert.equal(dropped.payloadBytes, multipart.length); assert.equal(dropped.contentType, multipartType);
  assert.match(dropped.payloadSha256, /^[a-f0-9]{64}$/); assert.deepEqual(dropped.result, mutationResult);
  guard.allowRetry(); guard.complete(sendMutation(guard), 201, mutationResult);
  const recovered = guard.assertRecovered();
  assert.deepEqual([recovered.received, recovered.dispatched, recovered.dropped, recovered.retryStatus], [2, 2, 1, 201]);
  assert.deepEqual(recovered.retryResult, mutationResult); assert.equal(dropped.received, 1);
  assert.equal(recovered.bytesEqual, true); assert.equal(recovered.contentTypeEqual, true);
  assert.equal(Object.values(recovered).some(Buffer.isBuffer), false);
});

test('pure working response-loss guard rejects automatic retries and altered retry bytes or type', () => {
  for (const mode of ['automatic', 'bytes', 'content-type', 'result', 'status']) {
    const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
    guard.complete(sendMutation(guard), 201, mutationResult);
    if (mode !== 'automatic') guard.allowRetry();
    assert.throws(() => {
      const kind = sendMutation(guard, 'POST', versionPath, mode === 'bytes' ? Buffer.from('changed') : multipart,
        mode === 'content-type' ? `${multipartType}-changed` : multipartType);
      guard.complete(kind, mode === 'status' ? 500 : 201, mode === 'result' ? { ...mutationResult, resultingRevision: 3 } : mutationResult);
    }, /Unexpected|changed|success/);
    assert.throws(() => guard.assertRecovered()); assert.throws(() => guard.arm({ method: 'PUT', path: `${versionPath}/${versionId}` }));
    assert.equal(guard.receipt().unexpected, 1);
  }
});

test('pure working response-loss guard restricts origin credentials methods sizes and exact targets', () => {
  for (const origin of ['https://127.0.0.1:41001', 'http://localhost:41001', 'http://user@127.0.0.1:41001', 'http://127.1:41001', `${humanOrigin}/x`]) {
    assert.throws(() => workingLoss.workingResponseLossGuard(origin), /loopback/);
  }
  for (const [method, url, headers] of [['CONNECT', humanOrigin, {}], ['GET', 'http://external.invalid/x', {}],
    ['GET', `http://user@127.0.0.1:41001/x`, {}], ['GET', `${humanOrigin}/x`, { Cookie: 'secret' }],
    ['GET', `${humanOrigin}/x`, { authorization: 'secret' }], ['GET', `${humanOrigin}/x`, { 'proxy-authorization': 'secret' }],
    ['GET', `${humanOrigin}/x`, { upgrade: 'websocket' }], ['DELETE', versionPath, {}], ['POST', `${versionPath}:rebase`, {}]]) {
    const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
    assert.throws(() => guard.receive(method, url, headers)); assert.equal(guard.receipt().unexpected, 1);
  }
  for (const size of [workingLoss.SYNTHETIC_MULTIPART_LIMIT + 1, 0]) {
    const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
    assert.throws(() => sendMutation(guard, 'POST', versionPath, Buffer.alloc(size)), /synthetic/);
  }
  assert.throws(() => makeGuard().arm({ method: 'POST', path: '/v1/documents' }));
});

test('pure working response-loss guard allows only same-document POST then one PUT and one recovered publish', () => {
  const guard = makeGuard(); recover(guard);
  guard.arm({ method: 'PUT', path: `${versionPath}/${versionId}` });
  const updated = { ...mutationResult, resultingRevision: 3 };
  guard.complete(sendMutation(guard, 'PUT', `${versionPath}/${versionId}`), 200, updated);
  guard.allowRetry(); guard.complete(sendMutation(guard, 'PUT', `${versionPath}/${versionId}`), 200, updated);
  assert.equal(guard.assertRecovered().dispatched, 2);
  guard.allowPublish(`${versionPath}/${versionId}:publish`);
  const publish = guard.receive('POST', `${humanOrigin}${versionPath}/${versionId}:publish`, { 'content-type': 'application/json' });
  guard.dispatch(publish, Buffer.from('{}'), 'application/json'); guard.complete(publish, 200, {});
  assert.equal(guard.receipt().published, 1);
  assert.throws(() => guard.receive('POST', `${humanOrigin}${versionPath}/${versionId}:publish`, {}));
  for (const action of [g => g.arm({ method: 'PUT', path: `${versionPath}/${documentId}` }),
    g => g.allowPublish(`${versionPath}/${documentId}:publish`), g => g.arm({ method: 'POST', path: versionPath })]) {
    const other = makeGuard(); recover(other); assert.throws(() => action(other));
  }
});

test('pure working response-loss wrapper rejects invalid observation windows before creating a listener', async () => {
  assert.equal(typeof workingLoss.withWorkingResponseLoss, 'function');
  for (const timeoutMs of [0, -1, 50_001, Infinity]) {
    await assert.rejects(workingLoss.withWorkingResponseLoss(humanOrigin, () => assert.fail('callback ran'), { timeoutMs }), /window/);
  }
});

test('pure working response-loss observations start at each explicit send, not at page scope entry', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = (await readFile(new URL('../response-loss.mjs', import.meta.url), 'utf8')).split('export async function withWorkingResponseLoss')[1];
  assert.match(source, /arm: operation => \{[^\n]+startObservation\(\)/);
  assert.match(source, /allowRetry: \(\) => \{ guard\.allowRetry\(\); startObservation\(\); \}/);
  assert.match(source, /guard\.complete\(kind, incoming\.statusCode, result\); stopObservation\(\)/);
  assert.doesNotMatch(source, /const timeout = setTimeout/);
});

// Hosted-only HTTP cases. Local verification explicitly selects ^pure working.
test('hosted HTTP working loss forwards raw compressed reads and exact multipart retries, then closes its listener', async t => {
  const { request } = await import('node:http');
  const { gzipSync } = await import('node:zlib');
  let writes = 0;
  const upstream = createServer(async (req, res) => {
    const body = Buffer.concat(await Array.fromAsync(req));
    if (req.method === 'GET') {
      res.writeHead(200, { 'content-type': 'text/plain', 'content-encoding': 'gzip' }); res.end(gzipSync('synthetic read')); return;
    }
    writes++; assert.equal(body.equals(multipart), true); assert.equal(req.headers['content-type'], multipartType);
    res.writeHead(201, { 'content-type': 'application/json', 'content-encoding': 'gzip' }); res.end(gzipSync(JSON.stringify(mutationResult)));
  });
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  t.after(() => { upstream.closeAllConnections(); return new Promise(resolve => upstream.close(resolve)); });
  const origin = `http://127.0.0.1:${upstream.address().port}`;
  const through = (proxy, method, path, body) => new Promise((resolve, reject) => {
    const outgoing = request(proxy, { method, path: origin + path, headers: body ? { 'content-type': multipartType } : {} }, async incoming => {
      try { resolve({ status: incoming.statusCode, headers: incoming.headers, body: Buffer.concat(await Array.fromAsync(incoming)) }); } catch (error) { reject(error); }
    });
    outgoing.on('error', reject); outgoing.end(body);
  });
  let proxy;
  await workingLoss.withWorkingResponseLoss(origin, async control => {
    proxy = control.origin;
    const read = await through(proxy, 'GET', '/asset');
    assert.equal(read.headers['content-encoding'], 'gzip'); assert.equal(read.body.equals(gzipSync('synthetic read')), true);
    control.arm({ method: 'POST', path: versionPath });
    const lost = assert.rejects(through(proxy, 'POST', versionPath, multipart));
    assert.equal((await control.dropped()).upstreamStatus, 201); await lost;
    control.allowRetry(); const replay = await through(proxy, 'POST', versionPath, multipart);
    assert.equal(replay.status, 201); assert.equal(replay.body.equals(gzipSync(JSON.stringify(mutationResult))), true);
    assert.equal((await control.assertRecovered()).dispatched, 2);
  });
  assert.equal(writes, 2); await assert.rejects(through(proxy, 'GET', '/asset'), /ECONNREFUSED/);
});

test('hosted HTTP working loss bounds stalled upstream and closes accepted sockets', async t => {
  const { request } = await import('node:http');
  const upstream = createServer(() => {});
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  t.after(() => { upstream.closeAllConnections(); return new Promise(resolve => upstream.close(resolve)); });
  const origin = `http://127.0.0.1:${upstream.address().port}`;
  await assert.rejects(workingLoss.withWorkingResponseLoss(origin, async control => {
    control.arm({ method: 'POST', path: versionPath });
    const outgoing = request(control.origin, { method: 'POST', path: origin + versionPath, headers: { 'content-type': multipartType } });
    outgoing.on('error', () => {}); outgoing.end(multipart);
    await control.dropped();
  }, { timeoutMs: 100 }), /window/);
});
