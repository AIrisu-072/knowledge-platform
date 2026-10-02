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
