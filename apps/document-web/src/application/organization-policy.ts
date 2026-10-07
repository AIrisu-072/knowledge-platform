/** Presentation helpers for Organization policy records. Every result is a hint:
 * the server re-resolves eligibility, assignment and time at mutation time. */
import type { BusinessRole, Delegation, OrganizationalUnit, PolicyAction, Responsibility, RoleAssignment, SyntheticPrincipal, TaskSummary, WorkSession } from '../api/work-api';
import { SYNTHETIC_PRINCIPALS } from '../api/work-api';
import { formatDateTime } from '../view-model/date-time';

export const principalNames: Record<SyntheticPrincipal, string> = {
  'sales-01': '営業担当（模擬）',
  'office-01': '事務担当（模擬）',
  'review-01': '審査担当（模擬）',
  'approver-01': '承認・業務管理（模擬）',
  'multi-role-01': '兼務担当（模擬）',
  'delegate-01': '代理担当（模擬）',
};
export const principalLabel = (id: string) => `${principalNames[id as SyntheticPrincipal] ?? '不明な利用者'} · ${id}`;
export const policyActionLabels: Record<PolicyAction, string> = {
  'context.read': '文脈の閲覧', 'context.progress.read': '進捗の閲覧', 'context.history.read': '文脈履歴の閲覧', 'queue.read': 'キューの閲覧',
  'work.read': 'タスクの閲覧', 'work.claim': '引受', 'work.assign': '担当変更', 'work.edit': '文案の編集', 'work.submit': '提出', 'work.return': '差戻',
  'work.complete': '完了', 'work.hold': '保留', 'work.resume': '再開', 'evidence.register': '根拠の登録', 'finding.register': '候補の登録',
  'decision.record': '人間判断の記録', 'agent.request': 'Agentへの依頼', 'organization.manage': '担当・委任の管理',
};
/** Policy and reassignment reasons: 1024 UTF-8 bytes, no control characters
 * other than line breaks and tabs (the server rejects them; this is a hint). */
export const MAX_REASON_BYTES = 1024;
export function reasonProblem(reason: string): string | undefined {
  if (new TextEncoder().encode(reason).length > MAX_REASON_BYTES) return `理由はUTF-8で${MAX_REASON_BYTES}バイト以内にしてください`;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(reason)) return '理由に改行・タブ以外の制御文字は使用できません';
  return undefined;
}
/** Privilege administration is never delegable in v0 (Organization policy追補§8). */
export const delegable = (action: PolicyAction) => action !== 'organization.manage' && action !== 'work.assign';

const time = (value: string | null | undefined) => (value ? Date.parse(value) : Number.NaN);
export function recordStatus(record: { validFrom: string; validUntil: string | null; revokedAt: string | null }, now: number): 'revoked' | 'scheduled' | 'expired' | 'effective' {
  if (record.revokedAt) return 'revoked';
  if (time(record.validFrom) > now) return 'scheduled';
  if (record.validUntil && time(record.validUntil) <= now) return 'expired';
  return 'effective';
}
export const statusLabel = { revoked: '取消済み', scheduled: '開始前', expired: '期限切れ', effective: '有効' } as const;

export function responsibilityLabel(value: Pick<Responsibility, 'roleLabel' | 'unitLabel' | 'kind' | 'delegator'>): string {
  return `${value.roleLabel}@${value.unitLabel}${value.kind === 'delegation' && value.delegator ? `（${value.delegator}から委任）` : ''}`;
}
export function actingLabel(session: Pick<WorkSession, 'actingAssignmentId'> & { responsibilities?: Responsibility[] | null }, id: string | null | undefined): string {
  const found = session.responsibilities?.find((value) => value.id === id);
  return found ? responsibilityLabel(found) : (id ?? 'なし');
}
/** Only a currently effective formal management assignment may administer policy. */
export function managementResponsibility(session: { responsibilities?: Responsibility[] | null }): Responsibility | undefined {
  return session.responsibilities?.find((value) => value.kind === 'role_assignment' && value.actions.includes('organization.manage'));
}
export function assigningResponsibility(session: { responsibilities?: Responsibility[] | null }, scope?: string): Responsibility | undefined {
  return session.responsibilities?.find((value) => (!scope || value.id === scope) && value.actions.includes('work.assign'));
}

export type AssignmentCandidate = { principal: SyntheticPrincipal; responsibilityId: string; label: string };
/** Hint list from the manager-visible policy records, evaluated at the server's
 * instant; excludes the current period and the managing actor itself. */
export function assignmentCandidates(task: Pick<TaskSummary, 'requiredRoleId' | 'assignment'>, assignments: RoleAssignment[], delegations: Delegation[], roles: BusinessRole[], units: OrganizationalUnit[], now: number, actor: string): AssignmentCandidate[] {
  if (!task.requiredRoleId) return [];
  const roleOf = (id: string) => roles.find((role) => role.id === id);
  const unitOf = (id: string) => units.find((unit) => unit.id === id);
  const current = (principal: string, id: string) => task.assignment?.principalId === principal && task.assignment.actingAssignmentId === id;
  const formal = assignments
    .filter((value) => value.principal !== actor && value.roleId === task.requiredRoleId && recordStatus(value, now) === 'effective' && roleOf(value.roleId)?.actions.includes('work.claim') && !current(value.principal, value.id))
    .map((value) => ({ principal: value.principal, responsibilityId: value.id, label: `${principalLabel(value.principal)} · ${roleOf(value.roleId)?.label ?? '役割'}@${unitOf(value.unitId)?.label ?? '組織'}（正式割当）` }));
  const delegated = delegations
    .filter((value) => {
      const source = assignments.find((assignment) => assignment.id === value.sourceAssignmentId);
      return value.recipient !== actor && source && source.roleId === task.requiredRoleId && source.principal === value.delegator && recordStatus(source, now) === 'effective' && recordStatus(value, now) === 'effective' && value.actions.includes('work.claim') && !current(value.recipient, value.id);
    })
    .map((value) => ({ principal: value.recipient, responsibilityId: value.id, label: `${principalLabel(value.recipient)} · ${value.delegator}から委任（〜${formatDateTime(value.validUntil, 'Asia/Tokyo')}）` }));
  return [...formal, ...delegated];
}
export const knownPrincipals = SYNTHETIC_PRINCIPALS;
