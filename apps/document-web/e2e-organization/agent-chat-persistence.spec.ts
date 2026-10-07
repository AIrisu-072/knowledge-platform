import { expect, test } from '@playwright/test';
import type { AgentResult, GeneratedArtifact, SuggestedAction, TaskDetail } from '../src/api/generated-work/types.gen';
import { hidden, openPage, read } from './policy-support';
import { readContextRuntime } from './context-support';
import { agentChatAction, loadAgentChatState } from './agent-chat-support';

test.use({ screenshot: 'off', trace: 'off', video: 'off' });

test('6 processの再起動後もAgentの構造化結果・下書き候補・提案と非開示を保持し、Chatの入力は残さない', async ({ browser, request }) => {
  const context = readContextRuntime();
  const state = await loadAgentChatState(context);
  agentChatAction('agent-chat-persistence');
  const owners = [[context.office, state.office], [context.sales, state.sales], [context.sales, state.followup]] as const;
  for (const [origin, turn] of owners) {
    expect(await read<AgentResult>(request, origin, `/v1/organization/agent-executions/${turn.executionId}/result`)).toEqual(turn.result);
    for (const value of turn.generated) expect(await read<GeneratedArtifact>(request, origin, `/v1/organization/generated-artifacts/${value.id}`)).toEqual(value);
    for (const value of turn.suggested) expect(await read<SuggestedAction>(request, origin, `/v1/organization/suggested-actions/${value.id}`)).toEqual(value);
  }
  // Request receipts recover by operation ID, never re-executing.
  expect(await read(request, context.sales, `/v1/organization/operations/${state.sales.operationId}`)).toMatchObject({ kind: 'agent_execution_requested', execution: { id: state.sales.executionId } });
  for (const [owner, turn] of owners) {
    for (const origin of [context.office, context.sales, context.review, context.approver, context.delegate, context.multiRole].filter((value) => value !== owner)) {
      for (const value of turn.generated) await hidden(request, origin, `/v1/organization/generated-artifacts/${value.id}`, 'WORK_ITEM_NOT_FOUND', value.title);
      for (const value of turn.suggested) await hidden(request, origin, `/v1/organization/suggested-actions/${value.id}`, 'WORK_ITEM_NOT_FOUND', value.rationale);
    }
  }
  // The saved draft is the Human's work artifact; the candidate stays a separate record.
  const detail = await read<TaskDetail>(request, context.sales, `/v1/organization/tasks/${state.sales.taskId}`);
  expect(detail.workingArtifacts).toEqual([state.saved.result.artifact]);
  expect(detail.agentExecutionIds).toEqual([state.sales.executionId, state.followup.executionId]);

  const sales = await openPage(browser, context.sales);
  try {
    await sales.goto(`/tasks?taskId=${state.sales.taskId}`);
    await expect(sales.getByLabel('作業中の文案', { exact: true })).toHaveValue(state.saved.result.artifact.value!.text);
    await sales.getByRole('button', { name: 'Agent', exact: true }).click();
    const module = sales.getByRole('region', { name: '合成Agent', exact: true });
    const thread = module.getByRole('list', { name: 'Agentとのやり取り', exact: true });
    const latest = thread.getByRole('region', { name: `Agent実行 ${state.followup.executionId}`, exact: true });
    await expect(latest).toContainText('実行状態：成功');
    await expect(latest).toContainText(state.followup.purpose);
    await expect(latest.getByRole('article', { name: `下書き候補 ${state.followup.generated[0]!.id}`, exact: true })).toContainText(state.followup.generated[0]!.title);
    // Session-local input is gone after reload; the records are server business records.
    await expect(module.getByLabel('Agentへの依頼目的', { exact: true })).toHaveValue('');
    await thread.getByRole('button', { name: '依頼 1 を表示', exact: true }).click();
    await expect(thread.getByRole('region', { name: `Agent実行 ${state.sales.executionId}`, exact: true })).toContainText(state.sales.purpose);
  } finally {
    await sales.context().close();
  }
});
