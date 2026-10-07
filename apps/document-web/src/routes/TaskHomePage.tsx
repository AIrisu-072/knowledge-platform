import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useRouterState, useSearch } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { AppShell } from '../components/app-shell/AppShell';
import { useOrganizationContext, useTaskTransient, useClearTaskTransients, useClearAgentTransients, emptySelection, emptyAgentDraft } from '../application/organization-context';
import { DocumentContextModule } from '../components/document/DocumentContextModule';
import { AgentContextModule } from '../components/agent/AgentContextModule';
import { EvidenceContextModule, decisionLabel } from '../components/evidence/EvidenceContextModule';
import { useEvidenceRecords, evidenceRecordsKey } from '../application/use-evidence-records';
import { selectedHandoffIsClosed, toggleReference } from '../application/evidence-workspace';
import { createOperationId } from '../application/operation-id';
import { workApi, actingFor, executeWorkOperation, isOperationNotFound, validateTaskSearch, workErrorMessage, isDisclosureDenied, isUnknownOutcome, taskStateLabel, type TaskSearch, type TaskSummary, type TaskDetail, type WorkSession, type WorkResult, type WorkCommand, type WorkOperation, type HandoffSnapshot, type ReturnInstruction } from '../application/work-workspace';
import { formatDateTime } from '../view-model/date-time';
import { TaskAssignmentPanel, TaskAssignmentSummary } from '../components/organization/TaskAssignmentPanel';
import { actingLabel, responsibilityLabel } from '../application/organization-policy';
import { AttentionBadges, ContextCollection, ContextOverview, QueueCollection } from '../components/organization/WorkContextPanels';
import styles from './TaskWorkspace.module.css';

const sessionKey = ['organization-session'];
const actorKey = (session: WorkSession) => ['organization', session.principalId, session.actingAssignmentId];
const taskKey = (session: WorkSession, task: TaskSummary) => [...actorKey(session), 'task', task.id, task.attemptId];
const attemptKey = (task: TaskSummary) => `${task.id}:${task.attemptId}`;
const matchesCurrent = (current: TaskSummary, incoming: TaskSummary) => current.id === incoming.id && current.attemptId === incoming.attemptId && incoming.revision >= current.revision;

export function TaskHomePage() {
  const search = useSearch({ from: '/tasks' }) as TaskSearch;
  const navigate = useNavigate({ from: '/tasks' });
  const currentHref = useRouterState({ select: (state) => state.location.href });
  const organization = useOrganizationContext();
  const clearTransients = useClearTaskTransients();
  const clearAgentTransients = useClearAgentTransients();
  const client = useQueryClient();
  const [blockedId, setBlockedId] = useState<string | null>(null);
  const session = useQuery({ queryKey: sessionKey, queryFn: workApi.getSession, staleTime: 0, gcTime: 0, retry: false });
  const sessionData = session.isSuccess ? session.data : undefined;
  useEffect(() => { if (sessionData && (currentHref === '/tasks' || currentHref.startsWith('/tasks?'))) organization.setContext(sessionData, currentHref); }, [sessionData, currentHref, organization.setContext]);
  // A selected responsibility only narrows the projection; an unknown one is ignored, never sent as identity.
  const scope = search.acting && sessionData?.responsibilities?.some((value) => value.id === search.acting) ? search.acting : undefined;
  // Presentation default only: the selected (or session) responsibility's WorkViewProfile.
  const profiles = useQuery({ queryKey: ['organization-work-view-profiles'], queryFn: workApi.listWorkViewProfiles, enabled: Boolean(sessionData) && !search.view, staleTime: 0, gcTime: 0, retry: false });
  const activeResponsibility = sessionData?.responsibilities?.find((value) => value.id === (scope ?? sessionData.actingAssignmentId));
  const profile = profiles.isSuccess ? profiles.data.items.find((value) => value.id === activeResponsibility?.workViewProfileId) : undefined;
  const profileSettled = Boolean(search.view) || !activeResponsibility || profiles.isSuccess || profiles.isError;
  const view = search.view ?? profile?.archetype ?? 'context';
  const tasks = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'tasks', view, ...(scope ? [scope] : [])], queryFn: () => workApi.listTasks(view, scope), enabled: Boolean(sessionData) && profileSettled, placeholderData: (previous, query) => query?.queryKey[1] === sessionData?.principalId && query?.queryKey[2] === sessionData?.actingAssignmentId ? previous : undefined, staleTime: 0, gcTime: 0, retry: false });
  const contexts = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'contexts', ...(scope ? [scope] : [])], queryFn: () => workApi.listWorkContexts(scope), enabled: Boolean(sessionData) && profileSettled && view === 'context', staleTime: 0, gcTime: 0, retry: false });
  const selectedContext = !search.taskId && search.contextId && contexts.isSuccess ? contexts.data.items.find((value) => value.id === search.contextId) : undefined;
  useEffect(() => {
    if (isDisclosureDenied(session.error) || isDisclosureDenied(tasks.error)) {
      clearTransients('');
      client.removeQueries({ queryKey: ['organization'], predicate: (query) => query.queryKey[3] !== 'tasks' });
    }
  }, [session.error, tasks.error, clearTransients, client]);
  const selected = tasks.isSuccess ? tasks.data.items.find((item) => item.id === search.taskId) : undefined;
  // Private detail is requested only for the actor's own assignment; a manager's
  // assignment view and an eligible-only queue row never fetch it.
  const managedOnly = Boolean(selected && selected.canAssign && !selected.canClaim && selected.assignment?.principalId !== sessionData?.principalId);
  // A manager who may also claim, or who is the current assignee, keeps both paths.
  const alsoManaged = Boolean(selected && selected.canAssign && !managedOnly);
  const readable = Boolean(selected && !selected.canClaim && !managedOnly);
  const detail = useQuery({ queryKey: sessionData && selected ? taskKey(sessionData, selected) : ['organization-no-task'], queryFn: () => workApi.getTask(selected!.id), enabled: Boolean(sessionData && selected && readable), staleTime: 0, gcTime: 0, retry: false });
  const detailData = sessionData && selected && blockedId !== attemptKey(selected) && detail.isSuccess && matchesCurrent(selected, detail.data) && readable ? detail.data : undefined;
  const snapshot = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'snapshot', detailData?.handoffSnapshotId], queryFn: () => workApi.getSnapshot(detailData!.handoffSnapshotId!), enabled: Boolean(detailData?.handoffSnapshotId), staleTime: 0, gcTime: 0, retry: false });
  const instruction = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'return-instruction', detailData?.returnInstructionId], queryFn: () => workApi.getReturnInstruction(detailData!.returnInstructionId!), enabled: Boolean(detailData?.returnInstructionId), staleTime: 0, gcTime: 0, retry: false });
  const priorSnapshotId = instruction.isSuccess && detailData?.returnInstructionId === instruction.data.id ? instruction.data.previousSubmissionId : undefined;
  const priorSnapshot = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'snapshot', priorSnapshotId], queryFn: () => workApi.getSnapshot(priorSnapshotId!), enabled: Boolean(priorSnapshotId && priorSnapshotId !== detailData?.handoffSnapshotId), staleTime: 0, gcTime: 0, retry: false });
  useEffect(() => { if (sessionData && selected) clearTransients(`${sessionData.principalId}:${sessionData.actingAssignmentId}:${selected.id}:`, `${sessionData.principalId}:${sessionData.actingAssignmentId}:${selected.id}:${selected.attemptId}`); }, [sessionData?.principalId, sessionData?.actingAssignmentId, selected?.id, selected?.attemptId, clearTransients]);
  useEffect(() => {
    const current = sessionData && selected ? `${sessionData.principalId}:${sessionData.actingAssignmentId}:${selected.id}:${selected.attemptId}` : undefined;
    clearAgentTransients(current);
    client.removeQueries({ queryKey: ['organization'], predicate: (query) => query.queryKey[3] === 'agent-context' && (query.queryKey[1] !== sessionData?.principalId || query.queryKey[2] !== sessionData?.actingAssignmentId || query.queryKey[4] !== selected?.id || query.queryKey[5] !== selected?.attemptId) });
  }, [sessionData?.principalId, sessionData?.actingAssignmentId, selected?.id, selected?.attemptId, clearAgentTransients, client]);
  const [module, setModule] = useState('document');
  useEffect(() => { setModule(profile?.initialModule ?? 'document'); setBlockedId(null); }, [search.taskId, sessionData?.principalId, sessionData?.actingAssignmentId, profile?.initialModule]);
  const [attentionNotice, setAttentionNotice] = useState('');
  useEffect(() => setAttentionNotice(''), [search.taskId]);
  const acknowledge = useMutation({ mutationFn: ({ taskId, period }: { taskId: string; period: string }) => workApi.markAttentionSeen(taskId, period), onSuccess: () => { setAttentionNotice('新しい割当を確認済みにしました。作業は完了していません。'); void tasks.refetch(); if (view === 'context') void contexts.refetch(); } });
  const updateSearch = (patch: Partial<TaskSearch>) => { void navigate({ search: (previous) => ({ ...previous, ...patch }) }); };
  const refresh = async () => { if (sessionData && selected) await client.invalidateQueries({ queryKey: evidenceRecordsKey(sessionData, selected) }); await tasks.refetch(); if (selected && !selected.canClaim) await detail.refetch(); if (detailData?.handoffSnapshotId) await snapshot.refetch(); if (detailData?.returnInstructionId) await instruction.refetch(); if (priorSnapshotId && priorSnapshotId !== detailData?.handoffSnapshotId) await priorSnapshot.refetch(); };
  function denyDisclosure() {
    if (!selected || !sessionData) return;
    setBlockedId(attemptKey(selected));
    clearTransients(`${sessionData.principalId}:${sessionData.actingAssignmentId}:${selected.id}:`);
    client.removeQueries({ queryKey: [...actorKey(sessionData), 'evidence-context', selected.id] });
    client.removeQueries({ queryKey: [...actorKey(sessionData), 'agent-context', selected.id] });
    client.removeQueries({ queryKey: taskKey(sessionData, selected) });
    client.removeQueries({ queryKey: [...actorKey(sessionData), 'snapshot'] });
    client.removeQueries({ queryKey: [...actorKey(sessionData), 'return-instruction'] });
  }
  useEffect(() => { if (isDisclosureDenied(detail.error) || isDisclosureDenied(snapshot.error) || isDisclosureDenied(instruction.error) || isDisclosureDenied(priorSnapshot.error)) denyDisclosure(); }, [detail.error, snapshot.error, instruction.error, priorSnapshot.error]);
  function applyResult(result: WorkResult) {
    if (!sessionData || result.task.id !== selected?.id || result.task.attemptId !== selected.attemptId || result.task.revision < selected.revision || blockedId === attemptKey(selected)) return;
    client.setQueriesData<{ items: TaskSummary[]; nextCursor: null }>({ queryKey: [...actorKey(sessionData), 'tasks'] }, (previous) => previous ? { ...previous, items: previous.items.map((item) => matchesCurrent(item, result.task) ? result.task : item) } : previous);
    if (result.kind !== 'claimed') client.setQueryData<TaskDetail>(taskKey(sessionData, result.task), (previous) => previous && matchesCurrent(previous, result.task) ? { ...previous, ...result.task, workingArtifacts: result.kind === 'draft_saved' ? [result.artifact] : previous.workingArtifacts, ...((result.kind === 'agent_execution_requested' || result.kind === 'agent_execution_cancelled') ? { agentExecutionIds: Array.from(new Set([...previous.agentExecutionIds, result.execution.id])) } : {}) } : previous);
    if (['evidence_registered', 'finding_registered', 'decision_recorded'].includes(result.kind)) void client.invalidateQueries({ queryKey: evidenceRecordsKey(sessionData, result.task) });
    if (['completed', 'held', 'resumed'].includes(result.kind)) void client.invalidateQueries({ queryKey: taskKey(sessionData, result.task) });
    if (result.kind === 'returned') client.setQueryData([...actorKey(sessionData), 'return-instruction', result.returnInstruction.id], result.returnInstruction);
    if (result.kind === 'submitted') client.setQueryData([...actorKey(sessionData), 'snapshot', result.snapshot.id], result.snapshot);
  }
  function applyAssignment(result: WorkResult) {
    if (!sessionData || result.kind !== 'assigned') return;
    // The new responsibility period changes who may read; re-evaluate the projection.
    client.removeQueries({ queryKey: taskKey(sessionData, result.task) });
    void client.invalidateQueries({ queryKey: [...actorKey(sessionData), 'tasks'] });
  }
  const hint = (item: TaskSummary) => item.canClaim ? '担当を引き受けると詳細を表示' : item.canAssign && item.assignment?.principalId !== sessionData?.principalId ? (item.assignment ? (item.assignment.responsibilityEffective ? `担当 ${item.assignment.principalId}` : `担当の責任が終了 · ${item.assignment.principalId}`) : '未割当') : item.contextTitle ?? item.stepLabel;
  const collection = <aside className={styles.collection} aria-label="タスク一覧">
    <p className={styles.muted}>同じタスクの2つの表示{profile ? `（既定：${profile.label}）` : ''}</p>
    <button type="button" aria-pressed={view === 'context'} onClick={() => updateSearch({ view: 'context', workTypeId: undefined })}>営業型・文脈</button>
    <button type="button" aria-pressed={view === 'queue'} onClick={() => updateSearch({ view: 'queue', contextId: undefined })}>事務型・キュー</button>
    <h2>{view === 'context' ? '文脈ごとのタスク' : '担当可能なキュー'}</h2>
    <p className={styles.muted}>現在の担当で閲覧できる範囲</p>
    {(tasks.isFetching || !profileSettled) && <p role="status">一覧を読み込み中…</p>}
    {tasks.isError && <p role="alert">{workErrorMessage(tasks.error)}</p>}
    {tasks.isSuccess && tasks.data.items.length === 0 && <p>閲覧できるタスクはありません</p>}
    {tasks.isSuccess && sessionData && (view === 'context'
      ? <ContextCollection contexts={contexts.isSuccess ? contexts.data.items : undefined} contextError={contexts.error} tasks={tasks.data.items} selectedContext={search.contextId} selectedTask={search.taskId} hint={hint} onContext={(id) => updateSearch({ contextId: id, taskId: undefined })} onTask={(id) => updateSearch({ taskId: id })} />
      : <QueueCollection tasks={tasks.data.items} session={sessionData} workTypeId={search.workTypeId} selectedTask={search.taskId} hint={hint} onWorkType={(id) => updateSearch({ workTypeId: id })} onTask={(id) => updateSearch({ taskId: id })} />)}
    <div className={styles.actions}><button type="button" onClick={() => void refresh()}>再読込</button></div>
  </aside>;
  const newlyAssigned = selected?.attention.find((value) => value.kind === 'newly_assigned' && value.sourceId);
  const contextPanel = <>
    <div className={styles.moduleButtons} aria-label="文脈モジュール">
      {[['document', '文書・比較'], ['history', '履歴'], ['evidence', '根拠'], ['agent', 'Agent'], ['search', '検索'], ['resources', 'Workspace'], ['return', '差戻']].map(([id, label]) => <button key={id} type="button" aria-pressed={module === id} onClick={() => setModule(id!)}>{label}</button>)}
    </div>
    {!detailData ? <p>担当を引き受けたタスクの情報を表示します</p> : module === 'document' ? <DocumentContextModule key={`${sessionData!.principalId}:${sessionData!.actingAssignmentId}:${detailData.id}:${detailData.attemptId}`} session={sessionData!} task={detailData} /> : module === 'agent' ? <AgentContextModule key={`${sessionData!.principalId}:${sessionData!.actingAssignmentId}:${detailData.id}:${detailData.attemptId}`} session={sessionData!} task={detailData} applyResult={applyResult} onDenied={denyDisclosure} refresh={refresh} openEvidence={() => setModule('evidence')} /> : module === 'evidence' ? <EvidenceContextModule key={`${sessionData!.principalId}:${sessionData!.actingAssignmentId}:${detailData.id}:${detailData.attemptId}`} session={sessionData!} task={detailData} applyResult={applyResult} onDenied={denyDisclosure} /> : module === 'history' ? <><h2>業務履歴</h2>{detailData.history.length ? <ul>{detailData.history.map((entry, index) => <li key={`${entry.occurredAt}-${index}`}><span>{historyLabel(entry.kind)}</span><br /><time dateTime={entry.occurredAt}>{formatDateTime(entry.occurredAt)}</time></li>)}</ul> : <p>記録された履歴はありません</p>}<p className={styles.muted}>この履歴は監査基盤の資格取得を示すものではありません。</p></> : module === 'return' ? <><h2>差戻</h2><p>{detailData.returnInstructionId ? '確定した差戻指示と過去の提出内容を主作業に表示します。差戻理由は変更できません。' : detailData.canReturn ? '受領内容と差戻理由を確認して、主作業から差戻してください。' : 'この試行に差戻の記録はありません。'}</p></> : <><h2>{module === 'agent' ? 'Agent' : module === 'search' ? '検索' : module === 'evidence' ? '根拠・判断' : 'Workspace'}</h2><p>このブラウザーPoCでは未実装です</p></>}
    <div className={styles.notice}><strong>ブラウザーPoC</strong><p>ブラウザーではネイティブWorkspaceを利用できません</p><p>Search / ファイル添付は未実装です</p></div>
  </>;
  return <AppShell activeNavigation="tasks" mainLabel="タスクワークスペース" headerContext={<span className={styles.identity}>{sessionData ? <><strong>{sessionData.displayName}</strong> · {sessionData.principalId}<br />担当: {actingLabel(sessionData, sessionData.actingAssignmentId)} · 起動時固定の模擬ユーザー
    {sessionData.responsibilities === null ? <><br /><span role="alert">現在の担当・委任を確認できません。操作はサーバーで拒否されます。</span></> : sessionData.responsibilities && sessionData.responsibilities.length > 1 ? <><br /><label>表示する担当 <select value={scope ?? ''} onChange={(event) => updateSearch({ acting: event.target.value || undefined, taskId: undefined })}><option value="">すべての担当</option>{sessionData.responsibilities.map((value) => <option key={value.id} value={value.id}>{responsibilityLabel(value)}</option>)}</select></label></> : sessionData.responsibilities?.length === 0 ? <><br />現在有効な担当はありません</> : null}
    {sessionData.responsibilities !== undefined && <><br /><Link to="/organization/responsibilities">担当・委任を確認</Link></>}</> : 'タスク'} </span>} contextPanel={contextPanel}>
    <div className={styles.layout}>{collection}<section className={styles.work} aria-label="主作業">
      {session.isPending && <p role="status">利用者を確認中…</p>}
      {session.isError && <><h1>タスクを開けません</h1><p role="alert">{workErrorMessage(session.error)}</p></>}
      {sessionData && !search.taskId && selectedContext && tasks.isSuccess && <ContextOverview session={sessionData} context={selectedContext} tasks={tasks.data.items} onTask={(id) => updateSearch({ taskId: id })} />}
      {sessionData && !search.taskId && search.contextId && contexts.isSuccess && !selectedContext && <><h1>選択中の文脈を利用できません</h1><p>対象が一覧にありません。別の文脈を自動選択していません。</p></>}
      {sessionData && !search.taskId && !search.contextId && <><h1>タスク</h1><p>一覧から作業する{view === 'context' ? '文脈またはタスク' : 'タスク'}を選択してください</p></>}
      {sessionData && search.taskId && tasks.isSuccess && !selected && <><h1>選択中のタスクを利用できません</h1><p>対象が一覧にありません。別のタスクを自動選択していません。</p></>}
      {sessionData && selected && <><h1>{selected.title}</h1>{selected.contextTitle && <p>文脈：{selected.contextTitle}</p>}<p className={styles.muted}>タスク {selected.id} · 試行 {selected.attemptNumber}（{selected.attemptId}）{selected.workTypeLabel ? ` · ${selected.workTypeLabel}` : ''}</p><span className={styles.badge}>{taskStateLabel(selected.state)}</span>
        {selected.dueAt && <p>期限 <time dateTime={selected.dueAt}>{formatDateTime(selected.dueAt, 'Asia/Tokyo')}</time></p>}
        <AttentionBadges attention={selected.attention} label="このタスクの注意" />
        {newlyAssigned && <div className={styles.actions}><button type="button" disabled={acknowledge.isPending} onClick={() => acknowledge.mutate({ taskId: selected.id, period: newlyAssigned.sourceId! })}>確認済みにする</button></div>}
        {acknowledge.isError && <p role="alert">{workErrorMessage(acknowledge.error)}</p>}
        {attentionNotice && <p role="status">{attentionNotice}</p>}
        {selected.assignment && <TaskAssignmentSummary task={selected} session={sessionData} />}
        {managedOnly ? <TaskAssignmentPanel key={`${selected.id}:${selected.attemptId}`} session={sessionData} task={selected} scope={scope} onAssigned={applyAssignment} /> : blockedId === attemptKey(selected) ? <p role="alert">このタスクを現在の担当では利用できません。内容を非表示にしました。</p> : selected.canClaim ? <TaskAction key={`${sessionData.principalId}:${sessionData.actingAssignmentId}:${selected.id}:${selected.attemptId}`} session={sessionData} task={selected} applyResult={applyResult} refresh={refresh} onDenied={denyDisclosure} /> : detail.isError ? <p role="alert">{workErrorMessage(detail.error)}</p> : detailData ? <>
          {detailData.returnInstructionId && instruction.isPending && <p role="status">差戻指示を確認中…</p>}
          {detailData.returnInstructionId && instruction.isError && <p role="alert">差戻指示を取得できません。{workErrorMessage(instruction.error)}</p>}
          {detailData.returnInstructionId && instruction.isSuccess && <ReturnInstructionView instruction={instruction.data} />}
          {priorSnapshotId && priorSnapshotId !== detailData.handoffSnapshotId && priorSnapshot.isSuccess && <Snapshot snapshot={priorSnapshot.data} received={false} prior />}
          {priorSnapshotId && priorSnapshotId !== detailData.handoffSnapshotId && priorSnapshot.isError && <p role="alert">過去の提出内容を取得できません。{workErrorMessage(priorSnapshot.error)}</p>}
          {snapshot.isError && <p role="alert">受け渡し内容を取得できません。{workErrorMessage(snapshot.error)}</p>}
          {detailData.handoffSnapshotId && snapshot.isFetching && <p role="status">受け渡し内容を確認中…</p>}
          {snapshot.isSuccess && snapshot.data && <Snapshot snapshot={snapshot.data} received={snapshot.data.sourceTaskId !== detailData.id} />}
          <TaskAction key={`${sessionData.principalId}:${sessionData.actingAssignmentId}:${detailData.id}:${detailData.attemptId}`} session={sessionData} task={detailData} detail={detailData} snapshot={snapshot.isSuccess ? snapshot.data : undefined} applyResult={applyResult} refresh={refresh} onDenied={denyDisclosure} />
        </> : <p role="status">タスクの詳細を読み込み中…</p>}
        {alsoManaged && blockedId !== attemptKey(selected) && <TaskAssignmentPanel key={`${selected.id}:${selected.attemptId}`} session={sessionData} task={selected} scope={scope} onAssigned={applyAssignment} />}
      </>}
    </section></div>
  </AppShell>;
}

function TaskAction({ session, task, detail, snapshot, applyResult, refresh, onDenied }: { session: WorkSession; task: TaskSummary; detail?: TaskDetail; snapshot?: HandoffSnapshot; applyResult: (result: WorkResult) => void; refresh: () => Promise<void>; onDenied: () => void }) {
  const artifact = detail?.workingArtifacts[0];
  const evidenceRecords = useEvidenceRecords(session, task, Boolean(detail));
  const mounted = useRef(true); useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { if (isDisclosureDenied(evidenceRecords.error)) onDenied(); }, [evidenceRecords.error]);
  const [transient, setTransient] = useTaskTransient(`${session.principalId}:${session.actingAssignmentId}:${task.id}:${task.attemptId}`);
  const { draft, reason, notice, operation, unknown, error } = transient;
  const setDraft = (value: string | null) => setTransient((previous) => ({ ...previous, draft: value }));
  const setNotice = (value: string) => setTransient((previous) => ({ ...previous, notice: value }));
  const [confirmation, setConfirmation] = useState<'submit' | 'return' | 'complete' | 'hold' | 'resume' | null>(null);
  const [denied, setDenied] = useState(false);
  const text = draft ?? artifact?.value.text ?? '';
  const dirty = text !== (artifact?.value.text ?? '');
  const hasUnsavedInput = dirty || Boolean(reason || transient.agent?.purpose || transient.agent?.support.length || transient.evidence?.documentId || transient.evidence?.fileKey || transient.evidence?.relevantLocation || transient.evidence?.claim || transient.evidence?.support.length || Object.values(transient.evidence?.decisions ?? {}).some((entry) => entry.adoptedClaim || entry.reason || entry.decision !== 'accepted'));
  const oversized = new TextEncoder().encode(text).length > 8192;
  const mutation = useMutation({
    mutationFn: async (input: { execute: () => Promise<WorkResult>; recovery?: boolean }) => input.execute(),
    onSuccess: (result) => {
      if (!mounted.current) return;
      if (result.task.id !== task.id || result.task.attemptId !== task.attemptId || (operation && operation.kind !== result.kind) || (operation?.kind === 'agent_execution_cancelled' && (result.kind !== 'agent_execution_cancelled' || result.execution.id !== operation.executionId))) { setTransient((previous) => ({ ...previous, unknown: true })); setNotice('応答と操作が一致しません。同じ操作IDで結果を確認してください。'); return; }
      applyResult(result); setConfirmation(null);
      if (result.kind === 'agent_execution_requested' || result.kind === 'agent_execution_cancelled') {
        // A recovered/replayed receipt does not admit a worker. Only a fresh terminal read resolves execution uncertainty.
        const unresolved = result.kind === 'agent_execution_requested' && ['queued', 'running'].includes(result.execution.status);
        setTransient((previous) => ({ ...previous, unknown: unresolved, operation: unresolved ? previous.operation : null, error: null, notice: '操作の記録を確認しました。Agentの現在の実行状態を確認してください。', agent: { ...(previous.agent ?? emptyAgentDraft), executionId: result.execution.id, recoveryExecutionId: unresolved ? result.execution.id : undefined } }));
        return;
      }
      setTransient((previous) => ({ ...previous, unknown: false, operation: null, error: null }));
      if (result.kind === 'draft_saved') { setDraft(null); setNotice('文案を保存しました'); }
      if (result.kind === 'returned') { setTransient((previous) => ({ ...previous, reason: null, notice: '差戻が確定しました' })); }
      if (result.kind === 'evidence_registered') setNotice('根拠を登録しました');
      if (result.kind === 'finding_registered') setNotice('候補を登録しました');
      if (result.kind === 'decision_recorded') setNotice('人間判断を記録しました');
      if (result.kind === 'completed') setTransient((previous) => ({ ...previous, draft: null, reason: null, notice: 'タスクの完了が確定しました' }));
      if (result.kind === 'held') setNotice('タスクを保留しました');
      if (result.kind === 'resumed') setNotice('タスクを再開しました');
      if (result.kind === 'claimed') setNotice('担当が確定しました');
      if (result.kind === 'submitted') setNotice(`提出が確定しました`);
    },
    onError: (error, input) => { if (!mounted.current) return; const unresolved = Boolean(input.recovery) || isUnknownOutcome(error); setTransient((previous) => ({ ...previous, unknown: unresolved, operation: unresolved ? previous.operation : null, error })); if (isDisclosureDenied(error) && !(input.recovery && isOperationNotFound(error))) { setDenied(true); setTransient({ draft: null, reason: null, operation: null, unknown: false, notice: '', error }); onDenied(); } setConfirmation(null); },
    retry: false,
  });
  function command(): WorkCommand {
    return { operationId: createOperationId(), expectedRevision: task.revision, actingAssignmentId: task.canClaim && task.claimAssignmentId ? task.claimAssignmentId : actingFor(session, task) };
  }
  function startOperation(request: WorkOperation) {
    setTransient((previous) => ({ ...previous, operation: request, unknown: true, notice: '', error: null }));
    mutation.mutate({ execute: () => executeWorkOperation(request) });
  }
  const returnReason = reason ?? '';
  const returnOversized = new TextEncoder().encode(returnReason).length > 8192;
  const completionActionId = task.canComplete ? task.completionActionId : null;
  const holdActionId = task.canHold ? task.holdActionId : null;
  const resumeActionId = task.canResume ? task.resumeActionId : null;
  const returnTarget = task.canReturn ? task.returnTransition : null;
  const canConfirmReturn = Boolean(returnTarget && snapshot?.id === returnTarget.previousSubmissionId && returnReason.trim() && !returnOversized);
  const busy = mutation.isPending || unknown;
  const selection = transient.sharing ?? emptySelection;
  const closure = evidenceRecords.isSuccess && selectedHandoffIsClosed(selection, evidenceRecords.data.evidence, evidenceRecords.data.findings, evidenceRecords.data.decisions);
  const selectRef = (kind: keyof typeof selection, ref: { id: string; revision: 1 }, selected: boolean) => setTransient((previous) => ({ ...previous, sharing: { ...(previous.sharing ?? emptySelection), [kind]: toggleReference((previous.sharing ?? emptySelection)[kind], ref, selected) } }));
  if (denied) return <p role="alert">このタスクを現在の担当では利用できません。内容を非表示にしました。</p>;
  return <>
    {notice && <p role="status" className={styles.notice}>{notice}</p>}
    {mutation.isPending && <p role="status">処理中です。サーバーの確定を待っています…</p>}
    {Boolean(error) && <div role="alert" className={styles.error}>{unknown && isOperationNotFound(error) ? '操作の確定記録はまだ見つかりません。未到達または処理中の可能性があります。同じ操作IDで再確認または再送できます。' : workErrorMessage(error)}{!unknown && <div className={styles.actions}><button type="button" onClick={() => void refresh()}>現在の状態を再読込</button></div>}</div>}
    {unknown && operation && !mutation.isPending && <div className={styles.notice}><p>結果は未確認です。操作ID: {operation.input.operationId}</p><div className={styles.actions}><button type="button" disabled={mutation.isPending} onClick={() => mutation.mutate({ execute: () => workApi.getOperation(operation.input.operationId), recovery: true })}>同じ操作の結果を確認</button>{isOperationNotFound(error) && <button type="button" disabled={mutation.isPending} onClick={() => mutation.mutate({ execute: () => executeWorkOperation(operation) })}>同じ操作を再送</button>}</div></div>}
    {task.canClaim && <><p>担当候補の一覧です。引き受けが確定するまで文案・受領内容は表示しません。</p><div className={styles.actions}><button type="button" disabled={busy} onClick={() => { startOperation({ kind: 'claimed', taskId: task.id, input: command() }); }}>担当を引き受ける</button></div></>}
    {detail && task.canEdit && <div className={styles.editor}><h2>自分の作業文案</h2><p className={styles.muted}>非公開 · このタスクの担当者だけが閲覧できます。提出で固定した内容だけを次担当へ渡します。</p><label htmlFor="work-draft">作業中の文案</label><textarea id="work-draft" value={text} disabled={busy} onChange={(event) => { setDraft(event.target.value); setNotice(''); }} />
      {oversized && <p role="alert">文案はUTF-8で8192バイト以内にしてください</p>}
      {dirty && <p className={styles.muted}>未保存の変更があります。提出前に保存してください。</p>}
      <div className={styles.actions}><button type="button" disabled={busy || oversized || !text.trim() || (!dirty && Boolean(artifact))} onClick={() => { startOperation({ kind: 'draft_saved', taskId: task.id, input: { ...command(), ...(artifact ? { artifactId: artifact.id } : {}), value: { text } } }); }}>文案を保存</button>
      <button type="button" className={styles.primary} disabled={busy || dirty || !artifact || !task.canSubmit || Boolean(detail.workingArtifacts.length > 1)} onClick={() => setConfirmation('submit')}>提出内容を確認</button></div>
      {!task.canSubmit && <p className={styles.muted}>この段階では提出できません。現在の担当で文案の確認・保存を行えます。</p>}
      {detail.workingArtifacts.length > 1 && <p role="alert">複数の成果物の編集・提出はこのPoCでは対応していません</p>}
    </div>}
    {detail && (holdActionId || resumeActionId) && <section className={styles.editor} aria-label="保留と再開"><h2>保留と再開</h2><p>同じ試行・担当・保存済みの内容を保持します。</p><div className={styles.actions}>
      {holdActionId && <button type="button" disabled={busy} onClick={() => setConfirmation('hold')}>保留内容を確認</button>}
      {resumeActionId && <button type="button" disabled={busy} onClick={() => setConfirmation('resume')}>再開内容を確認</button>}
    </div></section>}
    {detail && task.state === 'held' && <><p className={styles.notice}>保留中は読み取り専用です。保存済みの内容を現在の権限で確認できます。</p>
      {hasUnsavedInput && <p className={styles.notice}>未保存の入力はこのタブ内だけに保持しています。保存済みではありません。再開後に戻ります。タブを閉じると失われます。</p>}
      {detail.workingArtifacts.length > 0 && <section className={styles.snapshot} aria-label="保存済みの作業文案"><h2>保存済みの作業文案</h2>{detail.workingArtifacts.map((saved) => <div key={saved.id}><p className={styles.muted}>非公開 · 文案版 {saved.revision}</p><p className={styles.text}>{saved.value.text}</p></div>)}</section>}
    </>}
    {detail && completionActionId && <section className={styles.editor} aria-label="タスクの完了"><h2>タスクの完了</h2><p>現在のタスクを完了し、保存済みの内容を読み取り専用で残します。</p><div className={styles.actions}><button type="button" className={styles.primary} disabled={busy} onClick={() => setConfirmation('complete')}>完了内容を確認</button></div></section>}
    {detail && returnTarget && <div className={styles.editor}><h2>営業への差戻</h2><p className={styles.muted}>受領した提出内容は固定のまま残し、差戻先に新しい試行を作成します。</p><label htmlFor="return-reason">差戻理由</label><p id="return-reason-help" className={styles.muted}>必須 · 空白のみ不可、UTF-8で8192バイト以内</p><textarea id="return-reason" required aria-describedby="return-reason-help" value={returnReason} disabled={busy} onChange={(event) => { setTransient((previous) => ({ ...previous, reason: event.target.value, notice: '' })); }} />
      {returnOversized && <p role="alert">差戻理由はUTF-8で8192バイト以内にしてください</p>}
      <div className={styles.actions}><button type="button" disabled={busy || !canConfirmReturn} onClick={() => setConfirmation('return')}>差戻内容を確認</button></div>
    </div>}
    {detail && task.state !== 'held' && !task.canEdit && !completionActionId && <p className={styles.notice}>{returnTarget ? '受領した提出内容は読み取り専用です。' : '現在のタスクは読み取り専用です。提出済み内容は変更されません。'}</p>}
    {mutation.data?.kind === 'submitted' && <p>次のタスク：{mutation.data.nextTask.stepLabel}（{taskStateLabel(mutation.data.nextTask.state)}）</p>}
    {confirmation === 'submit' && artifact && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(null); }}><Dialog className={styles.dialog} aria-labelledby="submit-title"><Heading slot="title" id="submit-title">提出の確認</Heading>
      <p>{task.title} · タスク {task.id}</p><p className={styles.muted}>試行 {task.attemptId} · タスク版 {task.revision} · 文案版 {artifact.revision}</p>
      <p>保存済みの次の内容を固定し、ワークフローで定義された次担当へ渡します。</p><div className={styles.snapshot}><p className={styles.text}>{artifact.value.text}</p></div>
      <fieldset disabled={busy}><legend>共有する根拠・候補・判断を選択</legend><p>選択した版だけを固定します。未選択の記録は非公開のままです。何も共有しない提出もできます。</p>
        {evidenceRecords.isPending && <p role="status">共有候補を確認中…</p>}{evidenceRecords.isError && <p role="alert">共有候補を取得できません。再読込してください。</p>}
        {(evidenceRecords.isSuccess ? evidenceRecords.data.evidence : []).map((record) => <label className={styles.shareChoice} key={record.id}><input type="checkbox" aria-label={`共有する根拠 ${record.id}`} checked={selection.evidenceRevisionRefs.some((ref) => ref.id === record.id)} onChange={(event) => selectRef('evidenceRevisionRefs', record, event.target.checked)} />根拠 {record.id} · 版 {record.revision}<br />{record.relevantLocation}</label>)}
        {(evidenceRecords.isSuccess ? evidenceRecords.data.findings : []).map((record) => <label className={styles.shareChoice} key={record.id}><input type="checkbox" aria-label={`共有する候補 ${record.id}`} checked={selection.findingRevisionRefs.some((ref) => ref.id === record.id)} onChange={(event) => selectRef('findingRevisionRefs', record, event.target.checked)} />候補 {record.id} · 版 {record.revision}<br />{record.claim}<br />必要な根拠：{record.evidenceRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</label>)}
        {(evidenceRecords.isSuccess ? evidenceRecords.data.decisions : []).map((record) => <label className={styles.shareChoice} key={record.id}><input type="checkbox" aria-label={`共有する判断 ${record.id}`} checked={selection.decisionRevisionRefs.some((ref) => ref.id === record.id)} onChange={(event) => selectRef('decisionRevisionRefs', record, event.target.checked)} />判断 {record.id} · 版 {record.revision} · {decisionLabel(record.decision)}<br />{record.adoptedClaim ?? record.reason}<br />必要な候補：{record.findingId}（版 {record.findingRevision}）<br />必要な根拠：{record.evidenceRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</label>)}
        {!closure && evidenceRecords.isSuccess && <p role="alert">選択した候補・判断が参照する根拠と候補の同じ版も選択してください。合計100件以内です。</p>}
      </fieldset>
      <div className={styles.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" className={styles.primary} disabled={mutation.isPending || !closure} onClick={() => { startOperation({ kind: 'submitted', taskId: task.id, input: { ...command(), expectedAttemptId: task.attemptId, ...selection, artifacts: [{ artifactId: artifact.id, revision: artifact.revision }] } }); }}>提出を確定</button></div>
    </Dialog></Modal>}
    {confirmation === 'complete' && completionActionId && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(null); }}><Dialog className={styles.dialog} aria-labelledby="complete-title"><Heading slot="title" id="complete-title">タスク完了の確認</Heading>
      <p>{task.title} · タスク {task.id}</p><p>試行 {task.attemptNumber}（{task.attemptId}） · タスク版 {task.revision}</p>
      <p>実行する担当 {session.principalId} · {actingFor(session, task)}</p>
      <p>現在の試行を完了します。完了後は読み取り専用になり、保存済みの提出内容・根拠・人間判断・Agent結果を現在の権限で確認できます。</p>
      <p>新しい担当や提出スナップショットは作成せず、過去の記録は変更しません。</p>
      <div className={styles.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" className={styles.primary} disabled={busy} onClick={() => startOperation({ kind: 'completed', taskId: task.id, input: { ...command(), expectedAttemptId: task.attemptId, action: 'complete', definitionActionId: completionActionId } })}>完了を確定</button></div>
    </Dialog></Modal>}
    {((confirmation === 'hold' && holdActionId) || (confirmation === 'resume' && resumeActionId)) && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(null); }}><Dialog className={styles.dialog} aria-labelledby="hold-resume-title"><Heading slot="title" id="hold-resume-title">{confirmation === 'hold' ? '保留の確認' : '再開の確認'}</Heading>
      <p>{task.title} · タスク {task.id}</p><p>試行 {task.attemptNumber}（{task.attemptId}） · タスク版 {task.revision}</p><p>実行する担当 {session.principalId} · {actingFor(session, task)}</p>
      <p>{confirmation === 'hold' ? '現在の試行を保留し、読み取り専用にします。未保存の入力は保存せず、このタブ内だけに保持します。' : '同じ試行と担当で作業を再開します。このタブ内に保持した未保存の入力を戻します。Agentは自動再実行しません。'}</p>
      <p>保存済みの内容と過去の提出は保持されます。保存・提出・担当変更は行いません。</p>
      <div className={styles.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" className={styles.primary} disabled={busy} onClick={() => {
        if (confirmation === 'hold' && holdActionId) startOperation({ kind: 'held', taskId: task.id, input: { ...command(), expectedAttemptId: task.attemptId, action: 'hold', definitionActionId: holdActionId } });
        if (confirmation === 'resume' && resumeActionId) startOperation({ kind: 'resumed', taskId: task.id, input: { ...command(), expectedAttemptId: task.attemptId, action: 'resume', definitionActionId: resumeActionId } });
      }}>{confirmation === 'hold' ? '保留を確定' : '再開を確定'}</button></div>
    </Dialog></Modal>}
    {confirmation === 'return' && returnTarget && snapshot && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(null); }}><Dialog className={styles.dialog} aria-labelledby="return-title"><Heading slot="title" id="return-title">差戻の確認</Heading>
      <p>{task.title} · 試行 {task.attemptNumber}（{task.attemptId}） · タスク版 {task.revision}</p><p>差戻先タスク {returnTarget.targetTaskId} · 元の提出 {returnTarget.previousSubmissionId}</p>
      <p>現在の試行を完了し、差戻先に新しい試行を作成します。過去の提出内容とこの理由は変更できません。</p>
      <div className={styles.snapshot}><h3>差戻理由</h3><p className={styles.text}>{returnReason}</p><h3>元の提出内容</h3>{snapshot.artifacts.map((item) => <p key={item.artifactId} className={styles.text}>{item.value.text}</p>)}</div>
      <div className={styles.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" className={styles.primary} disabled={mutation.isPending || !canConfirmReturn} onClick={() => startOperation({ kind: 'returned', taskId: task.id, input: { ...command(), expectedAttemptId: task.attemptId, ...returnTarget, reason: returnReason } })}>差戻を確定</button></div>
    </Dialog></Modal>}
  </>;
}
function ReturnInstructionView({ instruction }: { instruction: ReturnInstruction }) {
  return <section className={styles.snapshot} aria-label="確定した差戻指示"><h2>確定した差戻指示</h2><p className={styles.text}>{instruction.reason}</p><p className={styles.muted}>読み取り専用 · {instruction.id}<br />元の提出 {instruction.previousSubmissionId}<br />差戻元の試行 {instruction.sourceAttemptId} → 新しい試行 {instruction.targetAttemptId}<br /><time dateTime={instruction.createdAt}>{formatDateTime(instruction.createdAt)}</time></p></section>;
}
function Snapshot({ snapshot, received, prior = false }: { snapshot: HandoffSnapshot; received: boolean; prior?: boolean }) {
  return <section className={styles.snapshot} aria-label={prior ? '差戻前のスナップショット' : received ? '受領したスナップショット' : '提出済みスナップショット'}><h2>{prior ? '差戻前のスナップショット' : received ? '受領したスナップショット' : '提出済みスナップショット'}</h2><p className={styles.muted}>固定された提出内容 · {snapshot.id}<br /><time dateTime={snapshot.createdAt}>{formatDateTime(snapshot.createdAt)}</time></p>{snapshot.artifacts.map((artifact) => <div key={artifact.artifactId}><p className={styles.muted}>文案版 {artifact.revision}</p><p className={styles.text}>{artifact.value.text}</p></div>)}{(['evidenceRevisionRefs', 'findingRevisionRefs', 'decisionRevisionRefs'] as const).map((kind) => <p key={kind} className={styles.muted}>{({ evidenceRevisionRefs: '共有された根拠', findingRevisionRefs: '共有された候補', decisionRevisionRefs: '共有された判断' })[kind]}：{snapshot[kind]?.length ? snapshot[kind].map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、') : 'なし'}</p>)}</section>;
}
function historyLabel(kind: string): string { return ({ claimed: '担当を引受', draft_saved: '文案を保存', evidence_registered: '根拠を登録', finding_registered: '候補を登録', decision_recorded: '人間判断を記録', submitted: '提出', completed: 'タスクを完了', held: 'タスクを保留', resumed: 'タスクを再開', returned: '差戻', seeded: 'タスクを作成' })[kind] ?? '業務状態を更新'; }
export function OrganizationSearchPage() { const organization = useOrganizationContext(); const search = validateTaskSearch(Object.fromEntries(new URLSearchParams(organization.taskHref.split('?')[1] ?? ''))); return <AppShell activeNavigation="search" mainLabel="検索ワークスペース" showContextPanel={false}><section className={styles.work}><h1>検索</h1><p>Search PlatformはこのブラウザーPoCでは未実装です</p><Link to="/tasks" search={search}>タスクへ戻る</Link></section></AppShell>; }
