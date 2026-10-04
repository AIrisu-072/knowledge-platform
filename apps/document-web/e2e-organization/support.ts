import { test } from '@playwright/test';
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import type { AgentExecutionRequest, AgentExecutionRequested, AgentExecution, AgentResult, Finding, DecisionCommand, DecisionRecorded, EvidenceCommand, EvidenceRegistered, FindingCommand, FindingRegistered, RevisionRef, Claimed, DraftSaved, HandoffSnapshot, ReturnCommand, Returned, ReturnInstruction, Submitted, TaskDetail, TaskPage, WorkSession } from '../src/api/generated-work/types.gen';

import type { DocumentRevisionPage, FileList, PublishedDocumentDetail } from '@knowledge-platform/document-api-client';

export type RuntimeContext = { sales: string; office: string; documentId: string; statePath: string };
export type PersistedState = {
  schemaVersion: 4;
  documentId: string;
  salesTaskId: string;
  officeTaskId: string;
  artifactId: string;
  snapshotId: string;
  text: string;
  save: { operationId: string; result: DraftSaved };
  submit: { operationId: string; result: Submitted };
  claim: { operationId: string; result: Claimed };
  rework: {
    text: string;
    returned: { operationId: string; command: ReturnCommand; result: Returned };
    salesClaim: { operationId: string; result: Claimed };
    save: { operationId: string; result: DraftSaved };
    submit: { operationId: string; result: Submitted };
    officeClaim: { operationId: string; result: Claimed };
  };
  evidence: EvidenceState;
  agents: AgentState;
  final: {
    salesContext: TaskPage;
    salesQueue: TaskPage;
    officeContext: TaskPage;
    officeQueue: TaskPage;
    salesTask: TaskDetail;
    officeTask: TaskDetail;
    snapshot: HandoffSnapshot;
    priorSnapshot: HandoffSnapshot;
    returnInstruction: ReturnInstruction;
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
  for (const session of [sales, office]) expect(session.capabilities).toEqual({ nativeWorkspace: false, agent: true, search: false, fileUpload: false, return: true });
  return { sales, office };
}
export async function assertHidden(request: APIRequestContext, origin: string, path: string, code: string, privateText?: string) {
  currentAction('visibility-verify');
  const response = await request.get(`${new URL(origin).origin}${path}`);
  expect(response.status()).toBe(404);
  expect(response.headers()['cache-control']).toBe('no-store');
  const body = await response.json() as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(['code', 'status', 'title', 'traceId', 'type']);
  expect(body).toMatchObject({ status: 404, code });
  if (privateText) expect(JSON.stringify(body)).not.toContain(privateText);
}
export async function captureFinal(request: APIRequestContext, context: RuntimeContext, salesTaskId: string, officeTaskId: string, snapshotId: string, priorSnapshotId: string, returnInstructionId: string): Promise<PersistedState['final']> {
  return {
    salesContext: await get(request, context.sales, '/v1/organization/tasks?view=context'),
    salesQueue: await get(request, context.sales, '/v1/organization/tasks?view=queue'),
    officeContext: await get(request, context.office, '/v1/organization/tasks?view=context'),
    officeQueue: await get(request, context.office, '/v1/organization/tasks?view=queue'),
    salesTask: await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}`),
    officeTask: await get(request, context.office, `/v1/organization/tasks/${officeTaskId}`),
    snapshot: await get(request, context.sales, `/v1/organization/handoff-snapshots/${snapshotId}`),
    priorSnapshot: await get(request, context.sales, `/v1/organization/handoff-snapshots/${priorSnapshotId}`),
    returnInstruction: await get(request, context.sales, `/v1/organization/return-instructions/${returnInstructionId}`),
  };
}
export async function saveState(context: RuntimeContext, state: PersistedState) {
  // This is a private synthetic restart oracle, not a Playwright attachment or export.
  // Never overwrite a prior journey receipt: each harness run owns a fresh directory.
  await writeFile(context.statePath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadState(context: RuntimeContext): Promise<PersistedState> {
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState;
  expect(state.schemaVersion).toBe(4);
  expect(state.documentId).toBe(context.documentId);
  return state;
}


export type EvidenceReceipt = { operationId: string; command: EvidenceCommand; result: EvidenceRegistered };
export type FindingReceipt = { operationId: string; command: FindingCommand; result: FindingRegistered };
export type DecisionReceipt = { operationId: string; command: DecisionCommand; result: DecisionRecorded };
export type EvidenceState = {
  selected: EvidenceReceipt;
  unselected: EvidenceReceipt;
  finding: FindingReceipt;
  privateFinding: FindingReceipt;
  decisions: DecisionReceipt[];
  privateDecision: DecisionReceipt;
  officeDecision: DecisionReceipt;
  officeReworkDecision: DecisionReceipt;
  rework: { evidence: EvidenceReceipt; finding: FindingReceipt; decision: DecisionReceipt };
};
export function revisionRef(record: RevisionRef): RevisionRef {
  return { id: record.id, revision: record.revision };
}
export async function publishedEvidenceSource(request: APIRequestContext, origin: string, documentId: string) {
  currentAction('source-read');
  const document = await get<PublishedDocumentDetail>(request, origin, `/v1/documents/${documentId}?view=published`);
  currentAction('source-read');
  const revisions = await get<DocumentRevisionPage>(request, origin, `/v1/documents/${documentId}/revisions?pageSize=100`);
  expect(document.displayRevision).not.toBeNull();
  const revision = document.displayRevision!;
  expect(revisions.items).toContainEqual(revision);
  expect(revision.documentVersionId).toBe(document.currentVersionId);
  currentAction('source-read');
  const files = await get<FileList>(request, origin, `/v1/documents/${documentId}/versions/${document.currentVersionId}/files?purpose=published`);
  const authoritative = files.items.filter((file) => file.role === 'AUTHORITATIVE');
  expect(authoritative.length).toBeGreaterThan(0);
  const file = authoritative[0]!;
  return {
    sourceRef: { providerId: 'document' as const, resourceId: documentId, revisionId: revision.revisionId, versionId: document.currentVersionId },
    authoritativeLocator: { kind: 'contentItem' as const, contentItemId: file.contentItemId, representationId: file.representationId },
  };
}
export async function registerEvidence(page: Page, taskId: string, source: Awaited<ReturnType<typeof publishedEvidenceSource>>, relevantLocation: string): Promise<EvidenceReceipt> {
  currentAction('source-document-select');
  await page.getByLabel('根拠にする入力文書', { exact: true }).selectOption(source.sourceRef.resourceId);
  currentAction('source-file-select');
  await page.getByLabel('原本ファイル', { exact: true }).selectOption(`${source.authoritativeLocator.contentItemId}:${source.authoritativeLocator.representationId}`);
  currentAction('evidence-input');
  await page.getByLabel('該当箇所（人間の記載・未検証）', { exact: true }).fill(relevantLocation);
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/evidence` && response.request().method() === 'POST');
  currentAction('evidence-submit');
  await page.getByRole('button', { name: '根拠を登録', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as EvidenceRegistered;
  const command = response.request().postDataJSON() as EvidenceCommand;
  expect(command).toMatchObject({ ...source, relevantLocation, expectedAttemptId: result.task.attemptId });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  expect(result.kind).toBe('evidence_registered');
  expect(result.evidence).toMatchObject({ ...source, relevantLocation, taskId, attemptId: result.task.attemptId, revision: 1, contextId: result.task.contextId, createdBy: 'sales-01', actingAssignmentId: command.actingAssignmentId, origin: 'human', coverage: 'unknown', fragmentOmissionReason: 'not_retained', relevantLocationVerified: false, policyDisposition: 'reference_only', visibility: 'work_item_private' });
  expect(result.evidence).not.toHaveProperty('fragment');
  expect(result.task.state).toBe('active');
  await expect(page.getByLabel(`候補の根拠 ${result.evidence.id}`, { exact: true })).toBeVisible();
  const saved = page.getByRole('region', { name: `根拠 ${result.evidence.id}`, exact: true });
  for (const value of [source.sourceRef.resourceId, source.sourceRef.revisionId, source.sourceRef.versionId, source.authoritativeLocator.contentItemId, source.authoritativeLocator.representationId, relevantLocation, 'unknown', 'not_retained']) await expect(saved).toContainText(value);
  return { operationId: command.operationId, command, result };
}
export async function registerFinding(page: Page, taskId: string, evidence: EvidenceRegistered['evidence'], claim: string): Promise<FindingReceipt> {
  currentAction('finding-input');
  await page.getByLabel('候補の主張', { exact: true }).fill(claim);
  const selected = page.getByRole('checkbox', { name: /^候補の根拠 / });
  for (const checkbox of await selected.all()) await checkbox.uncheck();
  await expect(page.getByRole('button', { name: '候補を登録', exact: true })).toBeDisabled();
  await page.getByLabel(`候補の根拠 ${evidence.id}`, { exact: true }).check();
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/findings` && response.request().method() === 'POST');
  currentAction('finding-submit');
  await page.getByRole('button', { name: '候補を登録', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as FindingRegistered;
  const command = response.request().postDataJSON() as FindingCommand;
  expect(command).toMatchObject({ claim, evidenceRevisionRefs: [revisionRef(evidence)], expectedAttemptId: result.task.attemptId });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  expect(result).toMatchObject({ kind: 'finding_registered', finding: { taskId, attemptId: result.task.attemptId, revision: 1, contextId: result.task.contextId, author: 'sales-01', actingAssignmentId: command.actingAssignmentId, claim, evidenceRevisionRefs: [revisionRef(evidence)], visibility: 'work_item_private' } });
  await expect(page.getByRole('region', { name: `候補 ${result.finding.id}`, exact: true })).toContainText(claim);
  return { operationId: command.operationId, command, result };
}
export async function recordDecision(page: Page, taskId: string, finding: FindingRegistered['finding'], evidence: EvidenceRegistered['evidence'], decision: DecisionCommand['decision'], reason: string, adoptedClaim?: string, cancelFirst = false): Promise<DecisionReceipt> {
  const region = page.getByRole('region', { name: `候補 ${finding.id}`, exact: true });
  currentAction('decision-select');
  await region.getByLabel(`候補の判断 ${finding.id}`, { exact: true }).selectOption(decision);
  if (decision === 'modified') {
    currentAction('decision-input');
    await region.getByLabel('採用文', { exact: true }).fill('');
    await expect(region.getByRole('button', { name: '判断内容を確認', exact: true })).toBeDisabled();
    currentAction('decision-input');
    await region.getByLabel('採用文', { exact: true }).fill(adoptedClaim!);
  }
  currentAction('decision-input');
  await region.getByLabel('判断理由', { exact: true }).fill(reason);
  const dialog = page.getByRole('dialog', { name: '人間判断の確認', exact: true });
  if (cancelFirst) {
    let posts = 0;
    const count = (outgoing: import('@playwright/test').Request) => { if (new URL(outgoing.url()).pathname === `/v1/organization/findings/${finding.id}/decisions` && outgoing.method() === 'POST') posts += 1; };
    page.on('request', count);
    currentAction('decision-preview');
    await region.getByRole('button', { name: '判断内容を確認', exact: true }).click();
    await expect(dialog).toContainText(finding.claim);
    await expect(dialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(region.getByLabel('判断理由', { exact: true })).toHaveValue(reason);
    if (adoptedClaim) await expect(region.getByLabel('採用文', { exact: true })).toHaveValue(adoptedClaim);
    expect(posts).toBe(0);
    page.off('request', count);
  }
  currentAction('decision-preview');
  await region.getByRole('button', { name: '判断内容を確認', exact: true }).click();
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/findings/${finding.id}/decisions` && response.request().method() === 'POST');
  currentAction('decision-confirm');
  await dialog.getByRole('button', { name: '判断を確定', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as DecisionRecorded;
  const command = response.request().postDataJSON() as DecisionCommand;
  expect(command).toMatchObject({ taskId, expectedAttemptId: result.task.attemptId, findingRevision: finding.revision, decision, reason, evidenceRevisionRefs: [revisionRef(evidence)], ...(adoptedClaim ? { adoptedClaim } : {}) });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  if (decision !== 'modified') expect(command).not.toHaveProperty('adoptedClaim');
  expect(result).toMatchObject({ kind: 'decision_recorded', decision: { taskId, attemptId: result.task.attemptId, findingId: finding.id, findingRevision: finding.revision, revision: 1, decision, reason, evidenceRevisionRefs: [revisionRef(evidence)], ...(adoptedClaim ? { adoptedClaim } : {}) } });
  expect(result.task.state).toBe('active');
  await expect(dialog).not.toBeVisible();
  await expect(region).toContainText(reason);
  return { operationId: command.operationId, command, result };
}

export async function assertEvidenceState(request: APIRequestContext, context: RuntimeContext, salesTaskId: string, officeTaskId: string, state: EvidenceState, agents: AgentState) {
  const sharedEvidence = state.selected.result.evidence;
  const sharedFinding = state.finding.result.finding;
  const privateEvidence = state.rework.evidence.result.evidence;
  const privateFinding = state.rework.finding.result.finding;
  const sharedDecisions = state.decisions.map((receipt) => receipt.result.decision);
  expect(await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}/evidence`)).toEqual({ items: [sharedEvidence, privateEvidence], nextCursor: null });
  expect(await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}/findings`)).toEqual({ items: [sharedFinding, privateFinding, agents.sales.finding], nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/tasks/${officeTaskId}/evidence`)).toEqual({ items: [sharedEvidence], nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/tasks/${officeTaskId}/findings`)).toEqual({ items: [sharedFinding, agents.sales.finding, agents.office.finding], nextCursor: null });
  for (const origin of [context.sales, context.office]) {
    expect(await get(request, origin, `/v1/organization/evidence/${sharedEvidence.id}`)).toEqual(sharedEvidence);
    expect(await get(request, origin, `/v1/organization/findings/${sharedFinding.id}`)).toEqual(sharedFinding);
    await assertHidden(request, origin, `/v1/organization/evidence/${state.unselected.result.evidence.id}`, 'EVIDENCE_NOT_FOUND', state.unselected.result.evidence.relevantLocation);
    await assertHidden(request, origin, `/v1/organization/findings/${state.privateFinding.result.finding.id}`, 'FINDING_NOT_FOUND', state.privateFinding.result.finding.claim);
    await assertHidden(request, origin, `/v1/organization/findings/${state.privateFinding.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
  }
  expect(await get(request, context.sales, `/v1/organization/findings/${sharedFinding.id}/decisions`)).toEqual({ items: sharedDecisions, nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.id}/decisions`)).toEqual({ items: [...sharedDecisions, state.officeReworkDecision.result.decision], nextCursor: null });
  expect(await get(request, context.sales, `/v1/organization/evidence/${privateEvidence.id}`)).toEqual(privateEvidence);
  expect(await get(request, context.sales, `/v1/organization/findings/${privateFinding.id}`)).toEqual(privateFinding);
  expect(await get(request, context.sales, `/v1/organization/findings/${privateFinding.id}/decisions`)).toEqual({ items: [state.rework.decision.result.decision], nextCursor: null });
  await assertHidden(request, context.office, `/v1/organization/evidence/${privateEvidence.id}`, 'EVIDENCE_NOT_FOUND', privateEvidence.relevantLocation);
  await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.id}`, 'FINDING_NOT_FOUND', privateFinding.claim);
  await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.id}/decisions`, 'FINDING_NOT_FOUND');
  for (const receipt of [state.selected, state.finding, ...state.decisions, state.rework.evidence, state.rework.finding, state.rework.decision]) {
    expect(await get(request, context.sales, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
    await assertHidden(request, context.office, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND');
  }
  await assertHidden(request, context.sales, `/v1/organization/operations/${state.unselected.operationId}`, 'EVIDENCE_NOT_FOUND');
  for (const receipt of [state.privateFinding, state.privateDecision]) await assertHidden(request, context.sales, `/v1/organization/operations/${receipt.operationId}`, 'FINDING_NOT_FOUND');
  await assertHidden(request, context.office, `/v1/organization/operations/${state.officeDecision.operationId}`, 'FINDING_NOT_FOUND');
  expect(await get(request, context.office, `/v1/organization/operations/${state.officeReworkDecision.operationId}`)).toEqual(state.officeReworkDecision.result);
  await assertHidden(request, context.sales, `/v1/organization/operations/${state.officeReworkDecision.operationId}`, 'WORK_ITEM_NOT_FOUND');
}

// The last action entered, not proof that it completed or that the test is waiting there.
type OrganizationAction =
  | 'journey-setup' | 'office-navigation' | 'sales-navigation' | 'document-navigation' | 'task-navigation'
  | 'draft-save' | 'source-read' | 'evidence-module' | 'source-document-select' | 'source-file-select'
  | 'evidence-input' | 'evidence-submit' | 'finding-input' | 'finding-submit' | 'decision-select'
  | 'decision-input' | 'decision-preview' | 'decision-confirm' | 'visibility-verify' | 'submit-preview'
  | 'submit-selection' | 'submit-confirm' | 'office-claim' | 'return-preview' | 'return-confirm'
  | 'sales-reclaim' | 'resubmit' | 'office-reclaim' | 'final-verify' | 'persistence-verify'
  | 'agent-module' | 'agent-input' | 'agent-request' | 'agent-result' | 'agent-replay';
export function currentAction(action: OrganizationAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) {
    if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  }
  annotations.push({ type: 'organization-stage', description: action });
}


export type AgentReceipt = { operationId: string; command: AgentExecutionRequest; result: AgentExecutionRequested; execution: AgentExecution; output: AgentResult; finding: Finding };
export type AgentState = { sales: AgentReceipt & { decision: DecisionReceipt }; office: AgentReceipt & { decision: DecisionReceipt } };

export async function requestSyntheticFinding(page: Page, request: APIRequestContext, origin: string, taskId: string, evidence: EvidenceRegistered['evidence'], principal: 'sales-01' | 'office-01'): Promise<AgentReceipt> {
  currentAction('agent-module');
  await page.getByRole('button', { name: 'Agent', exact: true }).click();
  const module = page.getByRole('region', { name: '合成Agent', exact: true });
  for (const label of ['固定規則の模擬処理', '原本本文を分析しません', '実LLM・MCP通信は使用しません']) await expect(module).toContainText(label);
  currentAction('agent-input');
  const purpose = '【合成データ】選択した参照から固定規則の候補を作成し、人間が別途判断する。';
  await module.getByLabel('Agentへの依頼目的', { exact: true }).fill(purpose);
  await expect(module.getByRole('button', { name: '合成Agentに依頼', exact: true })).toBeDisabled();
  await module.getByLabel(`Agentの根拠 ${evidence.id}`, { exact: true }).check();
  const accepted = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/agent-executions` && response.request().method() === 'POST');
  currentAction('agent-request');
  await module.getByRole('button', { name: '合成Agentに依頼', exact: true }).click();
  const response = await accepted;
  expect(response.status()).toBe(202);
  const command = response.request().postDataJSON() as AgentExecutionRequest;
  const result = await response.json() as AgentExecutionRequested;
  expect(Object.keys(command).sort()).toEqual(['actingAssignmentId', 'evidenceRevisionRefs', 'expectedAttemptId', 'expectedRevision', 'operationId', 'purpose']);
  expect(command).toMatchObject({ expectedAttemptId: result.task.attemptId, expectedRevision: result.task.revision - 1, purpose, evidenceRevisionRefs: [revisionRef(evidence)] });
  expect(result).toMatchObject({ kind: 'agent_execution_requested', task: { id: taskId, state: 'active', canRequestAgent: false }, execution: { workItemId: taskId, attemptId: result.task.attemptId, contextId: result.task.contextId, requestedBy: principal, requesterResponsibility: command.actingAssignmentId, executedBy: 'organization-synthetic/agent-01', executorInvocationKind: 'agent', status: 'queued', result: null, purpose, evidenceRevisionRefs: [revisionRef(evidence)], providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' }] } });
  currentAction('agent-result');
  const executionRegion = page.getByRole('region', { name: `Agent実行 ${result.execution.id}`, exact: true });
  try {
    await expect(executionRegion).toContainText('実行状態：成功');
  } catch (error) {
    // Failure-only observation of this same authorized execution. Never log its body, identifiers or purpose.
    try {
      const observed = await request.get(`${origin}/v1/organization/agent-executions/${result.execution.id}`, { timeout: 2000, maxRetries: 0, maxRedirects: 0 });
      if (observed.status() === 200) {
        const value: unknown = await observed.json();
        if (value && typeof value === 'object' && !Array.isArray(value)) {
          const { status, failureCode } = value as Record<string, unknown>;
          if (typeof status === 'string' && ['queued', 'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'].includes(status) && (failureCode === null || typeof failureCode === 'string' && ['provider_denied', 'context_stale', 'invalid_output', 'dependency_unavailable', 'interrupted', 'commit_outcome_unknown'].includes(failureCode))) {
            test.info().annotations.push({ type: 'organization-agent-status', description: status }, { type: 'organization-agent-failure-code', description: failureCode ?? 'none' });
          }
        }
      }
    } catch { /* Preserve the original UI failure even when the bounded diagnostic read is unavailable. */ }
    throw error;
  }
  const openCandidate = executionRegion.getByRole('button', { name: '候補を根拠モジュールで確認', exact: true });
  await expect(openCandidate).toBeEnabled();
  const execution = await get<AgentExecution>(request, origin, `/v1/organization/agent-executions/${result.execution.id}`);
  const output = await get<AgentResult>(request, origin, `/v1/organization/agent-executions/${result.execution.id}/result`);
  expect(execution).toMatchObject({ ...result.execution, status: 'succeeded', taskRevision: result.task.revision + 1, startedAt: execution.startedAt, effectiveContextRevision: execution.effectiveContextRevision, endedAt: execution.endedAt, result: output });
  expect(execution.endedAt).not.toBeNull();
  expect(output).toMatchObject({ simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false, evidenceRevisionRefs: [revisionRef(evidence)] });
  expect(output.findingRevisionRefs).toHaveLength(1);
  expect(output.uncertainty.length).toBeGreaterThan(0);
  const finding = await get<Finding>(request, origin, `/v1/organization/findings/${output.findingRevisionRefs[0].id}`);
  expect(finding).toMatchObject({ ...output.findingRevisionRefs[0], taskId, attemptId: result.task.attemptId, contextId: result.task.contextId, author: 'organization-synthetic/agent-01', originExecutionId: execution.id, evidenceRevisionRefs: [revisionRef(evidence)], uncertainty: output.uncertainty, visibility: 'work_item_private' });
  expect(await get(request, origin, `/v1/organization/findings/${finding.id}/decisions`)).toEqual({ items: [], nextCursor: null });
  const beforeReplay = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  expect(beforeReplay).toMatchObject({ state: 'active', revision: execution.taskRevision, agentExecutionIds: [execution.id] });
  if (principal === 'office-01') expect(beforeReplay).toMatchObject({ canEdit: false, workingArtifacts: [] });
  currentAction('agent-replay');
  const replay = await request.post(`${origin}/v1/organization/tasks/${taskId}/agent-executions`, { data: command });
  expect(replay.status()).toBe(202);
  expect(await replay.json()).toEqual(result);
  expect(await get(request, origin, `/v1/organization/operations/${command.operationId}`)).toEqual(result);
  expect(await get(request, origin, `/v1/organization/agent-executions/${execution.id}`)).toEqual(execution);
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(beforeReplay);
  currentAction('agent-result');
  await openCandidate.click();
  const candidate = page.getByRole('region', { name: `候補 ${finding.id}`, exact: true });
  await expect(candidate).toContainText(finding.claim);
  await expect(candidate).toContainText('organization-synthetic/agent-01');
  await expect(candidate).toContainText(`生成元の実行 ${execution.id}`);
  return { operationId: command.operationId, command, result, execution, output, finding };
}

export async function assertAgentState(request: APIRequestContext, context: RuntimeContext, agents: AgentState) {
  for (const [role, receipt] of Object.entries(agents) as ['sales' | 'office', AgentState['sales']][]) {
    const origin = context[role], other = context[role === 'sales' ? 'office' : 'sales'];
    expect(await get(request, origin, `/v1/organization/agent-executions/${receipt.execution.id}`)).toEqual(receipt.execution);
    expect(await get(request, origin, `/v1/organization/agent-executions/${receipt.execution.id}/result`)).toEqual(receipt.output);
    expect(await get(request, origin, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
    expect(await get(request, origin, `/v1/organization/operations/${receipt.decision.operationId}`)).toEqual(receipt.decision.result);
    expect(await get(request, origin, `/v1/organization/findings/${receipt.finding.id}`)).toEqual(receipt.finding);
    expect(await get(request, origin, `/v1/organization/findings/${receipt.finding.id}/decisions`)).toEqual({ items: [receipt.decision.result.decision], nextCursor: null });
    await assertHidden(request, other, `/v1/organization/agent-executions/${receipt.execution.id}`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/agent-executions/${receipt.execution.id}/result`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/operations/${receipt.decision.operationId}`, 'WORK_ITEM_NOT_FOUND');
  }
  expect(await get(request, context.office, `/v1/organization/findings/${agents.sales.finding.id}`)).toEqual(agents.sales.finding);
  expect(await get(request, context.office, `/v1/organization/findings/${agents.sales.finding.id}/decisions`)).toEqual({ items: [agents.sales.decision.result.decision], nextCursor: null });
  await assertHidden(request, context.sales, `/v1/organization/findings/${agents.office.finding.id}`, 'FINDING_NOT_FOUND', agents.office.finding.claim);
  await assertHidden(request, context.sales, `/v1/organization/findings/${agents.office.finding.id}/decisions`, 'FINDING_NOT_FOUND');
}
