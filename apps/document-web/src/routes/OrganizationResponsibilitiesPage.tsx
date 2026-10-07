import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { AppShell } from '../components/app-shell/AppShell';
import { useOrganizationContext } from '../application/organization-context';
import { validateTaskSearch, workApi, workErrorMessage, type WorkSession, type BusinessRole, type Delegation, type OrganizationalUnit, type PolicyAction, type PolicyResult, type Responsibility, type RoleAssignment, type SyntheticPrincipal } from '../application/work-workspace';
import { delegable, knownPrincipals, managementResponsibility, policyActionLabels, principalLabel, recordStatus, responsibilityLabel, statusLabel } from '../application/organization-policy';
import { jstDateTimeLocalToUtc } from '../application/schedule-time';
import { createOperationId } from '../application/operation-id';
import { useRecoverableOperation } from '../components/organization/use-recoverable-operation';
import { formatDateTime } from '../view-model/date-time';
import styles from './TaskWorkspace.module.css';

const sessionKey = ['organization-session'];
const policyKey = (session: WorkSession) => ['organization', session.principalId, session.actingAssignmentId ?? 'none', 'policy-records'];
const instant = (value: string | null) => (value ? formatDateTime(value, 'Asia/Tokyo') : '終了予定なし');
const bytes = (value: string) => new TextEncoder().encode(value).length;
type Pending = { title: string; lines: string[]; run: () => { operationId: string; send: () => Promise<PolicyResult>; recover: () => Promise<PolicyResult> } };

/** Own responsibilities and delegations; a current formal management assignment
 * additionally administers formal assignments. Not a primary navigation entry. */
export function OrganizationResponsibilitiesPage() {
  const organization = useOrganizationContext();
  const client = useQueryClient();
  const session = useQuery({ queryKey: sessionKey, queryFn: workApi.getSession, staleTime: 0, gcTime: 0, retry: false });
  const data = session.isSuccess ? session.data : undefined;
  const records = useQuery({
    queryKey: data ? policyKey(data) : ['organization-policy-unavailable'],
    queryFn: async () => { const [assignments, delegations, roles, units] = await Promise.all([workApi.listRoleAssignments(), workApi.listDelegations(), workApi.listRoles(), workApi.listUnits()]); return { assignments: assignments.items, delegations: delegations.items, roles: roles.items, units: units.items }; },
    enabled: Boolean(data?.responsibilities), staleTime: 0, gcTime: 0, retry: false,
  });
  const [confirmation, setConfirmation] = useState<Pending | null>(null);
  const [notice, setNotice] = useState('');
  const operation = useRecoverableOperation<PolicyResult>((result) => {
    setConfirmation(null);
    setNotice(({ role_assignment_created: '割当の追加が確定しました', role_assignment_revoked: '割当の取消が確定しました', delegation_created: '委任が確定しました', delegation_revoked: '委任の取消が確定しました' })[result.kind]);
    // Responsibilities and every task projection depend on the policy revision.
    void client.invalidateQueries({ queryKey: sessionKey });
    client.removeQueries({ queryKey: ['organization'] });
  });
  const taskSearch = validateTaskSearch(Object.fromEntries(new URLSearchParams(organization.taskHref.split('?')[1] ?? '')));
  const now = Date.now();
  const recovery = operation.unknown && operation.operation && !operation.pending ? <div className={styles.notice}><p>結果は未確認です。操作ID: {operation.operation.operationId}</p><div className={styles.actions}><button type="button" onClick={() => void operation.recover()}>同じ操作の結果を確認</button>{operation.canResend && <button type="button" onClick={() => void operation.resend()}>同じ操作を再送</button>}</div></div> : null;
  const failure = Boolean(operation.error) && !operation.unknown ? <p role="alert" className={styles.error}>{workErrorMessage(operation.error)}</p> : null;
  return <AppShell activeNavigation="tasks" mainLabel="担当と委任" showContextPanel={false} headerContext={<span className={styles.identity}>{data ? <><strong>{data.displayName}</strong> · {data.principalId}<br />起動時固定の模擬ユーザー</> : '担当と委任'}</span>}>
    <section className={styles.work} aria-label="担当と委任">
      <h1>担当と委任</h1>
      <p><Link to="/tasks" search={taskSearch}>タスクへ戻る</Link></p>
      <p className={styles.muted}>担当・委任の追加や取消は、サーバーの現在の組織policyで再評価されます。画面の表示は許可ではありません。</p>
      {session.isPending && <p role="status">利用者を確認中…</p>}
      {session.isError && <p role="alert">{workErrorMessage(session.error)}</p>}
      {data?.responsibilities === null && <p role="alert">現在の担当・委任を確認できません。時間をおいて再読込してください。</p>}
      {notice && <p role="status" className={styles.notice}>{notice}</p>}
      {!confirmation && recovery}{!confirmation && failure}
      {data?.responsibilities && <>
        <h2>現在の担当</h2>
        {data.responsibilities.length === 0 ? <p>現在有効な担当はありません。</p> : <ul>{data.responsibilities.map((value) => <li key={value.id}><strong>{responsibilityLabel(value)}</strong> · {value.kind === 'delegation' ? '委任' : '正式割当'}<br /><span className={styles.muted}>{instant(value.validFrom)} から {instant(value.validUntil)} · {value.id}</span><br /><span className={styles.muted}>操作：{value.actions.map((action) => policyActionLabels[action]).join('、')}</span></li>)}</ul>}
        {records.isPending && <p role="status">担当・委任の記録を確認中…</p>}
        {records.isError && <p role="alert">担当・委任の記録を取得できません。{workErrorMessage(records.error)}</p>}
        {records.isSuccess && <PolicySections session={data} assignments={records.data.assignments} delegations={records.data.delegations} roles={records.data.roles} units={records.data.units} now={now} busy={operation.pending || operation.unknown} confirm={(pending) => { setNotice(''); operation.reset(); setConfirmation(pending); }} />}
      </>}
      {confirmation && <Modal className={styles.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={operation.pending} onOpenChange={(open) => { if (!open && !operation.pending) setConfirmation(null); }}><Dialog className={styles.dialog} aria-labelledby="policy-title"><Heading slot="title" id="policy-title">{confirmation.title}</Heading>
        {confirmation.lines.map((line) => <p key={line} className={styles.text}>{line}</p>)}
        <p>確定はサーバーの成功応答の後だけです。結果が不明な場合は同じ操作IDで確認します。</p>
        <div className={styles.actions}><button type="button" autoFocus disabled={operation.pending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" className={styles.primary} disabled={operation.pending || operation.unknown} onClick={() => void operation.start(confirmation.run())}>確定</button></div>
        {operation.pending && <p role="status">処理中です。サーバーの確定を待っています…</p>}
        {recovery}{failure}
      </Dialog></Modal>}
    </section>
  </AppShell>;
}

function PolicySections({ session, assignments, delegations, roles, units, now, busy, confirm }: { session: WorkSession; assignments: RoleAssignment[]; delegations: Delegation[]; roles: BusinessRole[]; units: OrganizationalUnit[]; now: number; busy: boolean; confirm: (pending: Pending) => void }) {
  const roleOf = (id: string) => roles.find((value) => value.id === id);
  const unitOf = (id: string) => units.find((value) => value.id === id);
  const assignmentLabel = (value: RoleAssignment) => `${roleOf(value.roleId)?.label ?? '役割'}@${unitOf(value.unitId)?.label ?? '組織'}`;
  const manager = managementResponsibility(session);
  const revision = session.policyRevision ?? 0;
  const mine = delegations.filter((value) => value.delegator === session.principalId);
  const received = delegations.filter((value) => value.recipient === session.principalId);
  const ownFormal = (session.responsibilities ?? []).filter((value): value is Responsibility => value.kind === 'role_assignment' && value.actions.some(delegable));
  const revokeDelegation = (value: Delegation, acting: string) => confirm({ title: '委任の取消の確認', lines: [`${principalLabel(value.recipient)} への委任（${instant(value.validFrom)} 〜 ${instant(value.validUntil)}）を取り消します。`, '受任者は次の操作・閲覧から、この委任で担当していたタスクの非公開内容を読めなくなります。'], run: () => { const command = { operationId: createOperationId(), expectedRevision: revision, actingAssignmentId: acting, reason: '画面から委任を取り消しました' }; return { operationId: command.operationId, send: () => workApi.revokeDelegation(value.id, command), recover: () => workApi.getPolicyOperation(command.operationId) }; } });
  return <>
    <h2>自分が出した委任</h2>
    {mine.length === 0 ? <p>委任はありません。</p> : <ul>{mine.map((value) => { const status = recordStatus(value, now); const source = ownFormal.find((item) => item.id === value.sourceAssignmentId); return <li key={value.id}>{principalLabel(value.recipient)} · {statusLabel[status]}<br /><span className={styles.muted}>{instant(value.validFrom)} 〜 {instant(value.validUntil)} · {value.actions.map((action) => policyActionLabels[action]).join('、')}</span>{(status === 'effective' || status === 'scheduled') && (source || manager) && <div className={styles.actions}><button type="button" disabled={busy} onClick={() => revokeDelegation(value, source ? source.id : manager!.id)}>この委任を取り消す</button></div>}</li>; })}</ul>}
    {ownFormal.length > 0 && <DelegationForm session={session} sources={ownFormal} roles={roles} revision={revision} busy={busy} confirm={confirm} />}
    <h2>自分が受けている委任</h2>
    {received.length === 0 ? <p>受けている委任はありません。</p> : <ul>{received.map((value) => <li key={value.id}>{principalLabel(value.delegator)} から · {statusLabel[recordStatus(value, now)]}<br /><span className={styles.muted}>{instant(value.validFrom)} 〜 {instant(value.validUntil)} · {value.actions.map((action) => policyActionLabels[action]).join('、')}</span></li>)}</ul>}
    {manager && <>
      <h2>正式割当の管理</h2>
      <p className={styles.muted}>業務管理の正式割当（{manager.roleLabel}@{manager.unitLabel}）で操作します。取消は過去の操作記録を書き換えません。</p>
      <ul>{assignments.map((value) => { const status = recordStatus(value, now); return <li key={value.id}>{principalLabel(value.principal)} · {assignmentLabel(value)} · {statusLabel[status]}<br /><span className={styles.muted}>{instant(value.validFrom)} 〜 {instant(value.validUntil)}{value.revokedBy ? ` · 取消 ${value.revokedBy}` : ''}</span>{status !== 'revoked' && status !== 'expired' && value.id !== manager.id && <div className={styles.actions}><button type="button" disabled={busy} onClick={() => confirm({ title: '割当の取消の確認', lines: [`${principalLabel(value.principal)} の ${assignmentLabel(value)} を取り消します。`, 'この割当で担当中のタスクは自動では解放されません。担当変更で新しい担当者を割り当ててください。この割当を元にした委任も無効になります。'], run: () => { const command = { operationId: createOperationId(), expectedRevision: revision, actingAssignmentId: manager.id, reason: '画面から割当を取り消しました' }; return { operationId: command.operationId, send: () => workApi.revokeRoleAssignment(value.id, command), recover: () => workApi.getPolicyOperation(command.operationId) }; } })}>この割当を取り消す</button></div>}</li>; })}</ul>
      <AssignmentForm manager={manager} roles={roles} units={units} revision={revision} busy={busy} confirm={confirm} />
      <h2>すべての委任</h2>
      {delegations.length === 0 ? <p>委任はありません。</p> : <ul>{delegations.map((value) => { const status = recordStatus(value, now); return <li key={value.id}>{principalLabel(value.delegator)} → {principalLabel(value.recipient)} · {statusLabel[status]}<br /><span className={styles.muted}>{instant(value.validFrom)} 〜 {instant(value.validUntil)}</span>{(status === 'effective' || status === 'scheduled') && <div className={styles.actions}><button type="button" disabled={busy} onClick={() => revokeDelegation(value, manager.id)}>管理担当として取り消す</button></div>}</li>; })}</ul>}
    </>}
  </>;
}

function DelegationForm({ session, sources, roles, revision, busy, confirm }: { session: WorkSession; sources: Responsibility[]; roles: BusinessRole[]; revision: number; busy: boolean; confirm: (pending: Pending) => void }) {
  const [sourceId, setSourceId] = useState(sources[0]!.id);
  const source = sources.find((value) => value.id === sourceId) ?? sources[0]!;
  const available: PolicyAction[] = (roles.find((value) => value.id === source.roleId)?.actions ?? source.actions).filter(delegable);
  const [recipient, setRecipient] = useState<SyntheticPrincipal | ''>('');
  const [actions, setActions] = useState<PolicyAction[]>(['queue.read', 'work.read', 'work.claim']);
  const [until, setUntil] = useState('');
  const [reason, setReason] = useState('');
  const validUntil = jstDateTimeLocalToUtc(until);
  const chosen = actions.filter((action) => available.includes(action));
  const ready = recipient && chosen.length > 0 && validUntil && Date.parse(validUntil) > Date.now() && reason.trim() && bytes(reason) <= 1024;
  return <section className={styles.editor} aria-label="委任の作成">
    <h3>委任を作成</h3>
    <p className={styles.muted}>自分の正式割当の範囲だけを、期限付きで別の担当者に任せます。担当変更と担当・委任の管理は委任できません。再委任はできません。</p>
    <label>委任元の割当 <select value={source.id} disabled={busy} onChange={(event) => setSourceId(event.target.value)}>{sources.map((value) => <option key={value.id} value={value.id}>{responsibilityLabel(value)}</option>)}</select></label>
    <label>受任者 <select value={recipient} disabled={busy} onChange={(event) => setRecipient(event.target.value as SyntheticPrincipal)}><option value="">選択してください</option>{knownPrincipals.filter((value) => value !== session.principalId).map((value) => <option key={value} value={value}>{principalLabel(value)}</option>)}</select></label>
    <fieldset disabled={busy}><legend>任せる操作</legend>{available.map((action) => <label key={action} className={styles.shareChoice}><input type="checkbox" checked={chosen.includes(action)} onChange={(event) => setActions((previous) => event.target.checked ? [...previous, action] : previous.filter((value) => value !== action))} />{policyActionLabels[action]}</label>)}</fieldset>
    <label>期限（日本時間、この時刻を含まない）<input type="datetime-local" value={until} disabled={busy} onChange={(event) => setUntil(event.target.value)} /></label>
    {until && !validUntil && <p role="alert">期限を正しい日時で入力してください</p>}
    <label htmlFor="delegation-reason">委任の理由</label><textarea id="delegation-reason" value={reason} disabled={busy} onChange={(event) => setReason(event.target.value)} />
    {bytes(reason) > 1024 && <p role="alert">理由はUTF-8で1024バイト以内にしてください</p>}
    <div className={styles.actions}><button type="button" disabled={busy || !ready} onClick={() => confirm({ title: '委任の確認', lines: [`${responsibilityLabel(source)} を ${principalLabel(recipient)} に委任します。`, `任せる操作：${chosen.map((action) => policyActionLabels[action]).join('、')}`, `期限：${instant(validUntil)}（この時刻で失効）`, `理由：${reason}`], run: () => { const command = { operationId: createOperationId(), expectedRevision: revision, actingAssignmentId: source.id, sourceAssignmentId: source.id, recipientPrincipalId: recipient as SyntheticPrincipal, actions: chosen, validUntil: validUntil!, reason }; return { operationId: command.operationId, send: () => workApi.createDelegation(command), recover: () => workApi.getPolicyOperation(command.operationId) }; } })}>委任の内容を確認</button></div>
  </section>;
}

function AssignmentForm({ manager, roles, units, revision, busy, confirm }: { manager: Responsibility; roles: BusinessRole[]; units: OrganizationalUnit[]; revision: number; busy: boolean; confirm: (pending: Pending) => void }) {
  const pairs = units.flatMap((unit) => unit.roleIds.map((roleId) => ({ unit, role: roles.find((value) => value.id === roleId) })).filter((pair): pair is { unit: OrganizationalUnit; role: BusinessRole } => Boolean(pair.role)));
  const [principal, setPrincipal] = useState<SyntheticPrincipal | ''>('');
  const [pair, setPair] = useState('');
  const [until, setUntil] = useState('');
  const [reason, setReason] = useState('');
  const selected = pairs.find((value) => `${value.unit.id}:${value.role.id}` === pair);
  const validUntil = until ? jstDateTimeLocalToUtc(until) : null;
  const ready = principal && selected && (!until || (validUntil && Date.parse(validUntil) > Date.now())) && reason.trim() && bytes(reason) <= 1024;
  return <section className={styles.editor} aria-label="割当の追加">
    <h3>割当を追加</h3>
    <label>担当者 <select value={principal} disabled={busy} onChange={(event) => setPrincipal(event.target.value as SyntheticPrincipal)}><option value="">選択してください</option>{knownPrincipals.map((value) => <option key={value} value={value}>{principalLabel(value)}</option>)}</select></label>
    <label>役割@組織 <select value={pair} disabled={busy} onChange={(event) => setPair(event.target.value)}><option value="">選択してください</option>{pairs.map((value) => <option key={`${value.unit.id}:${value.role.id}`} value={`${value.unit.id}:${value.role.id}`}>{value.role.label}@{value.unit.label}</option>)}</select></label>
    <label>終了（日本時間、任意）<input type="datetime-local" value={until} disabled={busy} onChange={(event) => setUntil(event.target.value)} /></label>
    <label htmlFor="assignment-reason">割当の理由</label><textarea id="assignment-reason" value={reason} disabled={busy} onChange={(event) => setReason(event.target.value)} />
    {bytes(reason) > 1024 && <p role="alert">理由はUTF-8で1024バイト以内にしてください</p>}
    <div className={styles.actions}><button type="button" disabled={busy || !ready} onClick={() => confirm({ title: '割当の追加の確認', lines: [`${principalLabel(principal)} に ${selected!.role.label}@${selected!.unit.label} を割り当てます。`, `終了：${instant(validUntil)}`, `理由：${reason}`], run: () => { const command = { operationId: createOperationId(), expectedRevision: revision, actingAssignmentId: manager.id, principalId: principal as SyntheticPrincipal, roleId: selected!.role.id, unitId: selected!.unit.id, ...(validUntil ? { validUntil } : {}), reason }; return { operationId: command.operationId, send: () => workApi.createRoleAssignment(command), recover: () => workApi.getPolicyOperation(command.operationId) }; } })}>割当の内容を確認</button></div>
  </section>;
}
