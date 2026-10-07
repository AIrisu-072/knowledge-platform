import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { workApi, workErrorMessage, taskStateLabel, type Attention, type TaskSummary, type WorkContext, type WorkSession } from '../../application/work-workspace';
import { formatDateTime } from '../../view-model/date-time';
import styles from '../../routes/TaskWorkspace.module.css';

/** Derived attention is text plus position, never colour alone; it is not a state. */
export const attentionLabels: Record<Attention['kind'], string> = { newly_assigned: '新しい割当', returned: '差戻し', due_soon: '期限間近', overdue: '期限超過' };
export const contextKindLabels: Record<WorkContext['kind'], string> = { case: '案件', routine_run: '定型処理', batch: '一括処理', request: '依頼' };
const jst = (value: string) => formatDateTime(value, 'Asia/Tokyo');

export function AttentionBadges({ attention, label }: { attention: Attention[]; label?: string }) {
  if (!attention.length) return null;
  return <ul className={styles.attention} aria-label={label ?? '注意'}>
    {attention.map((item) => <li key={`${item.kind}:${item.sourceId ?? ''}`}><strong>{attentionLabels[item.kind]}</strong>{item.dueAt ? <> · 期限 <time dateTime={item.dueAt}>{jst(item.dueAt)}</time></> : null}</li>)}
  </ul>;
}

export function TaskRowButton({ item, selected, hint, onSelect }: { item: TaskSummary; selected: boolean; hint: string; onSelect: () => void }) {
  return <button type="button" aria-pressed={selected} onClick={onSelect}>
    {item.title}<span>{taskStateLabel(item.state)}</span>
    {item.attention.length > 0 && <span className={styles.muted}>{item.attention.map((value) => attentionLabels[value.kind]).join('・')}</span>}
    {item.dueAt && <span className={styles.muted}>期限 <time dateTime={item.dueAt}>{jst(item.dueAt)}</time></span>}
    <small>{hint}</small>
  </button>;
}

/** Sales archetype: authorized WorkContexts first, then their related tasks. Tasks
 * whose context identity is not disclosed stay listed without a customer label. */
export function ContextCollection({ contexts, contextError, tasks, selectedContext, selectedTask, hint, onContext, onTask }: { contexts: WorkContext[] | undefined; contextError: unknown; tasks: TaskSummary[]; selectedContext?: string; selectedTask?: string; hint: (item: TaskSummary) => string; onContext: (id: string) => void; onTask: (id: string) => void }) {
  const known = new Set((contexts ?? []).map((value) => value.id));
  const other = tasks.filter((item) => !known.has(item.contextId));
  return <>
    {contextError ? <p className={styles.muted}>文脈の一覧を取得できません。タスクだけを表示します。</p> : null}
    {(contexts ?? []).map((context) => <section key={context.id} className={styles.contextGroup} aria-label={`文脈 ${context.title}`}>
      <button type="button" aria-pressed={selectedContext === context.id && !selectedTask} onClick={() => onContext(context.id)}>
        {context.title}<span>{contextKindLabels[context.kind]}</span>{context.attentionCount > 0 && <small>注意 {context.attentionCount}件</small>}
      </button>
      {tasks.filter((item) => item.contextId === context.id).map((item) => <TaskRowButton key={item.id} item={item} selected={selectedTask === item.id} hint={hint(item)} onSelect={() => onTask(item.id)} />)}
    </section>)}
    {other.length > 0 && <section className={styles.contextGroup} aria-label="文脈を表示しないタスク">
      {contexts?.length ? <h3>文脈を表示しないタスク</h3> : null}
      {other.map((item) => <TaskRowButton key={item.id} item={item} selected={selectedTask === item.id} hint={hint(item)} onSelect={() => onTask(item.id)} />)}
    </section>}
  </>;
}

/** Office/review archetype: WorkType first, then own work and eligible-only rows. */
export function QueueCollection({ tasks, session, workTypeId, selectedTask, hint, onWorkType, onTask }: { tasks: TaskSummary[]; session: WorkSession; workTypeId?: string; selectedTask?: string; hint: (item: TaskSummary) => string; onWorkType: (id?: string) => void; onTask: (id: string) => void }) {
  const types = Array.from(new Map(tasks.map((item) => [item.workTypeId, item.workTypeLabel || item.stepLabel])).entries());
  const shown = tasks.filter((item) => !workTypeId || item.workTypeId === workTypeId);
  const own = shown.filter((item) => item.assignment?.principalId === session.principalId || (!item.canClaim && !item.canAssign));
  const claimable = shown.filter((item) => item.canClaim);
  const managed = shown.filter((item) => !own.includes(item) && !claimable.includes(item));
  const group = (label: string, items: TaskSummary[]) => items.length > 0 && <section className={styles.contextGroup} aria-label={label}><h3>{label}</h3>{items.map((item) => <TaskRowButton key={item.id} item={item} selected={selectedTask === item.id} hint={hint(item)} onSelect={() => onTask(item.id)} />)}</section>;
  return <>
    {types.length > 1 && <div className={styles.moduleButtons} aria-label="業務の種類">
      <button type="button" aria-pressed={!workTypeId} onClick={() => onWorkType(undefined)}>すべての種類</button>
      {types.map(([id, label]) => <button key={id} type="button" aria-pressed={workTypeId === id} onClick={() => onWorkType(id)}>{label}</button>)}
    </div>}
    {group('自分の担当', own)}
    {group('引受可能', claimable)}
    {group('管理対象のタスク', managed)}
  </>;
}

/** Context surface: identity, permitted progress and own next work; never private task detail. */
export function ContextOverview({ session, context, tasks, onTask }: { session: WorkSession; context: WorkContext; tasks: TaskSummary[]; onTask: (id: string) => void }) {
  const [showHistory, setShowHistory] = useState(false);
  const history = useQuery({ queryKey: ['organization', session.principalId, session.actingAssignmentId, 'context-history', context.id], queryFn: () => workApi.getWorkContextHistory(context.id), enabled: showHistory && context.canReadHistory, staleTime: 0, gcTime: 0, retry: false });
  const own = tasks.filter((item) => context.ownTaskIds.includes(item.id));
  const related = tasks.filter((item) => item.contextId === context.id);
  return <section aria-label="文脈の概要">
    <h1>{context.title}</h1>
    <p className={styles.muted}>{contextKindLabels[context.kind]} · 文脈 {context.id}</p>
    <h2>工程の進捗</h2>
    {context.progress ? <ol className={styles.progress} aria-label="工程の進捗">{context.progress.map((step) => <li key={step.taskId}><strong>{step.stepLabel}</strong> · {taskStateLabel(step.state)} · 試行 {step.attemptNumber} · {step.assigned ? '担当あり' : '未割当'}{step.dueAt ? <> · 期限 <time dateTime={step.dueAt}>{jst(step.dueAt)}</time></> : null}</li>)}</ol> : <p>この担当では文脈の進捗を表示できません。</p>}
    <h2>自分の次の作業</h2>
    {own.length ? <ul>{own.map((item) => <li key={item.id}><button type="button" onClick={() => onTask(item.id)}>{item.title}を開く</button> · {taskStateLabel(item.state)}<AttentionBadges attention={item.attention} label={`${item.title}の注意`} /></li>)}</ul> : related.some((item) => item.canClaim) ? <ul>{related.filter((item) => item.canClaim).map((item) => <li key={item.id}><button type="button" onClick={() => onTask(item.id)}>{item.title}（引受可能）を開く</button><AttentionBadges attention={item.attention} label={`${item.title}の注意`} /></li>)}</ul> : <p>この文脈で自分が担当している作業はありません。</p>}
    {context.canReadHistory && <><h2>文脈の履歴</h2>
      {!showHistory ? <div className={styles.actions}><button type="button" onClick={() => setShowHistory(true)}>文脈の履歴を表示</button></div>
        : history.isPending ? <p role="status">履歴を確認中…</p>
          : history.isError ? <p role="alert">履歴を取得できません。{workErrorMessage(history.error)}</p>
            : history.data.entries.length ? <ul aria-label="文脈の履歴">{history.data.entries.map((entry, index) => <li key={`${entry.occurredAt}-${index}`}>{historyKind(entry.kind)} · <time dateTime={entry.occurredAt}>{jst(entry.occurredAt)}</time></li>)}</ul> : <p>記録された履歴はありません</p>}
      <p className={styles.muted}>理由・本文・担当者の識別は含みません。この履歴は監査基盤の資格取得を示すものではありません。</p></>}
  </section>;
}
export function historyKind(kind: string): string { return ({ claimed: '担当を引受', draft_saved: '文案を保存', evidence_registered: '根拠を登録', finding_registered: '候補を登録', decision_recorded: '人間判断を記録', submitted: '提出', completed: 'タスクを完了', held: 'タスクを保留', resumed: 'タスクを再開', returned: '差戻', assigned: '担当変更', seeded: 'タスクを作成' })[kind] ?? '業務状態を更新'; }
