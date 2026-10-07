import { useEffect, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { emptyAgentDraft, useTaskTransient } from '../../application/organization-context';
import { useEvidenceRecords } from '../../application/use-evidence-records';
import { toggleReference } from '../../application/evidence-workspace';
import { createOperationId } from '../../application/operation-id';
import { actingFor, executeWorkOperation, isDisclosureDenied, isUnknownOutcome, workApi, WorkApiError, workErrorMessage, type AgentExecution, type RevisionRef, type TaskDetail, type WorkOperation, type WorkResult, type WorkSession } from '../../application/work-workspace';
import { formatDateTime } from '../../view-model/date-time';
import { GeneratedArtifactCard, SourceOutcomes, SuggestedActionItem } from './AgentCandidates';
import styles from '../evidence/EvidenceContextModule.module.css';
import shared from '../../routes/TaskWorkspace.module.css';
import chat from './AgentChat.module.css';

const activeStatus = (status: AgentExecution['status']) => status === 'queued' || status === 'running';
const statusLabel = (status: AgentExecution['status']) => ({ queued: '待機中', running: '実行中', succeeded: '成功', failed: '失敗', cancelled: '取消済み', outcome_unknown: '結果不明' })[status];
export const agentContextKey = (session: WorkSession, task: TaskDetail) => ['organization', session.principalId, session.actingAssignmentId, 'agent-context', task.id, task.attemptId];

/** Persisted executions are read through current authorization, never the historical command receipt. */
export function AgentContextModule({ session, task, applyResult, onDenied, refresh, openEvidence }: { session: WorkSession; task: TaskDetail; applyResult: (result: WorkResult) => void; onDenied: () => void; refresh: () => Promise<void>; openEvidence: () => void }) {
  const client = useQueryClient();
  const [transient, setTransient] = useTaskTransient(`${session.principalId}:${session.actingAssignmentId}:${task.id}:${task.attemptId}`);
  const draft = transient.agent ?? emptyAgentDraft;
  const records = useEvidenceRecords(session, task);
  const active = useRef(true), commandPending = useRef(false), refreshed = useRef<string | null>(null);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const executionId = draft.executionId ?? task.agentExecutionIds.at(-1);
  const key = agentContextKey(session, task);
  const current = useQuery({ queryKey: [...key, executionId, task.revision], enabled: Boolean(executionId), staleTime: 0, gcTime: 0,
    // A lifecycle transition can invalidate a read's authorization snapshot. Re-read only that bounded race; never retry writes or denials.
    retry: (failureCount, error) => failureCount < 2 && error instanceof WorkApiError && error.status === 409 && error.code === 'WORK_CONTEXT_STALE', retryDelay: 250,
    refetchInterval: (query) => query.state.status === 'success' && query.state.data && activeStatus(query.state.data.status) ? 500 : false,
    queryFn: async () => {
      const execution = await workApi.getAgentExecution(executionId!);
      if (execution.workItemId !== task.id || (task.contextId !== null && execution.contextId !== task.contextId) || execution.attemptId !== task.attemptId || execution.requestedBy !== session.principalId || execution.requesterResponsibility !== actingFor(session, task)) throw new WorkApiError(404, 'WORK_ITEM_NOT_FOUND');
      return execution;
    },
  });
  const execution = current.isSuccess ? current.data : undefined;
  const recoveringRequest = draft.recoveryExecutionId === executionId && transient.unknown && transient.operation?.kind === 'agent_execution_requested';
  const result = useQuery({ queryKey: [...key, executionId, task.revision, 'result'], queryFn: () => workApi.getAgentResult(executionId!), enabled: execution?.status === 'succeeded', staleTime: 0, gcTime: 0, retry: false });
  useEffect(() => {
    if (isDisclosureDenied(records.error) || isDisclosureDenied(current.error) || isDisclosureDenied(result.error)) { active.current = false; onDenied(); }
  }, [records.error, current.error, result.error]);
  useEffect(() => {
    if (execution && !activeStatus(execution.status) && (execution.status !== 'succeeded' || result.isSuccess) && refreshed.current !== `${execution.id}:${execution.status}`) { refreshed.current = `${execution.id}:${execution.status}`; void refresh(); }
  }, [execution?.id, execution?.status, result.isSuccess]);
  useEffect(() => {
    if (execution && draft.recoveryExecutionId === execution.id && !activeStatus(execution.status)) {
      setTransient((previous) => ({ ...previous, agent: { ...(previous.agent ?? emptyAgentDraft), recoveryExecutionId: undefined }, ...(previous.operation?.kind === 'agent_execution_requested' ? { unknown: false, operation: null, error: null, notice: 'Agentの現在の実行状態を確認しました。' } : {}) }));
    }
  }, [execution?.id, execution?.status, draft.recoveryExecutionId]);
  const mutation = useMutation({ retry: false, mutationFn: executeWorkOperation,
    onSuccess: (result, operation) => {
      if (!active.current) return;
      if ((result.kind !== 'agent_execution_requested' && result.kind !== 'agent_execution_cancelled') || result.kind !== operation.kind || result.task.id !== task.id || result.task.attemptId !== task.attemptId || result.execution.requestedBy !== session.principalId || result.execution.requesterResponsibility !== actingFor(session, task) || (operation.kind === 'agent_execution_cancelled' && result.execution.id !== operation.executionId)) {
        setTransient((previous) => ({ ...previous, unknown: true, notice: '応答と操作が一致しません。同じ操作IDで結果を確認してください。' })); return;
      }
      applyResult(result);
      setTransient((previous) => ({ ...previous, operation: null, unknown: false, error: null, notice: result.kind === 'agent_execution_requested' ? '合成実行の依頼を記録しました。現在の状態を確認します。' : '取消操作を記録しました。現在の状態を確認します。', agent: { ...(previous.agent ?? emptyAgentDraft), ...(result.kind === 'agent_execution_requested' ? { purpose: '', support: [] } : {}), executionId: result.execution.id, recoveryExecutionId: undefined } }));
      void client.invalidateQueries({ queryKey: key });
    },
    onError: (error) => {
      if (!active.current) return;
      if (isDisclosureDenied(error)) { active.current = false; onDenied(); return; }
      const unknown = isUnknownOutcome(error);
      setTransient((previous) => ({ ...previous, error, unknown, operation: unknown ? previous.operation : null }));
    },
    onSettled: () => { commandPending.current = false; },
  });
  const busy = transient.unknown || mutation.isPending;
  const start = (operation: WorkOperation) => { const recoverableCancel = recoveringRequest && operation.kind === 'agent_execution_cancelled' && operation.executionId === draft.recoveryExecutionId; if ((busy && !recoverableCancel) || commandPending.current) return; commandPending.current = true; setTransient((previous) => ({ ...previous, operation, unknown: true, notice: '', error: null })); mutation.mutate(operation); };
  const command = () => ({ operationId: createOperationId(), expectedRevision: task.revision, expectedAttemptId: task.attemptId, actingAssignmentId: actingFor(session, task) });
  const valid = Boolean(draft.purpose.trim()) && new TextEncoder().encode(draft.purpose).length <= 8192 && draft.support.length >= 1 && draft.support.length <= 16 && records.isSuccess && draft.support.every((ref) => records.data.evidence.some((record) => record.id === ref.id && record.revision === ref.revision));
  const error = records.error || current.error || result.error;
  // Using a draft candidate only fills the unsaved Work draft through the same
  // transient store as the editor; saving remains the normal draft save.
  const textDrafts = task.workingArtifacts.filter((value) => value.schemaId === 'organization.text-draft.v1');
  const savedText = textDrafts[0]?.value?.text ?? '';
  const draftBlocked = !task.canEdit ? 'この工程では作業文案を編集できません。' : textDrafts.length > 1 ? '複数の文案があるため入れられません。' : transient.draft !== null && transient.draft !== savedText ? '保存していない文案の変更があります。保存してから使ってください。' : busy ? '処理中の操作があります。完了後に使ってください。' : null;
  const useDraft = (text: string) => {
    if (!active.current) return;
    setTransient((previous) => previous.draft !== null && previous.draft !== savedText
      ? { ...previous, notice: '保存していない文案の変更があるため、下書き候補は入れていません。' }
      : { ...previous, draft: text, notice: 'Agentの下書き候補を作業中の文案に入れました（未保存）。内容を確認し、「文案を保存」で保存してください。' });
    window.setTimeout(() => document.getElementById('work-draft')?.focus(), 0);
  };
  const reviewFinding = (ref: RevisionRef) => {
    if (!active.current) return;
    setTransient((previous) => ({ ...previous, notice: `候補 ${ref.id} を根拠・判断モジュールで確認し、人間判断を記録してください。提案は実行されていません。` }));
    openEvidence();
  };
  const deny = () => { if (!active.current) return; active.current = false; onDenied(); };
  const candidateScope = [...key, executionId, task.revision];
  // A just-accepted execution is shown before the task detail lists it.
  const turns = executionId && !task.agentExecutionIds.includes(executionId) ? [...task.agentExecutionIds, executionId] : task.agentExecutionIds;
  const focus = (id: string) => setTransient((previous) => ({ ...previous, agent: { ...(previous.agent ?? emptyAgentDraft), executionId: id } }));
  return <section className={styles.module} aria-label="合成Agent"><h2>Agent Chat</h2>
    <p className={shared.notice}>固定規則の模擬処理です。原本本文を分析しません。実LLM・MCP通信は使用しません。</p>
    <p>認可済みの根拠を選んで依頼すると、候補・下書き・提案をサーバーの業務記録として保存します。人間判断・文案の保存・提出は別の操作です。依頼目的と実行履歴は自動共有されません。</p>
    <p className={shared.muted}>表示は依頼したあなたの現在の担当だけです。入力中の依頼はこの画面の中だけで保持し、利用者・担当・タスクが変わると消えます。</p>
    {Boolean(error) && <p role="alert">{workErrorMessage(error)}</p>}
    {current.isError && !isDisclosureDenied(current.error) && <button type="button" disabled={current.isFetching} onClick={() => void current.refetch()}>実行状態を再読込</button>}
    {records.isPending && <p role="status">現在の根拠を確認中…</p>}
    {turns.length > 0 && <ol className={chat.thread} aria-label="Agentとのやり取り">{turns.map((id, index) => <li key={id}>{id !== executionId
      ? <button type="button" disabled={busy} onClick={() => focus(id)}>{`依頼 ${index + 1} を表示`}</button>
      : <>
        {executionId && current.isPending && <p role="status">現在の実行状態を確認中…</p>}
        {execution && !error && <section className={shared.snapshot} aria-label={`Agent実行 ${execution.id}`}>
          <div className={chat.request}><h3>{`依頼 ${index + 1}`}</h3><p className={shared.text}>{execution.purpose}</p><p className={shared.muted}>選択した根拠：{execution.evidenceRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p></div>
          <div className={chat.reply}><h3>実行状態：{statusLabel(execution.status)}</h3><p>{execution.id}</p>
            <p className={shared.muted}>操作の受付記録と現在の実行状態は別です。状態の再読込で再実行は行いません。</p>
            {draft.recoveryExecutionId === execution.id && activeStatus(execution.status) && <p role="status">実行の継続は未確認です。受付記録の回復では再実行しません。現在の状態を確認するか、実行を取り消してください。</p>}
            {execution.status === 'outcome_unknown' && <p role="status">実行の結果は不明です。自動再実行しません。再依頼は新しい実行になります。</p>}
            {execution.status === 'failed' && <p role="alert">実行を完了できませんでした。現在の根拠と担当を確認してください。</p>}
            {execution.status === 'cancelled' && <p>未完了の実行を取り消しました。確定済みの候補を消す操作ではありません。</p>}
            <div className={shared.actions}><button type="button" disabled={current.isFetching} onClick={() => { void current.refetch(); if (execution.status === 'succeeded') void result.refetch(); }}>実行状態を再読込</button>{activeStatus(execution.status) && <button type="button" disabled={busy && !recoveringRequest} onClick={() => start({ kind: 'agent_execution_cancelled', taskId: task.id, executionId: execution.id, input: { ...command(), taskId: task.id } })}>実行を取消</button>}</div>
            <details><summary>実行の担当・範囲</summary><p>依頼者 {execution.requestedBy}<br />担当 {execution.requesterResponsibility}<br />実行者 {execution.executedBy} · {execution.executorInvocationKind}<br />提供側 {execution.providerPrincipalBindings.map((binding) => `${binding.providerId}: ${binding.principalId} (${binding.invocationKind})`).join('、')}<br />タスク {execution.workItemId} · 試行 {execution.attemptId}<br />文脈版 {execution.effectiveContextRevision}</p><time dateTime={execution.startedAt}>{formatDateTime(execution.startedAt)}</time></details>
            {execution.status === 'succeeded' && result.isPending && <p role="status">現在の権限で結果を確認中…</p>}
            {execution.status === 'succeeded' && result.isSuccess && <>
              <p>{result.data.summary}</p>
              <SourceOutcomes result={result.data} />
              <p>不確実な点：{result.data.uncertainty.join('、')}</p>
              {result.data.findingRevisionRefs.length > 0
                ? <><p>保存した候補：{result.data.findingRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p><button type="button" disabled={task.revision < execution.taskRevision} onClick={openEvidence}>候補を根拠モジュールで確認</button></>
                : <p className={shared.muted}>根拠付きの候補はありません。提案は検証済みの候補ではありません。</p>}
              {(result.data.generatedArtifactIds ?? []).map((id) => <GeneratedArtifactCard key={id} id={id} scope={candidateScope} onDenied={deny} draftBlocked={draftBlocked} useDraft={useDraft} />)}
              {(result.data.suggestedActionIds ?? []).length > 0 && <section aria-label="Agentの提案"><h4>提案（実行されません。選ぶと通常の画面を開きます）</h4><ul className={chat.suggestions}>{result.data.suggestedActionIds!.map((id) => <SuggestedActionItem key={id} id={id} scope={candidateScope} onDenied={deny} draftBlocked={draftBlocked} useDraft={useDraft} result={result.data} reviewFinding={reviewFinding} />)}</ul></section>}
            </>}
          </div>
        </section>}
      </>}</li>)}</ol>}
    <fieldset disabled={!task.canRequestAgent || busy || !records.isSuccess || Boolean(execution && activeStatus(execution.status))}><legend>現在のタスクへの依頼</legend>
      <label>Agentへの依頼目的<textarea aria-label="Agentへの依頼目的" value={draft.purpose} onChange={(event) => setTransient((previous) => ({ ...previous, agent: { ...(previous.agent ?? emptyAgentDraft), purpose: event.target.value } }))} /></label>
      <p className={shared.muted}>目的はUTF-8で8192バイト以内。既存の根拠を1〜16件選択してください。選択は閲覧権限を広げません。</p>
      {records.isSuccess && records.data.evidence.map((record) => <label key={record.id}><input type="checkbox" aria-label={`Agentの根拠 ${record.id}`} checked={draft.support.some((ref) => ref.id === record.id && ref.revision === record.revision)} onChange={(event) => setTransient((previous) => ({ ...previous, agent: { ...(previous.agent ?? emptyAgentDraft), support: toggleReference(previous.agent?.support ?? [], record, event.target.checked) } }))} />根拠 {record.id} · 版 {record.revision}<br />{record.relevantLocation}</label>)}
      <button type="button" disabled={!valid} onClick={() => start({ kind: 'agent_execution_requested', taskId: task.id, input: { ...command(), purpose: draft.purpose, evidenceRevisionRefs: draft.support } })}>合成Agentに依頼</button>
    </fieldset>
    {!task.canRequestAgent && <p>現在の担当・タスク状態では新しくAgentに依頼できません。</p>}
  </section>;
}
