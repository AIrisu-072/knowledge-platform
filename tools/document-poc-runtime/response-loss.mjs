// Test-only transparent fault control. No synthetic business response is served.
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';

export async function interruptMutationResponse(origin, path, body, { timeoutMs = 50_000 } = {}) {
  const target = new URL(origin);
  if (target.protocol !== 'http:' || target.hostname !== '127.0.0.1' || target.username || target.password
    || target.pathname !== '/' || target.search || target.hash) throw Error('Response loss requires an owned loopback origin');
  if (!/^\/v1\/documents\/[^/?#]+\/metadata$/.test(path)) throw Error('Only the synthetic metadata mutation is supported');
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 50_000) throw Error('Invalid response-loss observation window');
  const payload = JSON.stringify(body);
  const controller = new AbortController();
  const aborted = new Promise(resolve => controller.signal.addEventListener('abort', () => resolve('aborted'), { once: true }));
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  let calls = 0, upstreamStatus, upstreamError;
  let finish;
  const completed = new Promise(resolve => { finish = () => resolve('completed'); });
  const proxy = createServer(async (request, response) => {
    try {
      if (++calls !== 1 || request.method !== 'PATCH' || request.url !== path) throw Error('Unexpected fault-control request');
      let bytes = ''; for await (const chunk of request) bytes += chunk;
      if (bytes !== payload) throw Error('Mutation payload changed in transport');
      const upstream = await fetch(new URL(path, target), { method: 'PATCH', redirect: 'error', credentials: 'omit',
        headers: { 'content-type': 'application/json' }, body: bytes, signal: controller.signal });
      upstreamStatus = upstream.status;
      // Completion is observed by the fault controller only. The mutating client
      // gets no response headers/body and cannot infer success from the transport.
      await upstream.arrayBuffer();
    } catch (error) { upstreamError = error; }
    finally { response.destroy(); finish(); }
  });
  try {
    await new Promise((resolve, reject) => { proxy.once('error', reject); proxy.listen(0, '127.0.0.1', resolve); });
    let responseLost = false;
    try {
      const response = await fetch(`http://127.0.0.1:${proxy.address().port}${path}`, { method: 'PATCH',
        redirect: 'error', headers: { 'content-type': 'application/json' }, body: payload, signal: controller.signal });
      await response.arrayBuffer();
    } catch (error) {
      if (calls === 0) throw Error('Mutation client failed before proxy dispatch', { cause: error });
      responseLost = true;
    }
    if (await Promise.race([completed, aborted]) === 'aborted') throw Error('Response-loss observation aborted');
    if (upstreamError) throw upstreamError;
    if (!responseLost || calls !== 1 || upstreamStatus === undefined) throw Error('Response loss was not established');
    return { responseLost, upstreamStatus, payloadSha256: createHash('sha256').update(payload).digest('hex') };
  } finally {
    clearTimeout(timeout); controller.abort(); proxy.closeAllConnections();
    await new Promise(resolve => proxy.close(() => resolve()));
  }
}
