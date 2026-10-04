import { expect, type APIRequestContext } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import type { Claimed, DraftSaved, HandoffSnapshot, Submitted, TaskDetail, TaskPage, WorkSession } from '../src/api/generated-work/types.gen';

export type RuntimeContext = { sales: string; office: string; documentId: string; statePath: string };
export type PersistedState = {
  schemaVersion: 1;
  documentId: string;
  salesTaskId: string;
  officeTaskId: string;
  artifactId: string;
  snapshotId: string;
  text: string;
  save: { operationId: string; result: DraftSaved };
  submit: { operationId: string; result: Submitted };
  claim: { operationId: string; result: Claimed };
  final: {
    salesContext: TaskPage;
    salesQueue: TaskPage;
    officeContext: TaskPage;
    officeQueue: TaskPage;
    salesTask: TaskDetail;
    officeTask: TaskDetail;
    snapshot: HandoffSnapshot;
  };
};

export function readRuntimeContext(): RuntimeContext {
  const path = process.env.KP_ORGANIZATION_RUNTIME_CONTEXT;
  if (!path) throw new Error('Organization runtime context file is required');
  const value: unknown = JSON.parse(readFileSync(path, 'utf8'));
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid Organization context');
  const context = value as RuntimeContext;
  for (const origin of [context.sales, context.office]) {
    if (typeof origin !== 'string') throw new Error('Both fixed-profile origins are required');
    const url = new URL(origin);
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
      throw new Error('Organization acceptance requires explicit loopback HTTP origins');
    }
  }
  if (new URL(context.sales).origin === new URL(context.office).origin) throw new Error('Sales and office must be separate fixed-profile processes');
  if (typeof context.documentId !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(context.documentId)) throw new Error('Seeded Document UUID is required');
  if (typeof context.statePath !== 'string' || !isAbsolute(context.statePath)) throw new Error('Harness-owned absolute statePath is required');
  return { ...context, sales: new URL(context.sales).origin, office: new URL(context.office).origin };
}

export async function get<T>(request: APIRequestContext, origin: string, path: string): Promise<T> {
  const response = await request.get(`${new URL(origin).origin}${path}`);
  expect(response.status(), `GET ${path} returned an unexpected status`).toBe(200);
  return await response.json() as T;
}
export async function assertSessions(request: APIRequestContext, context: RuntimeContext) {
  const sales = await get<WorkSession>(request, context.sales, '/v1/organization/session');
  const office = await get<WorkSession>(request, context.office, '/v1/organization/session');
  expect(sales.principalId).toBe('sales-01');
  expect(office.principalId).toBe('office-01');
  expect(sales.actingAssignmentId).not.toBe(office.actingAssignmentId);
  for (const session of [sales, office]) expect(session.capabilities).toEqual({ nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: false });
  return { sales, office };
}
export async function assertHidden(request: APIRequestContext, origin: string, path: string, code: string, privateText?: string) {
  const response = await request.get(`${new URL(origin).origin}${path}`);
  expect(response.status()).toBe(404);
  expect(response.headers()['cache-control']).toBe('no-store');
  const body = await response.json() as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(['code', 'status', 'title', 'traceId', 'type']);
  expect(body).toMatchObject({ status: 404, code });
  if (privateText) expect(JSON.stringify(body)).not.toContain(privateText);
}
export async function captureFinal(request: APIRequestContext, context: RuntimeContext, salesTaskId: string, officeTaskId: string, snapshotId: string): Promise<PersistedState['final']> {
  return {
    salesContext: await get(request, context.sales, '/v1/organization/tasks?view=context'),
    salesQueue: await get(request, context.sales, '/v1/organization/tasks?view=queue'),
    officeContext: await get(request, context.office, '/v1/organization/tasks?view=context'),
    officeQueue: await get(request, context.office, '/v1/organization/tasks?view=queue'),
    salesTask: await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}`),
    officeTask: await get(request, context.office, `/v1/organization/tasks/${officeTaskId}`),
    snapshot: await get(request, context.sales, `/v1/organization/handoff-snapshots/${snapshotId}`),
  };
}
export async function saveState(context: RuntimeContext, state: PersistedState) {
  // This is a private synthetic restart oracle, not a Playwright attachment or export.
  // Never overwrite a prior journey receipt: each harness run owns a fresh directory.
  await writeFile(context.statePath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadState(context: RuntimeContext): Promise<PersistedState> {
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState;
  expect(state.schemaVersion).toBe(1);
  expect(state.documentId).toBe(context.documentId);
  return state;
}
