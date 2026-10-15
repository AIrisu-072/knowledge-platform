import { TextEncoder } from 'node:util';
Object.assign(globalThis, { TextEncoder });
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { validateTaskSearch } from '../src/application/work-workspace';
import { TaskHomePage } from '../src/routes/TaskHomePage';

jest.mock('../src/application/document-workspace', () => ({ documentApi: { getDocument: jest.fn(), listVersionFiles: jest.fn(), downloadVersionFile: jest.fn(), getRootFolder: jest.fn(), listFolderChildren: jest.fn(), listDocuments: jest.fn() } }));

const SALES_PROFILE = 'profile-sales', OFFICE_PROFILE = 'profile-office', REVIEW_PROFILE = 'profile-review';
const module = (name: string, presentation = 'available') => ({ module: name, presentation });
const profiles = [
  { id: SALES_PROFILE, key: 'sales-context', label: '営業・文脈', archetype: 'context', primaryGrouping: 'context', defaultSort: 'due_at', initialModule: 'document', modules: [module('document', 'prominent')] },
  { id: OFFICE_PROFILE, key: 'office-queue', label: '事務・キュー', archetype: 'queue', primaryGrouping: 'work_type', defaultSort: 'due_at', initialModule: 'document', modules: [module('document', 'visible')] },
  { id: REVIEW_PROFILE, key: 'review-queue', label: '審査・キュー', archetype: 'queue', primaryGrouping: 'work_type', defaultSort: 'due_at', initialModule: 'evidence', modules: [module('evidence', 'prominent')] },
];
const responsibility = (id: string, principal: string, roleLabel: string, unitLabel: string, workViewProfileId: string) => ({ id, kind: 'role_assignment', principal, roleId: `role-${id}`, roleLabel, unitId: `unit-${id}`, unitLabel, actions: ['queue.read', 'work.read', 'work.claim', 'work.edit', 'context.read'], validFrom: '2026-10-01T00:00:00Z', validUntil: null, sourceAssignmentId: null, delegator: null, workViewProfileId });
const session = (principalId: string, acting: ReturnType<typeof responsibility>) => ({ principalId, displayName: '合成', actingAssignmentId: acting.id, responsibilities: [acting], canManageOrganization: false, policyRevision: 1, capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: true } });
const sales = session('sales-01', responsibility('sales', 'sales-01', '営業', '営業店', SALES_PROFILE));
const office = session('office-01', responsibility('office', 'office-01', '事務処理', '事務', OFFICE_PROFILE));
const reviewer = session('review-01', responsibility('review', 'review-01', '審査', '融資審査', REVIEW_PROFILE));
const base = { contextId: 'context-c', attemptNumber: 1, revision: 1, canEdit: false, canSubmit: false, canComplete: false, completionActionId: null, canHold: false, holdActionId: null, canResume: false, resumeActionId: null, canReturn: false, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false, returnTransition: null, returnInstructionId: null, handoffSnapshotId: null, requiredRoleId: null, claimAssignmentId: null, canAssign: false, assignment: null, dueAt: null, attention: [], contextTitle: null };
const task = (id: string, extra: Record<string, unknown>) => ({ ...base, id, attemptId: `${id}-attempt`, title: '営業内容整理', stepLabel: '営業内容整理', state: 'ready', canClaim: true, workTypeId: 'type-sales', workTypeLabel: '営業内容整理', ...extra });
const overdue = { kind: 'overdue', sourceId: null, dueAt: '2026-10-07T08:00:00Z' };
const contextC = { id: 'context-c', kind: 'request', title: '合成依頼C・住所変更届', ownerUnitId: 'unit-sales', progress: [{ taskId: 'c-sales', stepLabel: '営業内容整理', workTypeId: 'type-sales', state: 'ready', attemptNumber: 1, dueAt: '2026-10-07T08:00:00Z', assigned: false }], canReadHistory: true, ownTaskIds: [], attentionCount: 1 };

function setup(entry: string, actor: unknown, tasks: unknown[], contexts: unknown[] = []) {
  jest.spyOn(workApi, 'getSession').mockResolvedValue(actor as never);
  jest.spyOn(workApi, 'listWorkViewProfiles').mockResolvedValue({ items: profiles, nextCursor: null } as never);
  jest.spyOn(workApi, 'listTasks').mockResolvedValue({ items: tasks, nextCursor: null } as never);
  jest.spyOn(workApi, 'listWorkContexts').mockResolvedValue({ items: contexts, nextCursor: null } as never);
  jest.spyOn(workApi, 'getTask').mockRejectedValue(new WorkApiError(404, 'WORK_ITEM_NOT_FOUND'));
  jest.spyOn(workApi, 'listEvidence').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listFindings').mockResolvedValue({ items: [], nextCursor: null });
  jest.spyOn(workApi, 'listDecisions').mockResolvedValue({ items: [], nextCursor: null });
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({ getParentRoute: () => root, path: '/tasks', validateSearch: validateTaskSearch, component: TaskHomePage });
  const router = createRouter({ routeTree: root.addChildren([route]), history: createMemoryHistory({ initialEntries: [entry] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { router };
}
afterEach(() => jest.restoreAllMocks());

test('without an explicit view, the responsibility profile picks the projection (presentation only)', async () => {
  setup('/tasks', office, []);
  await waitFor(() => expect(workApi.listTasks).toHaveBeenCalledWith('queue', undefined));
  expect(await screen.findByText(/既定：事務・キュー/)).toBeVisible();
  expect(workApi.listWorkContexts).not.toHaveBeenCalled();
  jest.restoreAllMocks();
});

test('sales sees authorized contexts first and opens a context overview without private detail', async () => {
  const cSales = task('c-sales', { dueAt: '2026-10-07T08:00:00Z', attention: [overdue], contextTitle: contextC.title });
  const other = task('hidden', { contextId: null, title: '事務内容確認', stepLabel: '事務内容確認', workTypeId: 'type-office', workTypeLabel: '事務内容確認' });
  const { router } = setup('/tasks', sales, [cSales, other], [contextC]);
  await waitFor(() => expect(workApi.listTasks).toHaveBeenCalledWith('context', undefined));
  const group = await screen.findByRole('region', { name: `文脈 ${contextC.title}` });
  expect(within(group).getByRole('button', { name: /依頼/ })).toBeVisible();
  expect(within(group).getByRole('button', { name: /期限超過/ })).toBeVisible();
  // A task whose context identity is not disclosed is listed without a customer label.
  expect(within(screen.getByRole('region', { name: '文脈を表示しないタスク' })).getByRole('button', { name: /事務内容確認/ })).toBeVisible();
  await userEvent.click(within(group).getByRole('button', { name: new RegExp(contextC.title) }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ contextId: contextC.id }));
  const overview = await screen.findByRole('region', { name: '文脈の概要' });
  expect(within(overview).getByRole('heading', { name: contextC.title })).toBeVisible();
  expect(within(overview).getByRole('list', { name: '工程の進捗' })).toHaveTextContent('営業内容整理 · 担当待ち · 試行 1 · 未割当');
  expect(workApi.getTask).not.toHaveBeenCalled();
  const history = jest.spyOn(workApi, 'getWorkContextHistory').mockResolvedValue({ contextId: contextC.id, entries: [{ kind: 'assigned', occurredAt: '2026-10-07T09:00:00Z' }] });
  await userEvent.click(within(overview).getByRole('button', { name: '文脈の履歴を表示' }));
  expect(await within(overview).findByRole('list', { name: '文脈の履歴' })).toHaveTextContent('担当変更');
  expect(history).toHaveBeenCalledWith(contextC.id);
  await userEvent.click(within(overview).getByRole('button', { name: /営業内容整理（引受可能）を開く/ }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ taskId: cSales.id }));
  expect(await screen.findByRole('list', { name: 'このタスクの注意' })).toHaveTextContent('期限超過');
});

test('queue groups by WorkType and separates own work from eligible-only rows', async () => {
  const own = task('office-own', { title: '事務内容確認', stepLabel: '事務内容確認', workTypeId: 'type-office', workTypeLabel: '事務内容確認', state: 'active', canClaim: false, assignment: { principalId: 'office-01', displayName: '事務担当（模擬）', actingAssignmentId: 'office', actingKind: 'role_assignment', roleLabel: '事務処理', delegatorPrincipalId: null, responsibilityEffective: true }, contextTitle: '合成案件A・設備更新相談' });
  const ready = task('office-ready', { contextId: null, title: '事務内容確認', stepLabel: '事務内容確認', workTypeId: 'type-office', workTypeLabel: '事務内容確認' });
  const review = task('review-ready', { contextId: null, title: '審査内容確認', stepLabel: '審査内容確認', workTypeId: 'type-review', workTypeLabel: '審査内容確認', dueAt: '2026-10-07T12:00:00Z', attention: [{ kind: 'due_soon', sourceId: null, dueAt: '2026-10-07T12:00:00Z' }] });
  const { router } = setup('/tasks?view=queue', office, [own, ready, review]);
  const types = await screen.findByLabelText('業務の種類');
  expect(within(screen.getByRole('region', { name: '自分の担当' })).getByRole('button', { name: /事務内容確認/ })).toHaveTextContent('合成案件A・設備更新相談');
  const claimable = screen.getByRole('region', { name: '引受可能' });
  expect(within(claimable).getAllByRole('button')).toHaveLength(2);
  expect(within(claimable).getByRole('button', { name: /審査内容確認/ })).toHaveTextContent('期限間近');
  expect(claimable).not.toHaveTextContent('合成案件');
  await userEvent.click(within(types).getByRole('button', { name: '審査内容確認' }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ view: 'queue', workTypeId: 'type-review' }));
  expect(within(screen.getByRole('region', { name: '引受可能' })).getAllByRole('button')).toHaveLength(1);
  expect(screen.queryByRole('region', { name: '自分の担当' })).not.toBeInTheDocument();
});

test('a newly assigned attention is acknowledged explicitly and never treated as completion', async () => {
  const assigned = task('c-sales', { state: 'active', canClaim: false, attention: [{ kind: 'newly_assigned', sourceId: 'period-1', dueAt: null }, overdue], assignment: { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'sales', actingKind: 'role_assignment', roleLabel: '営業', delegatorPrincipalId: null, responsibilityEffective: true } });
  setup(`/tasks?view=queue&taskId=${assigned.id}`, sales, [assigned]);
  const badges = await screen.findByRole('list', { name: 'このタスクの注意' });
  expect(badges).toHaveTextContent('新しい割当');
  expect(badges).toHaveTextContent('期限超過');
  const seen = jest.spyOn(workApi, 'markAttentionSeen').mockResolvedValue({ taskId: assigned.id, attemptId: assigned.attemptId, evaluatedAt: '2026-10-07T09:00:00Z', items: [overdue] } as never);
  const calls = jest.mocked(workApi.listTasks).mock.calls.length;
  await userEvent.click(screen.getByRole('button', { name: '確認済みにする' }));
  expect(await screen.findByText(/作業は完了していません/)).toBeVisible();
  expect(seen).toHaveBeenCalledWith(assigned.id, 'period-1');
  await waitFor(() => expect(jest.mocked(workApi.listTasks).mock.calls.length).toBeGreaterThan(calls));
});

test('the review profile opens Evidence first; the module choice grants nothing', async () => {
  const claimed = task('review-own', { title: '審査内容確認', stepLabel: '審査内容確認', state: 'active', canClaim: false, workTypeId: 'type-review', workTypeLabel: '審査内容確認' });
  setup(`/tasks?taskId=${claimed.id}`, reviewer, [claimed]);
  await waitFor(() => expect(workApi.listTasks).toHaveBeenCalledWith('queue', undefined));
  const modules = await screen.findByLabelText('文脈モジュール');
  await waitFor(() => expect(within(modules).getByRole('button', { name: '根拠' })).toHaveAttribute('aria-pressed', 'true'));
});

test('an explicit queue view still applies the review profile and its initial module', async () => {
  const claimed = task('review-own', { title: '審査内容確認', stepLabel: '審査内容確認', state: 'active', canClaim: false, workTypeId: 'type-review', workTypeLabel: '審査内容確認' });
  setup(`/tasks?view=queue&taskId=${claimed.id}`, reviewer, [claimed]);
  await waitFor(() => expect(workApi.listWorkViewProfiles).toHaveBeenCalled());
  const modules = await screen.findByLabelText('文脈モジュール');
  await waitFor(() => expect(within(modules).getByRole('button', { name: '根拠' })).toHaveAttribute('aria-pressed', 'true'));
});

test('context groups and undisclosed tasks are both ordered by the earliest due instant', async () => {
  const later = task('c-later', { dueAt: '2026-10-08T09:00:00Z', contextTitle: contextC.title, title: '事務内容確認', stepLabel: '事務内容確認' });
  const none = task('c-none', { dueAt: null, contextTitle: contextC.title });
  const sooner = task('c-sooner', { dueAt: '2026-10-07T10:00:00Z', contextTitle: contextC.title, title: '審査内容確認', stepLabel: '審査内容確認' });
  const hiddenLate = task('h-late', { contextId: null, dueAt: '2026-10-09T09:00:00Z', title: '事務内容確認', stepLabel: '事務内容確認' });
  const hiddenSoon = task('h-soon', { contextId: null, dueAt: '2026-10-07T11:00:00Z', title: '審査内容確認', stepLabel: '審査内容確認' });
  setup('/tasks', sales, [later, none, sooner, hiddenLate, hiddenSoon], [contextC]);
  const group = await screen.findByRole('region', { name: `文脈 ${contextC.title}` });
  const rows = within(group).getAllByRole('button').slice(1).map((button) => button.textContent ?? '');
  expect(rows[0]).toContain('審査内容確認');
  expect(rows[1]).toContain('事務内容確認');
  expect(rows[2]).toContain('営業内容整理');
  const undisclosed = within(screen.getByRole('region', { name: '文脈を表示しないタスク' })).getAllByRole('button').map((button) => button.textContent ?? '');
  expect(undisclosed[0]).toContain('審査内容確認');
  expect(undisclosed[1]).toContain('事務内容確認');
});
