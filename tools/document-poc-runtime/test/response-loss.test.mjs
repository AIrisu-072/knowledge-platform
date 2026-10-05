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
  assert.match(source, /response\.socket\.end\(\); stopObservation\(\)/);
  assert.doesNotMatch(source, /const timeout = setTimeout/);
});


test('pure working body loss preserves exact raw prefixes and declared complete lengths for every supported encoding', async () => {
  assert.equal(typeof workingLoss.workingResponseLossBody, 'function');
  const { gzipSync, deflateSync, brotliCompressSync } = await import('node:zlib');
  const json = Buffer.from(JSON.stringify(mutationResult));
  for (const [encoding, encode] of [[undefined, value => value], ['identity', value => value],
    ['gzip', gzipSync], ['deflate', deflateSync], ['br', brotliCompressSync]]) {
    const bytes = encode(json), original = Buffer.from(bytes);
    const headers = { 'content-type': 'application/json', 'x-synthetic-result': 'retained',
      connection: 'keep-alive, x-hop', 'x-hop': 'remove', 'transfer-encoding': 'chunked',
      ...(encoding ? { 'content-encoding': encoding } : {}) };
    const partial = workingLoss.workingResponseLossBody(headers, bytes);
    assert.equal(partial.headers['content-type'], headers['content-type']);
    assert.equal(partial.headers['x-synthetic-result'], headers['x-synthetic-result']);
    assert.equal(partial.headers['content-encoding'], encoding);
    assert.equal(partial.headers['content-length'], bytes.length);
    for (const hop of ['connection', 'x-hop', 'transfer-encoding']) assert.equal(partial.headers[hop], undefined);
    assert.ok(partial.prefix.length > 0 && partial.prefix.length < bytes.length);
    assert.deepEqual(partial.prefix, bytes.subarray(0, partial.prefix.length));
    assert.deepEqual(bytes, original); assert.equal(headers['transfer-encoding'], 'chunked');
  }
});

test('pure working body loss refuses nontruncatable or oversized responses and replaces stale length framing', () => {
  assert.equal(typeof workingLoss.workingResponseLossBody, 'function');
  for (const bytes of [Buffer.alloc(0), Buffer.alloc(1), Buffer.alloc(64 * 1024 + 1)]) {
    assert.throws(() => workingLoss.workingResponseLossBody({}, bytes), /\[working-loss:upstream-result\]/);
  }
  const partial = workingLoss.workingResponseLossBody({ 'content-length': '999', 'content-encoding': 'identity' }, Buffer.from('{}'));
  assert.deepEqual(partial, { headers: { 'content-length': 2, 'content-encoding': 'identity' }, prefix: Buffer.from('{') });
});

test('pure working body loss sends verified upstream headers and a raw prefix before FIN without ending the declared body', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = (await readFile(new URL('../response-loss.mjs', import.meta.url), 'utf8')).split('export async function withWorkingResponseLoss')[1];
  const initial = source.slice(source.indexOf("if (kind === 'initial') {"), source.indexOf('else {', source.indexOf("if (kind === 'initial') {")));
  assert.match(initial, /workingResponseLossBody\(incoming\.headers, bytes\)/);
  const sequence = ['response.writeHead(incoming.statusCode, partial.headers)', 'await new Promise', 'response.write(partial.prefix',
    'response.socket.end()', 'stopObservation()', 'dropped.resolve(guard.receipt())'];
  let position = -1;
  for (const token of sequence) { const next = initial.indexOf(token); assert.ok(next > position, token); position = next; }
  assert.ok(source.indexOf('guard.complete(kind, incoming.statusCode, result)') < source.indexOf("if (kind === 'initial') {"));
  assert.doesNotMatch(initial, /response\.(?:destroy|end|flushHeaders)\(/);
});

// Hosted-only HTTP cases. Local verification explicitly selects ^pure working.
test('hosted HTTP working loss forwards real headers and incomplete raw bodies before exact multipart retries, then closes its listener', async t => {
  const { request } = await import('node:http');
  const { gzipSync } = await import('node:zlib');
  let writes = 0, encoded = false;
  const resultBytes = () => encoded ? gzipSync(JSON.stringify(mutationResult)) : Buffer.from(JSON.stringify(mutationResult));
  const upstream = createServer(async (req, res) => {
    const body = Buffer.concat(await Array.fromAsync(req));
    if (req.method === 'GET') {
      res.writeHead(200, { 'content-type': 'text/plain', 'content-encoding': 'gzip' }); res.end(gzipSync('synthetic read')); return;
    }
    writes++; assert.equal(body.equals(multipart), true); assert.equal(req.headers['content-type'], multipartType);
    res.writeHead(req.method === 'POST' ? 201 : 200, { 'content-type': 'application/json', 'x-synthetic-result': 'retained',
      ...(encoded ? { 'content-encoding': 'gzip' } : {}) }); res.end(resultBytes());
  });
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  t.after(() => { upstream.closeAllConnections(); return new Promise(resolve => upstream.close(resolve)); });
  const origin = `http://127.0.0.1:${upstream.address().port}`;
  const through = (proxy, method, path, body, { agent } = {}) => new Promise((resolve, reject) => {
    const outgoing = request(proxy, { method, path: origin + path, agent, headers: body ? { 'content-type': multipartType } : {} }, async incoming => {
      const chunks = [];
      const observed = () => ({ status: incoming.statusCode, headers: incoming.headers, body: Buffer.concat(chunks), complete: incoming.complete });
      try { for await (const chunk of incoming) chunks.push(chunk); resolve(observed()); }
      catch (error) { reject(Object.assign(error, { response: observed() })); }
    });
    outgoing.on('error', reject); outgoing.end(body);
  });
  for (const compressed of [false, true]) for (const method of ['POST', 'PUT']) {
    encoded = compressed;
    const path = method === 'POST' ? versionPath : `${versionPath}/${versionId}`, status = method === 'POST' ? 201 : 200;
    const before = writes, raw = resultBytes(); let proxy;
    await workingLoss.withWorkingResponseLoss(origin, async control => {
      proxy = control.origin;
      const read = await through(proxy, 'GET', '/asset');
      assert.equal(read.headers['content-encoding'], 'gzip'); assert.equal(read.body.equals(gzipSync('synthetic read')), true);
      control.arm({ method, path });
      const lost = assert.rejects(through(proxy, method, path, multipart), error => {
        // Reject a pre-header disconnect: the real response and raw prefix must
        // arrive before the incomplete Content-Length terminates body reading.
        assert.equal(error.code, 'ECONNRESET');
        const partial = error.response; assert.ok(partial);
        assert.equal(partial.status, status); assert.equal(partial.headers['content-type'], 'application/json');
        assert.equal(partial.headers['content-encoding'], encoded ? 'gzip' : undefined);
        assert.equal(partial.headers['x-synthetic-result'], 'retained');
        assert.equal(Number(partial.headers['content-length']), raw.length);
        assert.equal(partial.headers['transfer-encoding'], undefined); assert.equal(partial.complete, false);
        assert.ok(partial.body.length > 0 && partial.body.length < raw.length);
        assert.deepEqual(partial.body, raw.subarray(0, partial.body.length)); return true;
      });
      const dropped = await control.dropped(); await lost;
      assert.deepEqual([dropped.received, dropped.dispatched, dropped.dropped, dropped.unexpected, dropped.upstreamStatus], [1, 1, 1, 0, status]);
      control.allowRetry(); const replay = await through(proxy, method, path, multipart);
      assert.equal(replay.status, status); assert.deepEqual(replay.body, raw); assert.equal(replay.complete, true);
      const recovered = await control.assertRecovered();
      assert.deepEqual([recovered.received, recovered.dispatched, recovered.dropped, recovered.unexpected], [2, 2, 1, 0]);
      assert.equal(recovered.bytesEqual, true); assert.equal(recovered.contentTypeEqual, true);
      assert.deepEqual(recovered.retryResult, dropped.result);
    });
    assert.equal(writes - before, 2);
    // Listener closure requires a fresh connection, not a just-closed keep-alive socket.
    await assert.rejects(through(proxy, 'GET', '/asset', undefined, { agent: false }), { code: 'ECONNREFUSED' });
  }
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


test('pure working loss diagnostics distinguish unarmed repeats and preserve the first failure through teardown', () => {
  const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
  guard.complete(sendMutation(guard), 201, mutationResult);
  let first;
  assert.throws(() => sendMutation(guard), error => {
    first = error; return /^\[working-loss:unarmed-retry\]/.test(error.message);
  });
  for (const action of [() => guard.allowRetry(), () => guard.assertRecovered(), () => guard.reject('PRIVATE_SECONDARY')]) {
    assert.throws(action, error => error === first);
  }
  assert.deepEqual([guard.receipt().received, guard.receipt().dispatched, guard.receipt().dropped, guard.receipt().unexpected], [2, 1, 1, 1]);
  const wrong = makeGuard(); wrong.arm({ method: 'POST', path: versionPath });
  wrong.complete(sendMutation(wrong), 201, mutationResult);
  assert.throws(() => wrong.receive('PUT', humanOrigin + versionPath, {}), /\[working-loss:admission\]/);
  assert.throws(() => makeGuard().assertRecovered(), /\[working-loss:unrecovered\]/);
});

test('pure working loss diagnostics label existing guard rejection sites without relaxing them', () => {
  const cases = [
    ['admission', guard => guard.receive('GET', 'http://external.invalid/x', {})],
    ['payload', guard => sendMutation(guard, 'POST', versionPath, Buffer.alloc(0))],
    ['upstream-status', guard => guard.complete(sendMutation(guard), 503, mutationResult)],
    ['upstream-result', guard => guard.complete(sendMutation(guard), 201, {})],
    ['retry-payload', guard => { guard.complete(sendMutation(guard), 201, mutationResult); guard.allowRetry(); sendMutation(guard, 'POST', versionPath, Buffer.from('changed')); }],
    ['retry-result', guard => { guard.complete(sendMutation(guard), 201, mutationResult); guard.allowRetry(); guard.complete(sendMutation(guard), 201, {}); }],
  ];
  for (const [code, action] of cases) {
    const guard = makeGuard(); guard.arm({ method: 'POST', path: versionPath });
    assert.throws(() => action(guard), error => error.message.startsWith(`[working-loss:${code}] `));
  }
  assert.throws(() => workingLoss.workingResponseLossGuard('https://127.0.0.1:41001'), /\[working-loss:configuration\]/);
});
