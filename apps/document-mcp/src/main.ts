import { serveStdio } from '@modelcontextprotocol/server/stdio';
import { createServer, loadConfig, createDocumentClient, verifyAgentSession } from './adapter';
async function main(): Promise<void> {
  const client = createDocumentClient(loadConfig(process.env));
  const lifetime = new AbortController();
  process.stdin.once('end', () => lifetime.abort());
  const verified = verifyAgentSession(client, lifetime.signal);
  // Install the SDK transport immediately, so EOF cancels preflight without consuming protocol bytes ourselves.
  const handle = serveStdio(async () => {
    await verified;
    return createServer(client);
  }, { onerror: () => {} });
  try { await verified; }
  catch (error) {
    await handle.close();
    if (!lifetime.signal.aborted) throw error;
  }
}
main().catch(() => {
  process.stderr.write('Document MCP startup failed: configuration, availability or PoC Agent session could not be verified.\n');
  process.exitCode = 1;
});
