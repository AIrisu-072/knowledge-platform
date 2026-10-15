import { expect, test, type Locator, type Page } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import type { AgentExecutionRequest, AgentExecutionRequested, AgentResult, DecisionRecorded, DraftSaved, EvidenceRegistered, GeneratedArtifact, Returned, SuggestedAction } from '../src/api/generated-work/types.gen';
import type { ContextRuntime } from './context-support';

type AgentChatAction = 'agent-chat-setup' | 'agent-chat-evidence' | 'agent-chat-request' | 'agent-chat-result' | 'agent-chat-visibility' | 'agent-chat-proposal' | 'agent-chat-decision' | 'agent-chat-return' | 'agent-chat-claim' | 'agent-chat-draft' | 'agent-chat-followup' | 'agent-chat-persistence';
/** Closed failure projection marker shared with the existing harness diagnostics. */
export function agentChatAction(action: AgentChatAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  annotations.push({ type: 'organization-stage', description: action });
}
type Source = { sourceRef: { providerId: 'document'; resourceId: string; revisionId: string; versionId: string }; authoritativeLocator: { kind: 'contentItem'; contentItemId: string; representationId: string } };
/** Human evidence through the existing Evidence module, for any synthetic principal. */
export async function registerOwnEvidence(page: Page, taskId: string, source: Source, relevantLocation: string, principal: string): Promise<EvidenceRegistered['evidence']> {
  await page.getByLabel('根拠にする入力文書', { exact: true }).selectOption(source.sourceRef.resourceId);
  await page.getByLabel('原本ファイル', { exact: true }).selectOption(`${source.authoritativeLocator.contentItemId}:${source.authoritativeLocator.representationId}`);
  await page.getByLabel('該当箇所（人間の記載・未検証）', { exact: true }).fill(relevantLocation);
  const pending = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/evidence` && response.request().method() === 'POST');
  await page.getByRole('button', { name: '根拠を登録', exact: true }).click();
  const response = await pending;
  expect(response.status()).toBe(200);
  const result = await response.json() as EvidenceRegistered;
  expect(result.evidence).toMatchObject({ ...source, taskId, relevantLocation, createdBy: principal, origin: 'human', visibility: 'work_item_private' });
  return result.evidence;
}
/** One chat turn through the existing request API; 202 is acceptance, not completion. */
export async function requestAgent(page: Page, taskId: string, module: Locator, purpose: string, evidenceId: string): Promise<{ command: AgentExecutionRequest; result: AgentExecutionRequested }> {
  const input = module.getByLabel('Agentへの依頼目的', { exact: true });
  await expect(input).toBeEnabled();
  await input.fill(purpose);
  await module.getByLabel(`Agentの根拠 ${evidenceId}`, { exact: true }).check();
  const accepted = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/agent-executions` && response.request().method() === 'POST');
  await module.getByRole('button', { name: '合成Agentに依頼', exact: true }).click();
  const response = await accepted;
  expect(response.status()).toBe(202);
  const command = response.request().postDataJSON() as AgentExecutionRequest;
  expect(Object.keys(command).sort()).toEqual(['actingAssignmentId', 'evidenceRevisionRefs', 'expectedAttemptId', 'expectedRevision', 'operationId', 'purpose']);
  return { command, result: await response.json() as AgentExecutionRequested };
}
type Receipt<T> = { operationId: string; command: Record<string, unknown>; result: T };
export type AgentChatTurn = { taskId: string; executionId: string; operationId: string; purpose: string; result: AgentResult; generated: GeneratedArtifact[]; suggested: SuggestedAction[] };
export type AgentChatState = {
  schemaVersion: 1;
  documentId: string;
  office: AgentChatTurn;
  officeDecision: Receipt<DecisionRecorded>;
  returned: Receipt<Returned>;
  sales: AgentChatTurn;
  followup: AgentChatTurn;
  saved: Receipt<DraftSaved>;
};
const path = (context: ContextRuntime) => `${context.contextStatePath}.agent-chat`;
export async function saveAgentChatState(context: ContextRuntime, state: AgentChatState) {
  await writeFile(path(context), `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadAgentChatState(context: ContextRuntime): Promise<AgentChatState> {
  const state = JSON.parse(await readFile(path(context), 'utf8')) as AgentChatState;
  expect(state.schemaVersion).toBe(1);
  expect(state.documentId).toBe(context.documentId);
  return state;
}
