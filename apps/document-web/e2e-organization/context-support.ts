import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import type { Assigned, Returned, Submitted, TaskAttention } from '../src/api/generated-work/types.gen';
import { readPolicyContext, type PolicyContext } from './policy-support';

type ContextAction = 'context-setup' | 'context-select' | 'context-claim' | 'context-submit' | 'context-assign' | 'context-acknowledge' | 'context-review' | 'context-return' | 'context-verify' | 'context-persistence';
/** Closed failure projection marker shared with the existing harness diagnostics. */
export function contextAction(action: ContextAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  annotations.push({ type: 'organization-stage', description: action });
}
export type ContextRuntime = PolicyContext & { contextStatePath: string };
/** The same six fixed-profile origins as the policy phases, plus a harness-owned state path. */
export function readContextRuntime(): ContextRuntime {
  const context = readPolicyContext();
  const value = JSON.parse(readFileSync(process.env.KP_ORGANIZATION_RUNTIME_CONTEXT!, 'utf8')) as Record<string, unknown>;
  if (typeof value.contextStatePath !== 'string' || !isAbsolute(value.contextStatePath)) throw new Error('Harness-owned context state path is required');
  return { ...context, contextStatePath: value.contextStatePath };
}
type Receipt<T> = { operationId: string; command: Record<string, unknown>; result: T };
export type ContextState = {
  schemaVersion: 1;
  documentId: string;
  contextB: string;
  contextC: string;
  cOfficeTaskId: string;
  bSalesTaskId: string;
  officeAssigned: Receipt<Assigned>;
  acknowledged: TaskAttention;
  cSubmitted: Receipt<Submitted>;
  bReturned: Receipt<Returned>;
  text: string;
};
export async function saveContextState(context: ContextRuntime, state: ContextState) {
  await writeFile(context.contextStatePath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadContextState(context: ContextRuntime): Promise<ContextState> {
  const state = JSON.parse(await readFile(context.contextStatePath, 'utf8')) as ContextState;
  expect(state.schemaVersion).toBe(1);
  expect(state.documentId).toBe(context.documentId);
  return state;
}
