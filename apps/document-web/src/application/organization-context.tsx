import { createContext, useContext, useEffect, useMemo, useState, useCallback, type ReactNode, type SetStateAction } from 'react';
import type { WorkSession, WorkOperation, RevisionRef, SelectedHandoff, HumanDecision } from './work-workspace';
export type EvidenceDraft = { documentId: string; fileKey: string; relevantLocation: string; claim: string; support: RevisionRef[]; decisions: Record<string, { decision: HumanDecision['decision']; adoptedClaim: string; reason: string }> };
export const emptyEvidenceDraft: EvidenceDraft = { documentId: '', fileKey: '', relevantLocation: '', claim: '', support: [], decisions: {} };
export const emptySelection: SelectedHandoff = { evidenceRevisionRefs: [], findingRevisionRefs: [], decisionRevisionRefs: [] };
type OrganizationContextValue = { session: WorkSession | null; taskHref: string; setContext: (session: WorkSession, taskHref: string) => void };
type TaskTransient = { draft: string | null; reason: string | null; operation: WorkOperation | null; unknown: boolean; notice: string; error: unknown; evidence?: EvidenceDraft; sharing?: SelectedHandoff };
const emptyTransient: TaskTransient = { draft: null, reason: null, operation: null, unknown: false, notice: '', error: null };
const TransientContext = createContext<{ items: Record<string, TaskTransient>; update: (key: string, value: SetStateAction<TaskTransient>) => void; clear: (prefix: string, except?: string) => void }>({ items: {}, update: () => undefined, clear: () => undefined });
const OrganizationContext = createContext<OrganizationContextValue>({ session: null, taskHref: '/tasks', setContext: () => undefined });
export function OrganizationProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Record<string, TaskTransient>>({});
  const hasUnsavedWork = Object.values(items).some((item) => item.draft !== null || item.reason !== null || item.unknown || Boolean(item.evidence?.relevantLocation || item.evidence?.claim || Object.values(item.evidence?.decisions ?? {}).some((entry) => entry.adoptedClaim || entry.reason)));
  useEffect(() => {
    if (!hasUnsavedWork) return;
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [hasUnsavedWork]);
  const clear = useCallback((prefix: string, except?: string) => setItems((previous) => Object.fromEntries(Object.entries(previous).filter(([key]) => !key.startsWith(prefix) || key === except))), []);
  const transient = useMemo(() => ({ items, clear, update: (key: string, value: SetStateAction<TaskTransient>) => setItems((previous) => ({ ...previous, [key]: typeof value === 'function' ? value(previous[key] ?? emptyTransient) : value })) }), [items, clear]);
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

export const useClearTaskTransients = () => useContext(TransientContext).clear;
