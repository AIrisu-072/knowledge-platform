import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import type { Page, Route } from '@playwright/test';

// TEST-ONLY: drives the real broker through the shared JSON wire dispatcher
// (crates/local-workspace-runtime/examples/stdio_bridge.rs). This stands in
// for the desktop shell's IPC; it is not Tauri or WebView2 evidence.
export const bridgeBinary = process.env.KP_LOCAL_BRIDGE_BIN ?? fileURLToPath(new URL('../../../target/debug/examples/stdio_bridge', import.meta.url));

type Reply = { ok: unknown } | { err: unknown };

export class Bridge {
  private child: ChildProcessWithoutNullStreams;
  private sequence = 0;
  private pending = new Map<number, (reply: Reply) => void>();
  readonly calls: Array<{ command: string; request: unknown }> = [];

  constructor(readonly stateRoot: string) {
    this.child = spawn(bridgeBinary, [stateRoot], { stdio: ['pipe', 'pipe', 'pipe'] });
    createInterface({ input: this.child.stdout }).on('line', (line) => {
      const message = JSON.parse(line) as { id: number } & Reply;
      this.pending.get(message.id)?.(message);
      this.pending.delete(message.id);
    });
  }

  private send(message: Record<string, unknown>): Promise<Reply> {
    const id = (this.sequence += 1);
    return new Promise((resolveReply) => {
      this.pending.set(id, resolveReply);
      this.child.stdin.write(`${JSON.stringify({ id, ...message })}\n`);
    });
  }

  call(command: string, request: unknown): Promise<Reply> {
    this.calls.push({ command, request });
    return this.send({ command, request });
  }

  /** Queue the next native-picker answer; `null` means the user cancelled. */
  async pick(path: string | null) {
    await this.send({ control: 'pick', path });
  }

  async stop() {
    this.child.stdin.end();
    if (this.child.exitCode === null) await new Promise((done) => this.child.once('exit', done));
  }

  callsOf(command: string) {
    return this.calls.filter((call) => call.command === command);
  }
}

/**
 * Inject the shell's IPC bridge into the page and forward it to `bridge()`.
 * `intercept` may replace a forwarded reply (for example drop it to simulate
 * a lost response after the broker already acted).
 */
export async function wireDesktop(page: Page, bridge: () => Bridge, intercept?: (command: string, route: Route, reply: Reply) => Promise<boolean>) {
  await page.addInitScript(() => {
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: async (name: string, args: unknown) => {
          const response = await fetch('/__desktop_bridge__/invoke', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name, args }) });
          const body = await response.json() as { ok?: unknown; err?: unknown };
          if ('ok' in body) return body.ok;
          throw body.err;
        },
      },
    };
  });
  await page.route('**/__desktop_bridge__/invoke', async (route) => {
    const { name, args } = JSON.parse(route.request().postData() ?? '{}') as { name: string; args: { command: string; request: unknown } };
    if (name !== 'local_workspace_runtime') {
      await route.fulfill({ json: { err: { code: 'unavailable', reason: 'unknown_command' } } });
      return;
    }
    const reply = await bridge().call(args.command, args.request);
    if (intercept && await intercept(args.command, route, reply)) return;
    await route.fulfill({ json: reply });
  });
  // No Document backend runs in this harness; keep API calls local and explicit.
  await page.route('**/v1/**', (route) => route.fulfill({ status: 503, json: { type: 'about:blank', title: 'Unavailable', status: 503 } }));
}
