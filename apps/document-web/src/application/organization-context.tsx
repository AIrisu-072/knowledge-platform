import { createContext, useContext, useEffect, useMemo, useState, type ReactNode, type SetStateAction } from 'react';
import type { WorkSession, WorkOperation } from './work-workspace';
type OrganizationContextValue = { session: WorkSession | null; taskHref: string; setContext: (session: WorkSession, taskHref: string) => void };
type TaskTransient = { draft: string | null; operation: WorkOperation | null; unknown: boolean; notice: string; error: unknown };
const emptyTransient: TaskTransient = { draft: null, operation: null, unknown: false, notice: '', error: null };
const TransientContext = createContext<{ items: Record<string, TaskTransient>; update: (key: string, value: SetStateAction<TaskTransient>) => void }>({ items: {}, update: () => undefined });
const OrganizationContext = createContext<OrganizationContextValue>({ session: null, taskHref: '/tasks', setContext: () => undefined });
export function OrganizationProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Record<string, TaskTransient>>({});
  const hasUnsavedWork = Object.values(items).some((item) => item.draft !== null || item.unknown);
  useEffect(() => {
    if (!hasUnsavedWork) return;
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [hasUnsavedWork]);
  const transient = useMemo(() => ({ items, update: (key: string, value: SetStateAction<TaskTransient>) => setItems((previous) => ({ ...previous, [key]: typeof value === 'function' ? value(previous[key] ?? emptyTransient) : value })) }), [items]);
  const [context, setContext] = useState<{ session: WorkSession | null; taskHref: string }>({ session: null, taskHref: '/tasks' });
  const value = useMemo(() => ({ ...context, setContext: (session: WorkSession, taskHref: string) => setContext((previous) => previous.session === session && previous.taskHref === taskHref ? previous : { session, taskHref }) }), [context]);
  return <OrganizationContext.Provider value={value}><TransientContext.Provider value={transient}>{children}</TransientContext.Provider></OrganizationContext.Provider>;
}
export const useOrganizationContext = () => useContext(OrganizationContext);

/** In-memory, identity-scoped edits and unresolved operation IDs; never an authorization cache. */
export function useTaskTransient(key: string): [TaskTransient, (value: SetStateAction<TaskTransient>) => void] {
  const context = useContext(TransientContext);
  return [context.items[key] ?? emptyTransient, (value) => context.update(key, value)];
}
