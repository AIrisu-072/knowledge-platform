import { browserRuntime } from './browser-runtime';
import type { RuntimeAdapter } from './contract';
import { createDesktopRuntime, type InvokeFn } from './desktop-runtime';

/**
 * Choose the runtime adapter once at startup. The desktop shell injects its
 * IPC bridge (`app.withGlobalTauri`); this is the only place that looks for it
 * so presentation code never branches on the host.
 */
export function selectRuntime(host: unknown = globalThis): RuntimeAdapter {
  const bridge = (host as { __TAURI__?: { core?: { invoke?: unknown } } } | null)?.__TAURI__?.core?.invoke;
  return typeof bridge === 'function' ? createDesktopRuntime(bridge as InvokeFn) : browserRuntime;
}
