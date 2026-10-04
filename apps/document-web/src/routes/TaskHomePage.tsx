import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useRouterState, useSearch } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { AppShell } from '../components/app-shell/AppShell';
import { useOrganizationContext, useTaskTransient } from '../application/organization-context';
import { createOperationId } from '../application/operation-id';
import { validateDetailSearch } from '../application/search-state';
import { workApi, executeWorkOperation, isOperationNotFound, validateTaskSearch, workErrorMessage, isDisclosureDenied, isUnknownOutcome, taskStateLabel, type TaskSearch, type TaskSummary, type TaskDetail, type WorkSession, type WorkResult, type WorkCommand, type WorkOperation, type HandoffSnapshot } from '../application/work-workspace';
import { formatDateTime } from '../view-model/date-time';
import styles from './TaskWorkspace.module.css';

const sessionKey = ['organization-session'];
const actorKey = (session: WorkSession) => ['organization', session.principalId, session.actingAssignmentId];
const taskKey = (session: WorkSession, id: string) => [...actorKey(session), 'task', id];

export function TaskHomePage() {
  const search = useSearch({ from: '/tasks' }) as TaskSearch;
  const navigate = useNavigate({ from: '/tasks' });
  const currentHref = useRouterState({ select: (state) => state.location.href });
  const organization = useOrganizationContext();
  const client = useQueryClient();
  const [blockedId, setBlockedId] = useState<string | null>(null);
  const session = useQuery({ queryKey: sessionKey, queryFn: workApi.getSession, staleTime: 0, gcTime: 0, retry: false });
  const sessionData = session.isSuccess ? session.data : undefined;
  useEffect(() => { if (sessionData && (currentHref === '/tasks' || currentHref.startsWith('/tasks?'))) organization.setContext(sessionData, currentHref); }, [sessionData, currentHref, organization.setContext]);
  const tasks = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'tasks', search.view], queryFn: () => workApi.listTasks(search.view), enabled: Boolean(sessionData), placeholderData: (previous, query) => query?.queryKey[1] === sessionData?.principalId && query?.queryKey[2] === sessionData?.actingAssignmentId ? previous : undefined, staleTime: 0, gcTime: 0, retry: false });
  const selected = tasks.isSuccess ? tasks.data.items.find((item) => item.id === search.taskId) : undefined;
  const detail = useQuery({ queryKey: sessionData && selected ? taskKey(sessionData, selected.id) : ['organization-no-task'], queryFn: () => workApi.getTask(selected!.id), enabled: Boolean(sessionData && selected && !selected.canClaim), staleTime: 0, gcTime: 0, retry: false });
  const detailData = sessionData && blockedId !== selected?.id && detail.isSuccess && selected && !selected.canClaim ? detail.data : undefined;
  const snapshot = useQuery({ queryKey: [...(sessionData ? actorKey(sessionData) : ['organization-unavailable']), 'snapshot', detailData?.handoffSnapshotId], queryFn: () => workApi.getSnapshot(detailData!.handoffSnapshotId!), enabled: Boolean(detailData?.handoffSnapshotId), staleTime: 0, gcTime: 0, retry: false });
  const [module, setModule] = useState('document');
  useEffect(() => { setModule('document'); setBlockedId(null); }, [search.taskId, sessionData?.principalId]);
  const updateSearch = (patch: Partial<TaskSearch>) => { void navigate({ search: (previous) => ({ ...previous, ...patch }) }); };
  const refresh = async () => { await tasks.refetch(); if (selected && !selected.canClaim) await detail.refetch(); if (detailData?.handoffSnapshotId) await snapshot.refetch(); };
  function denyDisclosure() {
    if (!selected || !sessionData) return;
    setBlockedId(selected.id);
    client.removeQueries({ queryKey: taskKey(sessionData, selected.id) });
    client.removeQueries({ queryKey: [...actorKey(sessionData), 'snapshot'] });
  }
  function applyResult(result: WorkResult) {
    if (!sessionData || result.task.id !== selected?.id) return;
    client.setQueriesData<{ items: TaskSummary[]; nextCursor: null }>({ queryKey: [...actorKey(sessionData), 'tasks'] }, (previous) => previous ? { ...previous, items: previous.items.map((item) => item.id === result.task.id ? result.task : item) } : previous);
    if (result.kind !== 'claimed') client.setQueryData<TaskDetail>(taskKey(sessionData, result.task.id), (previous) => previous ? { ...previous, ...result.task, workingArtifacts: result.kind === 'draft_saved' ? [result.artifact] : previous.workingArtifacts } : previous);
    if (result.kind === 'submitted') client.setQueryData([...actorKey(sessionData), 'snapshot', result.snapshot.id], result.snapshot);
  }
  const collection = <aside className={styles.collection} aria-label="タスク一覧">
    <p className={styles.muted}>同じタスクの2つの表示</p>
    <button type="button" aria-pressed={search.view === 'context'} onClick={() => updateSearch({ view: 'context' })}>営業型・文脈</button>
    <button type="button" aria-pressed={search.view === 'queue'} onClick={() => updateSearch({ view: 'queue' })}>事務型・キュー</button>
    <h2>{search.view === 'context' ? '文脈ごとのタスク' : '担当可能なキュー'}</h2>
    <p className={styles.muted}>現在の担当で閲覧できる範囲</p>
    {tasks.isFetching && <p role="status">一覧を読み込み中…</p>}
    {tasks.isError && <p role="alert">{workErrorMessage(tasks.error)}</p>}
    {tasks.isSuccess && tasks.data.items.length === 0 && <p>閲覧できるタスクはありません</p>}
    {tasks.isSuccess && tasks.data.items.map((item) => <button type="button" key={item.id} aria-pressed={search.taskId === item.id} onClick={() => updateSearch({ taskId: item.id })}>
      {item.title}<span>{taskStateLabel(item.state)}</span><small>{item.canClaim ? '担当を引き受けると詳細を表示' : search.view === 'context' ? `文脈 ${item.contextId}` : item.stepLabel}</small>
    </button>)}
    <div className={styles.actions}><button type="button" onClick={() => void refresh()}>再読込</button></div>
  </aside>;
  const contextPanel = <>
    <div className={styles.moduleButtons} aria-label="文脈モジュール">
      {[['document', '文書・比較'], ['history', '履歴'], ['evidence', '根拠'], ['agent', 'Agent'], ['search', '検索'], ['resources', 'Workspace'], ['return', '差戻']].map(([id, label]) => <button key={id} type="button" aria-pressed={module === id} onClick={() => setModule(id!)}>{label}</button>)}
    </div>
    {!detailData ? <p>担当を引き受けたタスクの情報を表示します</p> : module === 'document' ? <>
      <h2>共有の入力文書</h2><p className={styles.muted}>文書側の現在の権限で開きます。作業文案とは別の共有資料です。</p>
      {detailData.inputResources.length === 0 ? <p>入力文書はありません</p> : <ul>{detailData.inputResources.map((input) => <li key={input.documentId}><Link to="/documents/$documentId" params={{ documentId: input.documentId }} search={validateDetailSearch({ view: 'published' })}>{input.label}</Link></li>)}</ul>}
      <p className={styles.muted}>版・改訂・比較は既存の文書画面で確認できます。戻ると選択したタスクに戻ります。</p>
    </> : module === 'history' ? <><h2>業務履歴</h2>{detailData.history.length ? <ul>{detailData.history.map((entry, index) => <li key={`${entry.occurredAt}-${index}`}>{historyLabel(entry.kind)}<br /><time dateTime={entry.occurredAt}>{formatDateTime(entry.occurredAt)}</time></li>)}</ul> : <p>記録された履歴はありません</p>}<p className={styles.muted}>この履歴は監査基盤の資格取得を示すものではありません。</p></> : <><h2>{module === 'agent' ? 'Agent' : module === 'search' ? '検索' : module === 'evidence' ? '根拠・判断' : module === 'return' ? '差戻' : 'Workspace'}</h2><p>このブラウザーPoCでは未実装です</p></>}
    <div className={styles.notice}><strong>ブラウザーPoC</strong><p>ブラウザーではネイティブWorkspaceを利用できません</p><p>Search / Agent / ファイル添付 / 差戻は未実装です</p></div>
  </>;
  return <AppShell activeNavigation="tasks" mainLabel="タスクワークスペース" headerContext={<span className={styles.identity}>{sessionData ? <><strong>{sessionData.displayName}</strong> · {sessionData.principalId}<br />担当: {sessionData.actingAssignmentId} · 起動時固定の模擬ユーザー</> : 'タスク'} </span>} contextPanel={contextPanel}>
    <div className={styles.layout}>{collection}<section className={styles.work} aria-label="主作業">
      {session.isPending && <p role="status">利用者を確認中…</p>}
      {session.isError && <><h1>タスクを開けません</h1><p role="alert">{workErrorMessage(session.error)}</p></>}
      {sessionData && !search.taskId && <><h1>タスク</h1><p>一覧から作業するタスクを選択してください</p></>}
      {sessionData && search.taskId && tasks.isSuccess && !selected && <><h1>選択中のタスクを利用できません</h1><p>対象が一覧にありません。別のタスクを自動選択していません。</p></>}
      {sessionData && selected && <><h1>{selected.title}</h1><p className={styles.muted}>タスク {selected.id} · 試行 {selected.attemptId}</p><span className={styles.badge}>{taskStateLabel(selected.state)}</span>
        {blockedId === selected.id ? <p role="alert">このタスクを現在の担当では利用できません。内容を非表示にしました。</p> : selected.canClaim ? <TaskAction key={`${sessionData.principalId}:${selected.id}`} session={sessionData} task={selected} applyResult={applyResult} refresh={refresh} onDenied={denyDisclosure} /> : detail.isError ? <p role="alert">{workErrorMessage(detail.error)}</p> : detailData ? <>
          {snapshot.isError && <p role="alert">受け渡し内容を取得できません。{workErrorMessage(snapshot.error)}</p>}
          {detailData.handoffSnapshotId && snapshot.isFetching && <p role="status">受け渡し内容を確認中…</p>}
          {snapshot.isSuccess && snapshot.data && <Snapshot snapshot={snapshot.data} received={snapshot.data.sourceTaskId !== detailData.id} />}
          <TaskAction key={`${sessionData.principalId}:${detailData.id}:${detailData.attemptId}`} session={sessionData} task={detailData} detail={detailData} applyResult={applyResult} refresh={refresh} onDenied={denyDisclosure} />
        </> : <p role="status">タスクの詳細を読み込み中…</p>}
      </>}
    </section></div>
  </AppShell>;
}

function TaskAction({ session, task, detail, applyResult, refresh, onDenied }: { session: WorkSession; task: TaskSummary; detail?: TaskDetail; applyResult: (result: WorkResult) => void; refresh: () => Promise<void>; onDenied: () => void }) {
  const artifact = detail?.workingArtifacts[0];
  const [transient, setTransient] = useTaskTransient(`${session.principalId}:${session.actingAssignmentId}:${task.id}:${task.attemptId}`);
  const { draft, notice, operation, unknown, error } = transient;
  const setDraft = (value: string | null) => setTransient((previous) => ({ ...previous, draft: value }));
  const setNotice = (value: string) => setTransient((previous) => ({ ...previous, notice: value }));
  const [confirmation, setConfirmation] = useState(false);
  const [denied, setDenied] = useState(false);
  const text = draft ?? artifact?.value.text ?? '';
  const dirty = text !== (artifact?.value.text ?? '');
  const oversized = new TextEncoder().encode(text).length > 8192;
  const mutation = useMutation({
    mutationFn: async (input: { execute: () => Promise<WorkResult>; recovery?: boolean }) => input.execute(),
    onSuccess: (result) => {
      if (result.task.id !== task.id || (operation && operation.kind !== result.kind)) { setTransient((previous) => ({ ...previous, unknown: true })); setNotice('応答と操作が一致しません。同じ操作IDで結果を確認してください。'); return; }
      applyResult(result); setTransient((previous) => ({ ...previous, unknown: false, operation: null, error: null })); setConfirmation(false);
      if (result.kind === 'draft_saved') { setDraft(null); setNotice('文案を保存しました'); }
      if (result.kind === 'claimed') setNotice('担当が確定しました');
      if (result.kind === 'submitted') setNotice(`提出が確定しました`);
    },
    onError: (error, input) => { const unresolved = Boolean(input.recovery) || isUnknownOutcome(error); setTransient((previous) => ({ ...previous, unknown: unresolved, operation: unresolved ? previous.operation : null, error })); if (isDisclosureDenied(error) && !(input.recovery && isOperationNotFound(error))) { setDenied(true); setTransient({ draft: null, operation: null, unknown: false, notice: '', error }); onDenied(); } setConfirmation(false); },
    retry: false,
  });
  function command(): WorkCommand {
    return { operationId: createOperationId(), expectedRevision: task.revision, actingAssignmentId: session.actingAssignmentId };
  }
  function startOperation(request: WorkOperation) {
    setTransient((previous) => ({ ...previous, operation: request, unknown: true, notice: '', error: null }));
    mutation.mutate({ execute: () => executeWorkOperation(request) });
  }
  const busy = mutation.isPending || unknown;
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
      <button type="button" className={styles.primary} disabled={busy || dirty || !artifact || !task.canSubmit || Boolean(detail.workingArtifacts.length > 1)} onClick={() => setConfirmation(true)}>提出内容を確認</button></div>
      {!task.canSubmit && <p className={styles.muted}>この段階では提出できません。現在の担当で文案の確認・保存を行えます。</p>}
      {detail.workingArtifacts.length > 1 && <p role="alert">複数の成果物の編集・提出はこのPoCでは対応していません</p>}
    </div>}
    {detail && !task.canEdit && <p className={styles.notice}>現在のタスクは読み取り専用です。提出済み内容は変更されません。</p>}
    {mutation.data?.kind === 'submitted' && <p>次のタスク：{mutation.data.nextTask.stepLabel}（{taskStateLabel(mutation.data.nextTask.state)}）</p>}
    {confirmation && artifact && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(false); }}><Dialog className={styles.dialog} aria-labelledby="submit-title"><Heading slot="title" id="submit-title">提出の確認</Heading>
      <p>{task.title} · タスク {task.id}</p><p className={styles.muted}>試行 {task.attemptId} · タスク版 {task.revision} · 文案版 {artifact.revision}</p>
      <p>保存済みの次の内容を固定し、ワークフローで定義された次担当へ渡します。</p><div className={styles.snapshot}><p className={styles.text}>{artifact.value.text}</p></div>
      <div className={styles.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(false)}>キャンセル</button><button type="button" className={styles.primary} disabled={mutation.isPending} onClick={() => { startOperation({ kind: 'submitted', taskId: task.id, input: { ...command(), artifacts: [{ artifactId: artifact.id, revision: artifact.revision }] } }); }}>提出を確定</button></div>
    </Dialog></Modal>}
  </>;
}
function Snapshot({ snapshot, received }: { snapshot: HandoffSnapshot; received: boolean }) {
  return <section className={styles.snapshot} aria-label={received ? '受領したスナップショット' : '提出済みスナップショット'}><h2>{received ? '受領したスナップショット' : '提出済みスナップショット'}</h2><p className={styles.muted}>固定された提出内容 · {snapshot.id}<br /><time dateTime={snapshot.createdAt}>{formatDateTime(snapshot.createdAt)}</time></p>{snapshot.artifacts.map((artifact) => <div key={artifact.artifactId}><p className={styles.muted}>文案版 {artifact.revision}</p><p className={styles.text}>{artifact.value.text}</p></div>)}</section>;
}
function historyLabel(kind: string): string { return ({ claimed: '担当を引受', draft_saved: '文案を保存', submitted: '提出', seeded: 'タスクを作成' })[kind] ?? '業務状態を更新'; }
export function OrganizationSearchPage() { const organization = useOrganizationContext(); const search = validateTaskSearch(Object.fromEntries(new URLSearchParams(organization.taskHref.split('?')[1] ?? ''))); return <AppShell activeNavigation="search" mainLabel="検索ワークスペース" showContextPanel={false}><section className={styles.work}><h1>検索</h1><p>Search PlatformはこのブラウザーPoCでは未実装です</p><Link to="/tasks" search={search}>タスクへ戻る</Link></section></AppShell>; }
