import { expect, test, type APIRequestContext, type Locator } from '@playwright/test';
import type { AgentResult, Claimed, DraftSaved, Finding, GeneratedArtifact, Returned, SuggestedAction, TaskDetail } from '../src/api/generated-work/types.gen';
import { capture, hidden, openPage, read } from './policy-support';
import { loadContextState, readContextRuntime } from './context-support';
import { loadFilesState } from './files-support';
import { publishedEvidenceSource, recordDecision } from './support';
import { agentChatAction, registerOwnEvidence, requestAgent, saveAgentChatState, type AgentChatTurn } from './agent-chat-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });

const officePurpose = '【合成データ】住所変更届の該当欄を確認し、確認メモの下書きと次の操作の提案を作る。';
const salesPurpose = '【合成データ】差戻し理由に沿って再確認し、作業文案の下書きを作る。';
const followupPurpose = '【合成データ】追記した文案の根拠をもう一度確認する。';
const returnReason = '【合成データ】住所の記載根拠を再確認してください。';
const addition = '\n【合成データ】担当者が下書きを確認し、追記した。';

/** Waits for the chat turn to finish, then reads its structured records under current rights. */
async function finishedTurn(request: APIRequestContext, origin: string, module: Locator, taskId: string, executionId: string, operationId: string, purpose: string): Promise<AgentChatTurn> {
  const turn = module.getByRole('region', { name: `Agent実行 ${executionId}`, exact: true });
  await expect(turn).toContainText('実行状態：成功');
  await expect(turn).toContainText(purpose);
  const result = await read<AgentResult>(request, origin, `/v1/organization/agent-executions/${executionId}/result`);
  expect(result).toMatchObject({ simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false });
  expect(result.sourceOutcomes?.map((value) => value.outcome)).toEqual(result.evidenceRevisionRefs.map(() => 'referenced'));
  expect(result.findingRevisionRefs).toHaveLength(1);
  expect(result.generatedArtifactIds).toHaveLength(1);
  expect(result.suggestedActionIds).toHaveLength(2);
  const generated = await Promise.all(result.generatedArtifactIds!.map((id) => read<GeneratedArtifact>(request, origin, `/v1/organization/generated-artifacts/${id}`)));
  const suggested = await Promise.all(result.suggestedActionIds!.map((id) => read<SuggestedAction>(request, origin, `/v1/organization/suggested-actions/${id}`)));
  for (const value of generated) {
    expect(value).toMatchObject({ executionId, workItemId: taskId, schemaId: 'organization.text-draft.v1', visibility: 'agent_execution_private', simulated: true, author: 'organization-synthetic/agent-01', sourceRevisionRefs: result.evidenceRevisionRefs });
    // The requester's purpose is never echoed into a candidate.
    expect(value.value.text).not.toContain(purpose);
    await expect(turn.getByRole('article', { name: `下書き候補 ${value.id}`, exact: true })).toContainText(value.title);
  }
  expect(suggested.map((value) => value.action)).toEqual([{ kind: 'review_finding', findingRevisionRef: result.findingRevisionRefs[0] }, { kind: 'use_generated_artifact', generatedArtifactId: generated[0]!.id }]);
  await expect(turn.getByRole('list', { name: '根拠ごとの利用結果', exact: true })).toContainText('参照情報だけを使用');
  return { taskId, executionId, operationId, purpose, result, generated, suggested };
}

test('Agent Chatで構造化結果を確認し、提案から通常の画面で人間判断と文案の保存を行う', async ({ browser, request }) => {
  const context = readContextRuntime();
  const state = await loadContextState(context);
  // Runs after the files phases, on the same database.
  await loadFilesState(context);
  agentChatAction('agent-chat-setup');
  const source = await publishedEvidenceSource(request, context.office, context.documentId);
  const office = await openPage(browser, context.office);
  const sales = await openPage(browser, context.sales);
  try {
    // Queue profile: the office handler of context C asks within the current task.
    agentChatAction('agent-chat-evidence');
    await office.goto(`/tasks?taskId=${state.cOfficeTaskId}`);
    await office.getByRole('button', { name: '根拠', exact: true }).click();
    const officeEvidence = await registerOwnEvidence(office, state.cOfficeTaskId, source, '【合成データ】住所変更届・新住所欄', 'office-01');
    agentChatAction('agent-chat-request');
    await office.getByRole('button', { name: 'Agent', exact: true }).click();
    const officeModule = office.getByRole('region', { name: '合成Agent', exact: true });
    await expect(officeModule).toContainText('Agent Chat');
    for (const label of ['固定規則の模擬処理', '原本本文を分析しません', '実LLM・MCP通信は使用しません']) await expect(officeModule).toContainText(label);
    const officeRequest = await requestAgent(office, state.cOfficeTaskId, officeModule, officePurpose, officeEvidence.id);
    expect(officeRequest.result.execution).toMatchObject({ requestedBy: 'office-01', executedBy: 'organization-synthetic/agent-01', status: 'queued', purpose: officePurpose });
    agentChatAction('agent-chat-result');
    const officeTurn = await finishedTurn(request, context.office, officeModule, state.cOfficeTaskId, officeRequest.result.execution.id, officeRequest.command.operationId, officePurpose);
    const turn = officeModule.getByRole('region', { name: `Agent実行 ${officeTurn.executionId}`, exact: true });
    // The office step has no Work draft: the candidate cannot enter Work here.
    const officeCard = turn.getByRole('article', { name: `下書き候補 ${officeTurn.generated[0]!.id}`, exact: true });
    await expect(officeCard.getByRole('button', { name: '作業文案に入れる', exact: true })).toBeDisabled();
    await expect(officeCard).toContainText('この工程では作業文案を編集できません');

    agentChatAction('agent-chat-visibility');
    for (const origin of [context.sales, context.review, context.approver, context.delegate, context.multiRole]) {
      await hidden(request, origin, `/v1/organization/generated-artifacts/${officeTurn.generated[0]!.id}`, 'WORK_ITEM_NOT_FOUND', officeTurn.generated[0]!.title);
      for (const value of officeTurn.suggested) await hidden(request, origin, `/v1/organization/suggested-actions/${value.id}`, 'WORK_ITEM_NOT_FOUND', value.rationale);
    }

    // Choosing a proposal only opens the normal screen; the Human decides there.
    agentChatAction('agent-chat-proposal');
    const finding = await read<Finding>(request, context.office, `/v1/organization/findings/${officeTurn.result.findingRevisionRefs[0]!.id}`);
    expect(finding).toMatchObject({ author: 'organization-synthetic/agent-01', originExecutionId: officeTurn.executionId });
    await turn.getByRole('listitem', { name: `提案 ${officeTurn.suggested[0]!.id}`, exact: true }).getByRole('button', { name: '提案を開く', exact: true }).click();
    await expect(office.getByRole('region', { name: '根拠・候補・人間判断', exact: true })).toBeVisible();
    await expect(office.getByText(`候補 ${finding.id} を根拠・判断モジュールで確認し、人間判断を記録してください。提案は実行されていません。`, { exact: true })).toBeVisible();
    expect(await read(request, context.office, `/v1/organization/findings/${finding.id}/decisions`)).toEqual({ items: [], nextCursor: null });
    agentChatAction('agent-chat-decision');
    const officeDecision = await recordDecision(office, state.cOfficeTaskId, finding, officeEvidence, 'rejected', '【合成データ】事務が合成候補を確認し、採用しない。');
    expect(officeDecision.result.decision).toMatchObject({ humanPrincipal: 'office-01', findingId: finding.id });

    // The office returns context C; the returned sales attempt starts empty and private.
    agentChatAction('agent-chat-return');
    await office.getByLabel('差戻理由', { exact: true }).fill(returnReason);
    await office.getByRole('button', { name: '差戻内容を確認', exact: true }).click();
    const returned = await capture<Returned>(office, 'POST', new RegExp(`/tasks/${state.cOfficeTaskId}/return$`, 'u'), () => office.getByRole('dialog', { name: '差戻の確認', exact: true }).getByRole('button', { name: '差戻を確定', exact: true }).click());
    const salesTaskId = returned.result.nextTask.id;
    // The completed office attempt keeps its own records.
    expect(await read<AgentResult>(request, context.office, `/v1/organization/agent-executions/${officeTurn.executionId}/result`)).toEqual(officeTurn.result);

    agentChatAction('agent-chat-claim');
    await sales.goto(`/tasks?taskId=${salesTaskId}`);
    await capture<Claimed>(sales, 'POST', new RegExp(`/tasks/${salesTaskId}/claim$`, 'u'), () => sales.getByRole('button', { name: '担当を引き受ける', exact: true }).click());
    await expect(sales.getByLabel('作業中の文案', { exact: true })).toHaveValue('');
    agentChatAction('agent-chat-evidence');
    await sales.getByRole('button', { name: '根拠', exact: true }).click();
    const salesEvidence = await registerOwnEvidence(sales, salesTaskId, source, '【合成データ】住所変更届・旧住所欄', 'sales-01');
    agentChatAction('agent-chat-request');
    await sales.getByRole('button', { name: 'Agent', exact: true }).click();
    const salesModule = sales.getByRole('region', { name: '合成Agent', exact: true });
    // Earlier attempts' Agent records never attach to this attempt's chat.
    await expect(salesModule.getByRole('list', { name: 'Agentとのやり取り', exact: true })).toHaveCount(0);
    const salesRequest = await requestAgent(sales, salesTaskId, salesModule, salesPurpose, salesEvidence.id);
    agentChatAction('agent-chat-result');
    const salesTurn = await finishedTurn(request, context.sales, salesModule, salesTaskId, salesRequest.result.execution.id, salesRequest.command.operationId, salesPurpose);

    // Using the candidate only fills the unsaved draft; the Human saves it normally.
    agentChatAction('agent-chat-draft');
    const candidate = salesTurn.generated[0]!;
    const writes: string[] = [];
    const track = (outgoing: import('@playwright/test').Request) => { if (outgoing.method() !== 'GET' && /\/working-artifacts/u.test(new URL(outgoing.url()).pathname)) writes.push(outgoing.method()); };
    sales.on('request', track);
    await salesModule.getByRole('article', { name: `下書き候補 ${candidate.id}`, exact: true }).getByRole('button', { name: '作業文案に入れる', exact: true }).click();
    const draft = sales.getByLabel('作業中の文案', { exact: true });
    await expect(draft).toHaveValue(candidate.value.text);
    await expect(sales.getByText('Agentの下書き候補を作業中の文案に入れました（未保存）。内容を確認し、「文案を保存」で保存してください。', { exact: true })).toBeVisible();
    sales.off('request', track);
    expect(writes).toEqual([]);
    expect((await read<TaskDetail>(request, context.sales, `/v1/organization/tasks/${salesTaskId}`)).workingArtifacts).toEqual([]);
    await draft.fill(`${candidate.value.text}${addition}`);
    const saved = await capture<DraftSaved>(sales, 'POST', new RegExp(`/tasks/${salesTaskId}/working-artifacts$`, 'u'), () => sales.getByRole('button', { name: '文案を保存', exact: true }).click());
    expect(saved.result.artifact).toMatchObject({ schemaId: 'organization.text-draft.v1', value: { text: `${candidate.value.text}${addition}` }, visibility: 'work_item_private' });
    expect(saved.result.artifact).not.toHaveProperty('derivedFrom');
    expect(JSON.stringify(saved.command)).not.toContain(candidate.id);

    // A follow-up turn joins the same thread; earlier turns stay collapsed.
    agentChatAction('agent-chat-followup');
    const followupRequest = await requestAgent(sales, salesTaskId, salesModule, followupPurpose, salesEvidence.id);
    const thread = salesModule.getByRole('list', { name: 'Agentとのやり取り', exact: true });
    const followup = await finishedTurn(request, context.sales, salesModule, salesTaskId, followupRequest.result.execution.id, followupRequest.command.operationId, followupPurpose);
    await expect(thread.getByRole('button', { name: '依頼 1 を表示', exact: true })).toBeVisible();
    await expect(thread.getByRole('region', { name: `Agent実行 ${salesTurn.executionId}`, exact: true })).toHaveCount(0);
    expect((await read<TaskDetail>(request, context.sales, `/v1/organization/tasks/${salesTaskId}`)).agentExecutionIds).toEqual([salesTurn.executionId, followup.executionId]);
    // A saved draft is not an unsaved change: the new candidate is usable again.
    await expect(thread.getByRole('article', { name: `下書き候補 ${followup.generated[0]!.id}`, exact: true }).getByRole('button', { name: '作業文案に入れる', exact: true })).toBeEnabled();
    for (const origin of [context.office, context.review, context.approver]) {
      await hidden(request, origin, `/v1/organization/generated-artifacts/${candidate.id}`, 'WORK_ITEM_NOT_FOUND', candidate.title);
    }

    await saveAgentChatState(context, { schemaVersion: 1, documentId: context.documentId, office: officeTurn, officeDecision, returned, sales: salesTurn, followup, saved });
  } finally {
    for (const page of [office, sales]) await page.context().close();
  }
});
