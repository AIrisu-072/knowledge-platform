import { createServer } from 'node:http';
import { expect, test } from '@playwright/test';

// No production backend, trace, screenshot, video, or persistent listener.
test.use({ screenshot: 'off', trace: 'off', video: 'off' });
test('charset-less Japanese text: raw fetch bytes are the oracle; DevTools response.body can transcode', async ({ page }) => {
  const original = Buffer.from('【合成データ】公開操作の検証\n第1条 初版の内容です。\n');
  const server = createServer((_request, response) => {
    response.writeHead(200, { 'content-type': 'text/plain', 'content-length': String(original.length) });
    response.end(original);
  });
  try {
    await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
    const address = server.address(); if (!address || typeof address === 'string') throw Error('Expected owned loopback listener');
    const response = await page.goto(`http://127.0.0.1:${address.port}`);
    expect(response!.status()).toBe(200);
    expect(response!.headers()['content-length']).toBe(String(original.length));
    const devtoolsBytes = await response!.body();
    expect(devtoolsBytes.equals(original)).toBe(false);
    const raw = Buffer.from(await page.evaluate(async () => Array.from(new Uint8Array(await (await fetch(location.href)).arrayBuffer()))));
    expect(raw.byteLength).toBe(original.length); expect(raw.equals(original)).toBe(true);
    const utf8Text = new TextDecoder('utf-8', { fatal: true }).decode(raw);
    expect(Buffer.from(utf8Text, 'utf8').equals(original)).toBe(true);
  } finally {
    server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve()));
  }
});
