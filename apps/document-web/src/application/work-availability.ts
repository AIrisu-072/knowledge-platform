import { queryOptions, useQuery, type QueryClient } from '@tanstack/react-query';
import { workApi, WorkApiError } from './work-workspace';

/**
 * Whether the connected server offers the Work API (the Organization client).
 * Presentation only: it picks the landing screen and the primary navigation,
 * never what the user may read or do.
 */
export type WorkAvailability = 'available' | 'unavailable';

/** How long `/` waits for the answer before opening the document screen. */
export const LANDING_PROBE_LIMIT_MS = 3_000;

/** While the answer is unknown, a screen that stays open asks again this often. */
export const AVAILABILITY_RECHECK_MS = 10_000;

// A document-only server has no /v1/organization routes and answers 404. Any
// other failure (offline, 5xx, a desktop shell without a backend, a malformed
// answer) leaves it unknown: the query fails, the screens keep the document-only
// behaviour, and they ask again on the next screen or after a while (a desktop
// app may start before its backend). Only the answer is kept, never the session
// itself (that is not cached; see TaskHomePage).
export const workAvailabilityQuery = queryOptions({
  queryKey: ['work-api-availability'] as const,
  queryFn: async (): Promise<WorkAvailability> => {
    try {
      await workApi.getSession();
      return 'available';
    } catch (error) {
      if (error instanceof WorkApiError && error.status === 404) return 'unavailable';
      throw error;
    }
  },
  staleTime: Infinity,
  gcTime: Infinity,
  retry: false,
});

export function useWorkAvailability(enabled: boolean): WorkAvailability | undefined {
  return useQuery({ ...workAvailabilityQuery, enabled, refetchInterval: (query) => query.state.status === 'error' ? AVAILABILITY_RECHECK_MS : false }).data;
}

/**
 * The screen `/` opens: タスク when the server offers the Work API, otherwise
 * (a document-only server, or no answer in time) the document list as before.
 * A late answer still lands in the cache, where the navigation picks it up.
 */
export async function landingScreen(client: QueryClient, limitMs = LANDING_PROBE_LIMIT_MS): Promise<'tasks' | 'documents'> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const limit = new Promise<undefined>((resolve) => { timer = setTimeout(resolve, limitMs, undefined); });
  try {
    const availability = await Promise.race([client.ensureQueryData(workAvailabilityQuery).catch(() => undefined), limit]);
    return availability === 'available' ? 'tasks' : 'documents';
  } finally {
    clearTimeout(timer);
  }
}
