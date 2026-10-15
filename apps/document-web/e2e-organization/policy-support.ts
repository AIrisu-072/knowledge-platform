import { expect, test, type APIRequestContext, type Browser, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import type { Assigned, Claimed, Completed, Delegation, DelegationCreated, DelegationRevoked, RoleAssignment, RoleAssignmentCreated, RoleAssignmentRevoked, Submitted, TaskDetail, WorkSession } from '../src/api/generated-work/types.gen';

export const POLICY_PROFILES = { sales: 'sales-01', office: 'office-01', review: 'review-01', approver: 'approver-01', multiRole: 'multi-role-01', delegate: 'delegate-01' } as const;
export type PolicyRole = keyof typeof POLICY_PROFILES;
export type PolicyContext = Record<PolicyRole, string> & { documentId: string; statePath: string };
type PolicyAction = 'policy-setup' | 'policy-draft' | 'policy-role-assignment' | 'policy-reassign-sales' | 'policy-transfer-verify' | 'policy-submit' | 'policy-delegation' | 'policy-concurrent-claim' | 'policy-reassign-office' | 'policy-delegation-revoke' | 'policy-assignment-revoke' | 'policy-complete' | 'policy-persistence';

/** Closed failure projection marker shared with the existing harness diagnostics. */
export function policyAction(action: PolicyAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  annotations.push({ type: 'organization-stage', description: action });
}
/** Six distinct fixed-profile loopback origins; identity is never chosen by the browser. */
export function readPolicyContext(): PolicyContext {
  const path = process.env.KP_ORGANIZATION_RUNTIME_CONTEXT;
  if (!path) throw new Error('Organization policy context file is required');
  const value = JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>;
  const origins = new Set<string>();
  const context = {} as PolicyContext;
  for (const role of Object.keys(POLICY_PROFILES) as PolicyRole[]) {
    const origin = value[role];
    if (typeof origin !== 'string') throw new Error('Every fixed-profile origin is required');
    const url = new URL(origin);
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.pathname !== '/' || url.search || url.hash || url.username || url.password) throw new Error('Policy acceptance requires explicit loopback HTTP origins');
    origins.add(url.origin);
    context[role] = url.origin;
  }
  if (origins.size !== 6) throw new Error('Each profile must be a separate fixed-profile process');
  if (typeof value.documentId !== 'string' || typeof value.statePath !== 'string' || !isAbsolute(value.statePath)) throw new Error('Seeded Document and harness-owned state path are required');
  return { ...context, documentId: value.documentId, statePath: value.statePath };
}
export async function read<T>(request: APIRequestContext, origin: string, path: string): Promise<T> {
  const response = await request.get(`${origin}${path}`);
  expect(response.status(), `GET ${path.split('?')[0]?.replace(/[0-9a-f-]{36}/giu, '{id}')}`).toBe(200);
  expect(response.headers()['cache-control']).toBe('no-store');
  return await response.json() as T;
}
/** Hidden 404 with only the closed problem keys; no private text echoed. */
export async function hidden(request: APIRequestContext, origin: string, path: string, code: string, privateText: string) {
  const response = await request.get(`${origin}${path}`);
  expect(response.status()).toBe(404);
  const body = await response.json() as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(['code', 'status', 'title', 'traceId', 'type']);
  expect(body.code).toBe(code);
  expect(JSON.stringify(body)).not.toContain(privateText);
}
export async function sessions(request: APIRequestContext, context: PolicyContext) {
  const entries = await Promise.all((Object.keys(POLICY_PROFILES) as PolicyRole[]).map(async (role) => [role, await read<WorkSession>(request, context[role], '/v1/organization/session')] as const));
  const result = Object.fromEntries(entries) as Record<PolicyRole, WorkSession>;
  for (const role of Object.keys(POLICY_PROFILES) as PolicyRole[]) expect(result[role].principalId).toBe(POLICY_PROFILES[role]);
  return result;
}
export async function openPage(browser: Browser, origin: string): Promise<Page> {
  const context = await browser.newContext({ baseURL: origin, locale: 'ja-JP', viewport: { width: 1440, height: 900 }, serviceWorkers: 'block' });
  return await context.newPage();
}
/** JST wall-clock input for the delegation end, `minutes` from now. */
export function jstLocal(minutes: number): string {
  const jst = new Date(Date.now() + minutes * 60_000 + 9 * 60 * 60_000);
  return jst.toISOString().slice(0, 16);
}
type Receipt<T> = { operationId: string; command: Record<string, unknown>; result: T };
export type PolicyState = {
  schemaVersion: 1;
  documentId: string;
  salesTaskId: string;
  officeTaskId: string;
  text: string;
  roleAssignment: Receipt<RoleAssignmentCreated>;
  salesAssigned: Receipt<Assigned>;
  submitted: Receipt<Submitted>;
  delegation: Receipt<DelegationCreated>;
  claimWinner: 'multiRole' | 'delegate';
  claim: Receipt<Claimed>;
  officeAssigned: Receipt<Assigned>;
  delegationRevoked: Receipt<DelegationRevoked>;
  assignmentRevoked: Receipt<RoleAssignmentRevoked>;
  completed: Receipt<Completed>;
  final: { assignments: RoleAssignment[]; delegations: Delegation[]; officeTask: TaskDetail; reviewSalesTask: TaskDetail };
};
export async function savePolicyState(context: PolicyContext, state: PolicyState) {
  await writeFile(context.statePath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadPolicyState(context: PolicyContext): Promise<PolicyState> {
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PolicyState;
  expect(state.schemaVersion).toBe(1);
  expect(state.documentId).toBe(context.documentId);
  return state;
}
/** Wait for the single matching write and return its exact request body and receipt. */
export async function capture<T>(page: Page, method: string, path: RegExp, action: () => Promise<void>): Promise<Receipt<T>> {
  const responsePromise = page.waitForResponse((response) => response.request().method() === method && path.test(new URL(response.url()).pathname));
  await action();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const command = response.request().postDataJSON() as Record<string, unknown>;
  return { operationId: command.operationId as string, command, result: await response.json() as T };
}
