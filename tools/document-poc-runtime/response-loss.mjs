// Test-only transparent fault control. No synthetic business response is served.
import { createServer, request as httpRequest } from 'node:http';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { pipeline } from 'node:stream/promises';
import { gunzipSync, inflateSync, brotliDecompressSync } from 'node:zlib';

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

// Fixture-only caps, deliberately far below the production 1 GiB/256 MiB limits.
export const SYNTHETIC_MULTIPART_LIMIT = 256 * 1024;
const SMALL_RESULT_LIMIT = 64 * 1024;
const versionPathPattern = /^\/v1\/documents\/([0-9a-f-]{36})\/versions(?:\/([0-9a-f-]{36}))?$/;

// Fixed test-only markers survive Playwright error serialization; never expose raw details in CI.
const workingLossError = (code, message) => Error(`[working-loss:${code}] ${message}`);

// Pure admission/state logic is shared by the actual proxy and socket-free tests.
export function workingResponseLossGuard(origin) {
  const target = new URL(origin);
  if (target.protocol !== 'http:' || target.hostname !== '127.0.0.1' || !target.port
    || ![target.origin, target.origin + '/'].includes(origin)) throw workingLossError('configuration', 'Requires an owned fixed loopback origin');
  let phase = 'idle', cycles = 0, failure, rawPayload, receipt, publishPath;
  const reject = (message, code = 'upstream-transport') => { if (!failure) { failure = workingLossError(code, message); if (receipt) receipt.unexpected++; } throw failure; };
  const healthy = () => { if (failure) throw failure; };
  const snapshot = () => structuredClone(receipt);
  const guard = {
    arm({ method, path }) {
      healthy(); const match = versionPathPattern.exec(path);
      if (!match || !['POST', 'PUT'].includes(method) || (method === 'PUT') !== Boolean(match[2])
        || (cycles && (cycles !== 1 || phase !== 'recovered' || receipt.method !== 'POST' || method !== 'PUT'
          || path !== `${receipt.path}/${receipt.result.targetVersionId}`))) reject('Unexpected mutation arm', 'admission');
      cycles++; phase = 'armed'; rawPayload = undefined;
      receipt = { method, path, received: 0, dispatched: 0, dropped: 0, unexpected: 0, published: 0 };
    },
    receive(method, url, headers) {
      healthy(); const destination = new URL(url, target);
      if (destination.origin !== target.origin || destination.username || destination.password || destination.hash
        || ![destination.pathname + destination.search, destination.href].includes(url)
        || Object.keys(headers).some(name => /^(authorization|proxy-authorization|cookie|set-cookie|x-api-key|x-auth-token|upgrade)$/i.test(name))) {
        reject('Unexpected origin or credential/upgrade header', 'admission');
      }
      if (['GET', 'HEAD'].includes(method)) return 'read';
      if (method === 'POST' && phase === 'publish-armed' && destination.pathname === publishPath && !destination.search) {
        phase = 'publishing'; return 'publish';
      }
      if (receipt) receipt.received++;
      if (!receipt || method !== receipt.method || destination.pathname + destination.search !== receipt.path
        || !['armed', 'retry-armed'].includes(phase)) reject('Unexpected mutation request or automatic retry',
          receipt && method === receipt.method && destination.pathname + destination.search === receipt.path && phase === 'dropped' ? 'unarmed-retry' : 'admission');
      const kind = phase === 'armed' ? 'initial' : 'retry'; phase = kind; return kind;
    },
    dispatch(kind, body, contentType) {
      healthy();
      if (!body.length || body.length > SYNTHETIC_MULTIPART_LIMIT) reject('Invalid synthetic fixture body size', 'payload');
      if (kind === 'publish') {
        if (phase !== 'publishing' || contentType !== 'application/json') reject('Unexpected publish request', 'admission');
        return;
      }
      if (phase !== kind || !/^multipart\/form-data;\s*boundary=.+$/i.test(contentType ?? '')) reject('Unexpected multipart dispatch', 'payload');
      if (kind === 'initial') {
        rawPayload = Buffer.from(body);
        Object.assign(receipt, { contentType, payloadBytes: body.length, payloadSha256: createHash('sha256').update(body).digest('hex') });
      } else if (!rawPayload.equals(body) || contentType !== receipt.contentType) reject('Retry payload or content-type changed', 'retry-payload');
      if (kind === 'retry') Object.assign(receipt, { bytesEqual: true, contentTypeEqual: true });
      receipt.dispatched++;
    },
    complete(kind, status, result) {
      healthy();
      if (kind === 'publish') {
        if (phase !== 'publishing' || status !== 200) reject('Publish was not successful', 'upstream-status');
        receipt.published++; phase = 'published'; return;
      }
      if (phase !== kind || status !== (receipt.method === 'POST' ? 201 : 200)) reject('Mutation was not successful', 'upstream-status');
      if (kind === 'initial') {
        const match = versionPathPattern.exec(receipt.path);
        if (result?.documentId !== match[1] || !/^[0-9a-f-]{36}$/.test(result?.targetVersionId ?? '')
          || (match[2] && result.targetVersionId !== match[2])) reject('Unexpected upstream mutation result', 'upstream-result');
        Object.assign(receipt, { upstreamStatus: status, result: structuredClone(result) }); receipt.dropped++; phase = 'dropped';
      } else {
        if (!isDeepStrictEqual(result, receipt.result)) reject('Retry upstream result changed', 'retry-result');
        Object.assign(receipt, { retryStatus: status, retryResult: structuredClone(result) }); phase = 'recovered'; rawPayload = undefined;
      }
    },
    allowRetry() { healthy(); if (phase !== 'dropped') reject('Unexpected retry arm', 'admission'); phase = 'retry-armed'; },
    assertRecovered() { healthy(); if (!['recovered', 'published'].includes(phase)) reject('Mutation is not recovered', 'unrecovered'); return snapshot(); },
    allowPublish(path) {
      guard.assertRecovered();
      const documentPath = receipt.path.split('/versions')[0];
      if (phase !== 'recovered' || path !== `${documentPath}/versions/${receipt.result.targetVersionId}:publish`) reject('Unexpected publish target', 'admission');
      publishPath = path; phase = 'publish-armed';
    },
    receipt: snapshot,
    reject,
  };
  return guard;
}


export async function withWorkingResponseLoss(origin, use, { timeoutMs = 50_000 } = {}) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 50_000) throw workingLossError('configuration', 'Invalid response-loss observation window');
  const guard = workingResponseLossGuard(origin), target = new URL(origin);
  const sockets = new Set(), upstreams = new Set(), failed = Promise.withResolvers();
  failed.promise.catch(() => {}); // An error can precede a caller starting its wait.
  let dropped, recovered, timeout;
  const stopObservation = () => clearTimeout(timeout);
  const fail = (error, code = 'upstream-transport') => { stopObservation(); try { guard.reject(error.message, code); } catch (failure) { failed.reject(failure); } };
  const startObservation = () => {
    stopObservation();
    timeout = setTimeout(() => {
      fail(Error('Response-loss observation window expired'), 'observation-window');
      for (const request of upstreams) request.destroy(); for (const socket of sockets) socket.destroy();
    }, timeoutMs);
  };
  const headers = source => {
    const hop = new Set(['connection', 'proxy-connection', 'keep-alive', 'transfer-encoding', 'te', 'trailer', 'upgrade',
      ...(source.connection ?? '').toLowerCase().split(',').map(name => name.trim())]);
    return Object.fromEntries(Object.entries(source).filter(([name]) => !hop.has(name)));
  };
  const bounded = async (stream, limit) => {
    let size = 0; const chunks = [];
    for await (const chunk of stream) { size += chunk.length; if (size > limit) throw Error('Synthetic fixture size limit exceeded'); chunks.push(chunk); }
    return Buffer.concat(chunks);
  };
  const proxy = createServer(async (request, response) => {
    let kind, upstream, failureCode = 'admission';
    try {
      kind = guard.receive(request.method, request.url, request.headers);
      failureCode = 'payload';
      const body = kind === 'read' ? undefined : await bounded(request, SYNTHETIC_MULTIPART_LIMIT);
      if (body) guard.dispatch(kind, body, request.headers['content-type']);
      failureCode = 'upstream-transport';
      const incoming = await new Promise((resolve, reject) => {
        const path = new URL(request.url, target); path.hostname = target.hostname; path.port = target.port;
        upstream = httpRequest(path, { method: request.method, headers: { ...headers(request.headers), host: target.host }, agent: false }, resolve);
        upstreams.add(upstream); upstream.once('close', () => upstreams.delete(upstream)); upstream.on('error', reject);
        request.once('error', error => upstream.destroy(error));
        response.once('close', () => { if (!response.writableFinished) upstream.destroy(); });
        if (body) upstream.end(body); else request.pipe(upstream);
      });
      if (kind === 'read') {
        response.writeHead(incoming.statusCode, headers(incoming.headers)); await pipeline(incoming, response); return;
      }
      failureCode = 'upstream-result';
      const bytes = await bounded(incoming, SMALL_RESULT_LIMIT);
      const encoding = incoming.headers['content-encoding'];
      const decode = { gzip: gunzipSync, deflate: inflateSync, br: brotliDecompressSync }[encoding];
      if (encoding && encoding !== 'identity' && !decode) throw Error('Unexpected result encoding');
      const result = JSON.parse((decode ? decode(bytes, { maxOutputLength: SMALL_RESULT_LIMIT }) : bytes).toString('utf8'));
      guard.complete(kind, incoming.statusCode, result); stopObservation();
      if (kind === 'initial') { response.destroy(); dropped.resolve(guard.receipt()); }
      else {
        response.writeHead(incoming.statusCode, headers(incoming.headers)); response.end(bytes);
        if (kind === 'retry') recovered.resolve(guard.assertRecovered());
      }
    } catch (error) {
      // Read cancellation on navigation is ordinary browser behavior, never a mutation retry.
      if (!(kind === 'read' && (request.aborted || response.destroyed))) fail(error, failureCode);
      upstream?.destroy(); response.destroy();
    }
  });
  proxy.on('connection', socket => { sockets.add(socket); socket.once('close', () => sockets.delete(socket)); });
  for (const event of ['connect', 'upgrade']) proxy.on(event, (_request, socket) => { fail(Error('Unexpected CONNECT/upgrade'), 'admission'); socket.destroy(); });
  try {
    await new Promise((resolve, reject) => { proxy.once('error', reject); proxy.listen(0, '127.0.0.1', resolve); });
    const result = await use({
      origin: `http://127.0.0.1:${proxy.address().port}`,
      arm: operation => { guard.arm(operation); dropped = Promise.withResolvers(); recovered = Promise.withResolvers(); startObservation(); },
      dropped: () => Promise.race([dropped.promise, failed.promise]),
      allowRetry: () => { guard.allowRetry(); startObservation(); },
      assertRecovered: async () => { await Promise.race([recovered.promise, failed.promise]); return guard.assertRecovered(); },
      allowPublish: path => guard.allowPublish(path), receipt: () => guard.receipt(),
    });
    guard.assertRecovered(); return result;
  } finally {
    // The caller closes its dedicated browser context in its callback's finally.
    stopObservation();
    for (const request of upstreams) request.destroy(); for (const socket of sockets) socket.destroy();
    proxy.closeAllConnections(); await new Promise(resolve => proxy.close(() => resolve()));
  }
}
