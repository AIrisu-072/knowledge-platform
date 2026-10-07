import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { workApi, workErrorMessage, taskStateLabel, type TaskSummary, type WorkResult, type WorkSession } from '../../application/work-workspace';
import { assigningResponsibility, assignmentCandidates, principalLabel } from '../../application/organization-policy';
import { createOperationId } from '../../application/operation-id';
import { useRecoverableOperation } from './use-recoverable-operation';
import styles from '../../routes/TaskWorkspace.module.css';

/** Assignment view for the current assignee or a current `work.assign` holder.
 * Shows no private task content; the server decides eligibility at commit. */
export function TaskAssignmentSummary({ task, session }: { task: TaskSummary; session: WorkSession }) {
  const assignment = task.assignment;
  if (!assignment) return <p className={styles.muted}>担当者：未割当（担当待ち）</p>;
  const own = assignment.principalId === session.principalId;
  return <section className={styles.notice} aria-label="現在の担当">
    <p>担当者：{own ? '自分' : principalLabel(assignment.principalId)}{assignment.roleLabel ? ` · ${assignment.roleLabel}` : ''}{assignment.actingKind === 'delegation' ? `（${assignment.delegatorPrincipalId ?? '委任元不明'}から委任）` : assignment.actingKind === 'role_assignment' ? '（正式割当）' : ''}</p>
    {!assignment.responsibilityEffective && <p role="alert">現在の担当者の責任（割当・委任）は終了しています。非公開の内容はその担当者から読めません。担当変更が必要です。</p>}
  </section>;
}

export function TaskAssignmentPanel({ session, task, scope, onAssigned }: { session: WorkSession; task: TaskSummary; scope?: string; onAssigned: (result: WorkResult) => void }) {
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState('');
  const [reason, setReason] = useState('');
  const [notice, setNotice] = useState('');
  const manager = assigningResponsibility(session, scope);
  const records = useQuery({
    queryKey: ['organization', session.principalId, session.actingAssignmentId ?? 'none', 'assignment-candidates', task.id, task.attemptId],
    queryFn: async () => { const [assignments, delegations, roles, units] = await Promise.all([workApi.listRoleAssignments(), workApi.listDelegations(), workApi.listRoles(), workApi.listUnits()]); return { assignments: assignments.items, delegations: delegations.items, roles: roles.items, units: units.items }; },
    enabled: open, staleTime: 0, gcTime: 0, retry: false,
  });
  const candidates = records.isSuccess ? assignmentCandidates(task, records.data.assignments, records.data.delegations, records.data.roles, records.data.units, Date.now()) : [];
  const selected = candidates.find((candidate) => `${candidate.principal}:${candidate.responsibilityId}` === choice);
  const oversized = new TextEncoder().encode(reason).length > 8192;
  const operation = useRecoverableOperation<WorkResult>((result) => { setOpen(false); setChoice(''); setReason(''); setNotice('担当変更が確定しました'); onAssigned(result); });
  const busy = operation.pending || operation.unknown;
  if (!task.canAssign || !manager) return null;
  function confirm() {
    if (!selected || !manager) return;
    const command = { operationId: createOperationId(), expectedRevision: task.revision, actingAssignmentId: manager.id, expectedAttemptId: task.attemptId, assigneePrincipalId: selected.principal, assigneeResponsibilityId: selected.responsibilityId, reason };
    void operation.start({ operationId: command.operationId, send: () => workApi.assignTask(task.id, command), recover: () => workApi.getOperation(command.operationId) });
  }
  const recovery = operation.unknown && operation.operation && !operation.pending ? <div className={styles.notice}><p>担当変更の結果は未確認です。操作ID: {operation.operation.operationId}</p><div className={styles.actions}><button type="button" onClick={() => void operation.recover()}>同じ操作の結果を確認</button>{operation.canResend && <button type="button" onClick={() => void operation.resend()}>同じ操作を再送</button>}</div></div> : null;
  const failure = Boolean(operation.error) && !operation.unknown ? <p role="alert" className={styles.error}>{workErrorMessage(operation.error)}</p> : null;
  return <section className={styles.editor} aria-label="担当の管理">
    <h2>担当の管理</h2>
    <p className={styles.muted}>管理担当として表示しています（{manager.roleLabel}@{manager.unitLabel}）。タスクの非公開内容は表示しません。状態：{taskStateLabel(task.state)} · 試行 {task.attemptNumber}</p>
    {notice && <p role="status" className={styles.notice}>{notice}</p>}
    {!open && recovery}{!open && failure}
    <div className={styles.actions}><button type="button" disabled={busy} onClick={() => { setNotice(''); setOpen(true); }}>担当変更の内容を確認</button></div>
    {open && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={operation.pending} onOpenChange={(next) => { if (!next && !operation.pending) setOpen(false); }}><Dialog className={styles.dialog} aria-labelledby="assign-title"><Heading slot="title" id="assign-title">担当変更の確認</Heading>
      <p>{task.title} · タスク {task.id}</p><p className={styles.muted}>試行 {task.attemptNumber}（{task.attemptId}） · タスク版 {task.revision}</p>
      <p>現在の担当者は以後このタスクの非公開内容を読めなくなります。同じ試行の保存済み文案は新しい担当者へ引き継がれます。過去の操作者の記録は変更しません。</p>
      {records.isPending && <p role="status">担当候補を確認中…</p>}
      {records.isError && <p role="alert">担当候補を取得できません。{workErrorMessage(records.error)}</p>}
      {records.isSuccess && (candidates.length === 0 ? <p>この工程を担当できる現在有効な割当・委任がありません。</p> : <fieldset disabled={operation.pending}><legend>新しい担当者（工程の役割を持つ現在有効な割当・委任）</legend>{candidates.map((candidate) => { const value = `${candidate.principal}:${candidate.responsibilityId}`; return <label key={value} className={styles.shareChoice}><input type="radio" name="assignee" value={value} checked={choice === value} onChange={() => setChoice(value)} />{candidate.label}</label>; })}</fieldset>)}
      <label htmlFor="assign-reason">担当変更の理由</label><p id="assign-reason-help" className={styles.muted}>必須 · 空白のみ不可、UTF-8で8192バイト以内</p>
      <textarea id="assign-reason" required aria-describedby="assign-reason-help" value={reason} disabled={operation.pending} onChange={(event) => setReason(event.target.value)} />
      {oversized && <p role="alert">理由はUTF-8で8192バイト以内にしてください</p>}
      <p>実行する担当 {session.principalId} · {manager.id}</p>
      <div className={styles.actions}><button type="button" autoFocus disabled={operation.pending} onClick={() => setOpen(false)}>キャンセル</button><button type="button" className={styles.primary} disabled={busy || !selected || !reason.trim() || oversized} onClick={confirm}>担当変更を確定</button></div>
      {operation.pending && <p role="status">処理中です。サーバーの確定を待っています…</p>}
      {recovery}{failure}
    </Dialog></Modal>}
  </section>;
}
