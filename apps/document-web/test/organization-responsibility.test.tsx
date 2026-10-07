import { TextEncoder } from 'node:util';
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { assignmentCandidates, recordStatus } from '../src/application/organization-policy';
import { TaskHomePage } from '../src/routes/TaskHomePage';
import { OrganizationResponsibilitiesPage } from '../src/routes/OrganizationResponsibilitiesPage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(), getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn() } }));

const ROLE_PROCESSING = 'role-processing', ROLE_REVIEWING = 'role-reviewing', ROLE_MANAGEMENT = 'role-management';
const responsibility = (id: string, roleId: string, roleLabel: string, unitLabel: string, actions: string[], extra: Record<string, unknown> = {}) => ({ id, kind: 'role_assignment', principal: 'multi-role-01', roleId, roleLabel, unitId: `unit-${roleId}`, unitLabel, actions, validFrom: '2026-10-01T00:00:00Z', validUntil: null, sourceAssignmentId: null, delegator: null, ...extra });
const processing = responsibility('multi-processing', ROLE_PROCESSING, '事務処理', '事務', ['queue.read', 'work.read', 'work.claim', 'work.edit']);
const reviewing = responsibility('multi-review', ROLE_REVIEWING, '審査', '融資審査', ['queue.read', 'work.read', 'work.claim']);
const multiSession = { principalId: 'multi-role-01', displayName: '兼務担当（模擬）', actingAssignmentId: processing.id, responsibilities: [processing, reviewing], canManageOrganization: false, policyRevision: 3, capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: true } };
const management = { ...responsibility('approver-management', ROLE_MANAGEMENT, '業務管理', '承認', ['queue.read', 'work.assign', 'organization.manage']), principal: 'approver-01' };
const approverSession = { ...multiSession, principalId: 'approver-01', displayName: '承認・業務管理（模擬）', actingAssignmentId: management.id, responsibilities: [management], canManageOrganization: true };
const baseTask = { id: 'office-task', contextId: 'context-1', attemptId: 'office-attempt', attemptNumber: 1, revision: 4, title: '事務内容確認', stepLabel: '事務内容確認', state: 'ready', canClaim: false, canEdit: false, canSubmit: false, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false, returnTransition: null, returnInstructionId: null, handoffSnapshotId: 'snapshot-1', requiredRoleId: ROLE_PROCESSING, claimAssignmentId: null, canAssign: false, assignment: null };
const roleAssignment = (id: string, principal: string, roleId: string, extra: Record<string, unknown> = {}) => ({ id, principal, roleId, unitId: 'unit-office', validFrom: '2026-10-01T00:00:00Z', validUntil: null, reason: '合成', createdBy: null, createdAt: '2026-10-01T00:00:00Z', revokedAt: null, revokedBy: null, revokeReason: null, ...extra });
const roles = [{ id: ROLE_PROCESSING, key: 'processing', label: '事務処理', actions: ['queue.read', 'work.read', 'work.claim', 'work.edit', 'work.submit'] }, { id: ROLE_REVIEWING, key: 'reviewing', label: '審査', actions: ['queue.read', 'work.read', 'work.claim'] }, { id: ROLE_MANAGEMENT, key: 'management', label: '業務管理', actions: ['queue.read', 'work.assign', 'organization.manage'] }];
const units = [{ id: 'unit-office', label: '事務', defaultArchetype: 'queue', roleIds: [ROLE_PROCESSING] }];

function setup(entry: string, session: unknown, tasks: unknown[]) {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(session as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: tasks, nextCursor: null } as never);
  jest.spyOn(workApi, 'getTask').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  jest.spyOn(workApi, 'getSnapshot').mockRejectedValue(new WorkApiError(404, 'WORK_ARTIFACT_NOT_FOUND'));
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  const root = createRootRoute({ component: Outlet });
  const taskRoute = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const responsibilities = createRoute({ getParentRoute: () => root, path: '/organization/responsibilities', component: OrganizationResponsibilitiesPage });
  const router = createRouter({ routeTree: root.addChildren([taskRoute, responsibilities]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { router, client };
}
afterEach(() => jest.restoreAllMocks());

test('policy hints: exclusive end, revocation and current-period exclusion', () => {
  const now = Date.parse('2026-10-07T12:00:00Z');
  expect(recordStatus({ validFrom: '2026-10-07T00:00:00Z', validUntil: '2026-10-07T12:00:00Z', revokedAt: null }, now)).toBe('expired');
  expect(recordStatus({ validFrom: '2026-10-07T13:00:00Z', validUntil: null, revokedAt: null }, now)).toBe('scheduled');
  expect(recordStatus({ validFrom: '2026-10-01T00:00:00Z', validUntil: null, revokedAt: '2026-10-07T11:00:00Z' }, now)).toBe('revoked');
  const assignments = [roleAssignment('office', 'office-01', ROLE_PROCESSING), roleAssignment('multi', 'multi-role-01', ROLE_PROCESSING), roleAssignment('revoked', 'review-01', ROLE_PROCESSING, { revokedAt: '2026-10-07T00:00:00Z', revokedBy: 'approver-01' }), roleAssignment('review', 'review-01', ROLE_REVIEWING)];
  const delegations = [
    { id: 'delegated', sourceAssignmentId: 'office', delegator: 'office-01', recipient: 'delegate-01', actions: ['queue.read', 'work.read', 'work.claim'], validFrom: '2026-10-07T00:00:00Z', validUntil: '2026-10-07T18:00:00Z', reason: '代理', createdBy: 'office-01', createdAt: '2026-10-07T00:00:00Z', revokedAt: null, revokedBy: null, revokeReason: null },
    { id: 'expired', sourceAssignmentId: 'office', delegator: 'office-01', recipient: 'delegate-01', actions: ['work.claim'], validFrom: '2026-10-06T00:00:00Z', validUntil: '2026-10-07T00:00:00Z', reason: '代理', createdBy: 'office-01', createdAt: '2026-10-06T00:00:00Z', revokedAt: null, revokedBy: null, revokeReason: null },
    { id: 'no-claim', sourceAssignmentId: 'office', delegator: 'office-01', recipient: 'review-01', actions: ['work.read'], validFrom: '2026-10-07T00:00:00Z', validUntil: '2026-10-07T18:00:00Z', reason: '閲覧', createdBy: 'office-01', createdAt: '2026-10-07T00:00:00Z', revokedAt: null, revokedBy: null, revokeReason: null },
  ];
  const task = { requiredRoleId: ROLE_PROCESSING, assignment: { principalId: 'office-01', displayName: '事務担当（模擬）', actingAssignmentId: 'office', actingKind: 'role_assignment', roleLabel: '事務処理', delegatorPrincipalId: null, responsibilityEffective: true } } as const;
  const candidates = assignmentCandidates(task as never, assignments as never, delegations as never, roles as never, units as never, now);
  expect(candidates.map((value) => value.responsibilityId)).toEqual(['multi', 'delegated']);
});

test('a concurrent-role principal selects one projection scope and claims with the server-chosen responsibility', async () => {
  const ready = { ...baseTask, canClaim: true, claimAssignmentId: processing.id };
  const { router } = setup('/tasks?view=queue', multiSession, [ready]);
  expect(await screen.findByText(/担当: 事務処理@事務/)).toBeVisible();
  await userEvent.selectOptions(screen.getByLabelText(/表示する担当/), reviewing.id);
  await waitFor(() => expect(router.state.location.search).toMatchObject({ view: 'queue', acting: reviewing.id }));
  await waitFor(() => expect(workApi.listTasks).toHaveBeenLastCalledWith('queue', reviewing.id));
  // A forged or stale scope in the URL is ignored rather than sent as identity.
  await router.navigate({ to: '/tasks', search: { view: 'queue', acting: 'someone-else' } as never });
  await waitFor(() => expect(workApi.listTasks).toHaveBeenLastCalledWith('queue', undefined));
  await router.navigate({ to: '/tasks', search: { view: 'queue', taskId: ready.id } as never });
  const claim = jest.spyOn(workApi, 'claim').mockResolvedValue({ kind: 'claimed', task: { ...ready, state: 'active', canClaim: false } } as never);
  await userEvent.click(await screen.findByRole('button', { name: '担当を引き受ける' }));
  await waitFor(() => expect(claim).toHaveBeenCalledTimes(1));
  expect(claim.mock.calls[0]?.[1]).toMatchObject({ expectedRevision: ready.revision, actingAssignmentId: processing.id });
  expect(workApi.getTask).not.toHaveBeenCalledWith('someone-else');
});

test('a manager sees assignment state but never fetches private detail, and reassigns through an explicit confirmation', async () => {
  const assigned = { ...baseTask, state: 'active', canAssign: true, assignment: { principalId: 'office-01', displayName: '事務担当（模擬）', actingAssignmentId: 'office', actingKind: 'role_assignment', roleLabel: '事務処理', delegatorPrincipalId: null, responsibilityEffective: false } };
  setup(`/tasks?view=queue&taskId=${assigned.id}`, approverSession, [assigned]);
  jest.spyOn(workApi, 'listRoleAssignments').mockResolvedValue({ items: [roleAssignment('office', 'office-01', ROLE_PROCESSING), roleAssignment('multi', 'multi-role-01', ROLE_PROCESSING)], nextCursor: null } as never);
  jest.spyOn(workApi, 'listDelegations').mockResolvedValue({ items: [], nextCursor: null } as never);
  jest.spyOn(workApi, 'listRoles').mockResolvedValue({ items: roles, nextCursor: null } as never);
  jest.spyOn(workApi, 'listUnits').mockResolvedValue({ items: units, nextCursor: null } as never);
  const assign = jest.spyOn(workApi, 'assignTask').mockRejectedValueOnce(new WorkApiError(0, 'network_unavailable', true));
  const panel = await screen.findByRole('region', { name: '担当の管理' });
  expect(screen.getByRole('alert')).toHaveTextContent('現在の担当者の責任（割当・委任）は終了しています');
  expect(workApi.getTask).not.toHaveBeenCalled();
  expect(screen.queryByLabelText('作業中の文案')).not.toBeInTheDocument();
  await userEvent.click(within(panel).getByRole('button', { name: '担当変更の内容を確認' }));
  const dialog = await screen.findByRole('dialog', { name: '担当変更の確認' });
  expect(within(dialog).queryByLabelText(/office-01/)).not.toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '担当変更を確定' })).toBeDisabled();
  await userEvent.click(within(dialog).getByLabelText(/multi-role-01/));
  fireEvent.change(within(dialog).getByLabelText('担当変更の理由'), { target: { value: '担当者の割当終了' } });
  await userEvent.click(within(dialog).getByRole('button', { name: '担当変更を確定' }));
  expect(await within(dialog).findByText(/担当変更の結果は未確認です/)).toBeVisible();
  const command = assign.mock.calls[0]?.[1];
  expect(command).toMatchObject({ expectedRevision: assigned.revision, actingAssignmentId: management.id, expectedAttemptId: assigned.attemptId, assigneePrincipalId: 'multi-role-01', assigneeResponsibilityId: 'multi', reason: '担当者の割当終了' });
  // Recovery reuses the same operation ID; the list projection is then re-evaluated.
  const recovered = { kind: 'assigned', task: { ...assigned, revision: 5, assignment: { ...assigned.assignment, principalId: 'multi-role-01', actingAssignmentId: 'multi', responsibilityEffective: true } }, assignment: { id: 'period', taskId: assigned.id, attemptId: assigned.attemptId, principal: 'multi-role-01', actingAssignmentId: 'multi', assignedBy: 'approver-01', managerAssignmentId: management.id, reason: '担当者の割当終了', startedAt: '2026-10-07T00:00:00Z', endedAt: null, endedBy: null } };
  const recover = jest.spyOn(workApi, 'getOperation').mockResolvedValue(recovered as never);
  // The re-evaluated projection carries the new revision; the confirmed notice must survive it.
  jest.mocked(workApi.listTasks).mockResolvedValue({ items: [recovered.task], nextCursor: null } as never);
  const listCalls = jest.mocked(workApi.listTasks).mock.calls.length;
  await userEvent.click(within(dialog).getByRole('button', { name: '同じ操作の結果を確認' }));
  expect(await screen.findByText('担当変更が確定しました')).toBeVisible();
  expect(recover).toHaveBeenCalledWith(command!.operationId);
  expect(assign).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(jest.mocked(workApi.listTasks).mock.calls.length).toBeGreaterThan(listCalls));
  expect(await screen.findByRole('region', { name: '現在の担当' })).toHaveTextContent('multi-role-01');
  expect(screen.getByText('担当変更が確定しました')).toBeVisible();
});

test('policy unavailability is explicit and never rendered as an empty grant', async () => {
  setup('/tasks?view=queue', { ...multiSession, actingAssignmentId: null, responsibilities: null }, []);
  expect(await screen.findByText(/現在の担当・委任を確認できません/)).toBeVisible();
});

test('a holder delegates a narrowed, time-bounded responsibility after confirmation', async () => {
  setup('/organization/responsibilities', multiSession, []);
  jest.spyOn(workApi, 'listRoleAssignments').mockResolvedValue({ items: [roleAssignment(processing.id, 'multi-role-01', ROLE_PROCESSING)], nextCursor: null } as never);
  jest.spyOn(workApi, 'listDelegations').mockResolvedValue({ items: [], nextCursor: null } as never);
  jest.spyOn(workApi, 'listRoles').mockResolvedValue({ items: roles, nextCursor: null } as never);
  jest.spyOn(workApi, 'listUnits').mockResolvedValue({ items: units, nextCursor: null } as never);
  const created = { kind: 'delegation_created', policyRevision: 4, delegation: { id: 'new-delegation', sourceAssignmentId: processing.id, delegator: 'multi-role-01', recipient: 'delegate-01', actions: ['queue.read', 'work.read', 'work.claim'], validFrom: '2026-10-07T00:00:00Z', validUntil: '2099-10-07T09:00:00Z', reason: '休暇', createdBy: 'multi-role-01', createdAt: '2026-10-07T00:00:00Z', revokedAt: null, revokedBy: null, revokeReason: null } };
  const create = jest.spyOn(workApi, 'createDelegation').mockResolvedValue(created as never);
  const form = await screen.findByRole('region', { name: '委任の作成' });
  expect(within(form).queryByLabelText('担当変更')).not.toBeInTheDocument();
  await userEvent.selectOptions(within(form).getByLabelText('受任者'), 'delegate-01');
  fireEvent.change(within(form).getByLabelText(/期限/), { target: { value: '2099-10-07T18:00' } });
  fireEvent.change(within(form).getByLabelText('委任の理由'), { target: { value: '休暇' } });
  await userEvent.click(within(form).getByRole('button', { name: '委任の内容を確認' }));
  const dialog = await screen.findByRole('dialog', { name: '委任の確認' });
  expect(create).not.toHaveBeenCalled();
  await userEvent.click(within(dialog).getByRole('button', { name: '確定' }));
  expect(await screen.findByText('委任が確定しました')).toBeVisible();
  expect(create.mock.calls[0]?.[0]).toMatchObject({ expectedRevision: 3, actingAssignmentId: processing.id, sourceAssignmentId: processing.id, recipientPrincipalId: 'delegate-01', actions: ['queue.read', 'work.read', 'work.claim'], validUntil: '2099-10-07T09:00:00.000Z', reason: '休暇' });
  await waitFor(() => expect(jest.mocked(workApi.getSession).mock.calls.length).toBeGreaterThan(1));
});

test('only a formal management assignment administers assignments; its own acting assignment is not revocable', async () => {
  setup('/organization/responsibilities', approverSession, []);
  jest.spyOn(workApi, 'listRoleAssignments').mockResolvedValue({ items: [roleAssignment(management.id, 'approver-01', ROLE_MANAGEMENT), roleAssignment('office', 'office-01', ROLE_PROCESSING)], nextCursor: null } as never);
  jest.spyOn(workApi, 'listDelegations').mockResolvedValue({ items: [], nextCursor: null } as never);
  jest.spyOn(workApi, 'listRoles').mockResolvedValue({ items: roles, nextCursor: null } as never);
  jest.spyOn(workApi, 'listUnits').mockResolvedValue({ items: units, nextCursor: null } as never);
  const revoke = jest.spyOn(workApi, 'revokeRoleAssignment').mockResolvedValue({ kind: 'role_assignment_revoked', policyRevision: 4, assignment: roleAssignment('office', 'office-01', ROLE_PROCESSING, { revokedAt: '2026-10-07T00:00:00Z', revokedBy: 'approver-01', revokeReason: '画面から割当を取り消しました' }) } as never);
  await screen.findByRole('heading', { name: '正式割当の管理' });
  expect(screen.getAllByRole('button', { name: 'この割当を取り消す' })).toHaveLength(1);
  await userEvent.click(screen.getByRole('button', { name: 'この割当を取り消す' }));
  const dialog = await screen.findByRole('dialog', { name: '割当の取消の確認' });
  expect(within(dialog).getByText(/自動では解放されません/)).toBeVisible();
  await userEvent.click(within(dialog).getByRole('button', { name: '確定' }));
  expect(await screen.findByText('割当の取消が確定しました')).toBeVisible();
  expect(revoke).toHaveBeenCalledWith('office', expect.objectContaining({ expectedRevision: 3, actingAssignmentId: management.id }));
});
