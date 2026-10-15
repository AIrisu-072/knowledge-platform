import { createContext, useContext, type ReactNode } from 'react';
import { browserRuntime } from './browser-runtime';
import type { RuntimeAdapter } from './contract';

const RuntimeContext = createContext<RuntimeAdapter>(browserRuntime);

/** Provides the startup-selected runtime adapter; defaults to the browser runtime. */
export function RuntimeProvider({ runtime, children }: { runtime: RuntimeAdapter; children: ReactNode }) {
  return <RuntimeContext.Provider value={runtime}>{children}</RuntimeContext.Provider>;
}

export const useRuntime = () => useContext(RuntimeContext);
