import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Link, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { workApi, WorkApiError } from '../src/api/work-api';
import { OrganizationProvider } from '../src/application/organization-context';
import { AVAILABILITY_RECHECK_MS, landingScreen, workAvailabilityQuery } from '../src/application/work-availability';
import { AppShell } from '../src/components/app-shell/AppShell';

// The approved design: when the server offers the Work API, the primary navigation
// always has タスク/文書/検索 and the landing screen is タスク. A document-only server
// (no /v1/organization routes, so 404) keeps the document screens as before.

const session = { principalId: 'sales-01', displayName: '営業担当（模擬）', actingAssignmentId: 'assignment-sales', responsibilities: null, canManageOrganization: false, policyRevision: null, capabilities: { nativeWorkspace: false, agent: false, search: false, fileUpload: false, return: true } };
const ORGANIZATION_LINKS = ['タスク', '文書', '編集作業', '文書履歴', '検索'];
// Document mode keeps its decorative icons (aria-hidden), as before.
const DOCUMENT_LINKS = ['▯文書', '✎編集作業', '文書履歴'];

function setup(entry: string, client = new QueryClient({ defaultOptions: { queries: { retry: false } } })) {
  const root = createRootRoute({ component: Outlet });
  const documents = createRoute({ getParentRoute: () => root, path: '/documents', component: () => <AppShell><h1>文書一覧</h1><Link to="/local-workspaces">別の画面へ</Link></AppShell> });
  const local = createRoute({ getParentRoute: () => root, path: '/local-workspaces', component: () => <AppShell activeNavigation="local-workspaces"><h1>ローカルWorkspace</h1></AppShell> });
  const tasks = createRoute({ getParentRoute: () => root, path: '/tasks', component: () => <AppShell activeNavigation="tasks"><h1>タスク</h1></AppShell> });
  const search = createRoute({ getParentRoute: () => root, path: '/search', component: () => <AppShell activeNavigation="search"><h1>検索</h1></AppShell> });
  const router = createRouter({ routeTree: root.addChildren([documents, local, tasks, search]), history: createMemoryHistory({ initialEntries: [entry] }) });
  render(<OrganizationProvider><QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider></OrganizationProvider>);
  return { router, client };
}
const links = () => within(screen.getByRole('navigation', { name: 'メインナビゲーション' })).getAllByRole('link').map((link) => link.textContent?.trim());

afterEach(() => jest.restoreAllMocks());

test('Work APIを提供するserverでは、文書画面を直接開いても主ナビゲーションにタスク・文書・検索を出す', async () => {
  const probe = jest.spyOn(workApi, 'getSession').mockResolvedValue(session);
  setup('/documents');
  await screen.findByRole('heading', { name: '文書一覧' });
  await waitFor(() => expect(links()).toEqual(ORGANIZATION_LINKS));
  const navigation = within(screen.getByRole('navigation', { name: 'メインナビゲーション' }));
  expect(navigation.getByRole('link', { name: 'タスク' })).toHaveAttribute('href', '/tasks');
  expect(navigation.getByRole('link', { name: '検索' })).toHaveAttribute('href', '/search');
  expect(navigation.getByRole('link', { name: '文書' })).toHaveAttribute('aria-current', 'page');
  expect(screen.getByRole('link', { name: 'タスクホーム' })).toHaveAttribute('href', '/tasks');
  expect(probe).toHaveBeenCalledTimes(1);
});

test('メニューのタスク・検索で移動でき、移動しても判定を取り直さない', async () => {
  const probe = jest.spyOn(workApi, 'getSession').mockResolvedValue(session);
  const { router } = setup('/documents');
  await waitFor(() => expect(links()).toEqual(ORGANIZATION_LINKS));
  await userEvent.click(screen.getByRole('link', { name: 'タスク' }));
  await screen.findByRole('heading', { name: 'タスク' });
  expect(router.state.location.pathname).toBe('/tasks');
  await userEvent.click(screen.getByRole('link', { name: '検索' }));
  await screen.findByRole('heading', { name: '検索' });
  await userEvent.click(screen.getByRole('link', { name: '文書' }));
  await screen.findByRole('heading', { name: '文書一覧' });
  expect(links()).toEqual(ORGANIZATION_LINKS);
  await userEvent.click(screen.getByRole('link', { name: '別の画面へ' }));
  await screen.findByRole('heading', { name: 'ローカルWorkspace' });
  expect(links()).toEqual(ORGANIZATION_LINKS);
  expect(probe).toHaveBeenCalledTimes(1);
});

test('文書だけのserver（Work APIが404）では、今までどおり文書のメニューだけを出し、取り直さない', async () => {
  const probe = jest.spyOn(workApi, 'getSession').mockRejectedValue(new WorkApiError(404, 'request_failed'));
  const { client } = setup('/documents');
  await waitFor(() => expect(client.getQueryData(workAvailabilityQuery.queryKey)).toBe('unavailable'));
  expect(links()).toEqual(DOCUMENT_LINKS);
  expect(screen.getByRole('link', { name: '文書管理ホーム' })).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: 'タスクホーム' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('link', { name: '別の画面へ' }));
  await screen.findByRole('heading', { name: 'ローカルWorkspace' });
  expect(links()).toEqual(DOCUMENT_LINKS);
  expect(probe).toHaveBeenCalledTimes(1);
});

test.each([
  ['通信できない', new WorkApiError(0, 'network_unavailable')],
  ['desktop shellに接続先が無い（503）', new WorkApiError(503, 'request_failed')],
  ['serverの一時的な失敗（500）', new WorkApiError(500, 'request_failed')],
  ['応答の形が不正', new WorkApiError(200, 'invalid_response')],
])('Work APIの有無が分からない（%s）ときは文書のメニューのまま、次の画面で確かめ直す', async (_, error) => {
  const probe = jest.spyOn(workApi, 'getSession').mockRejectedValueOnce(error).mockResolvedValue(session);
  const { client } = setup('/documents');
  await waitFor(() => expect(client.getQueryState(workAvailabilityQuery.queryKey)?.status).toBe('error'));
  expect(links()).toEqual(DOCUMENT_LINKS);
  await userEvent.click(screen.getByRole('link', { name: '別の画面へ' }));
  await screen.findByRole('heading', { name: 'ローカルWorkspace' });
  await waitFor(() => expect(links()).toEqual(ORGANIZATION_LINKS));
  expect(probe).toHaveBeenCalledTimes(2);
});

test('分からないまま同じ画面に留まっても、一定の間隔で確かめ直し、Work APIが使えるようになればメニューに出す', async () => {
  jest.useFakeTimers();
  try {
    const probe = jest.spyOn(workApi, 'getSession').mockRejectedValueOnce(new WorkApiError(502, 'request_failed')).mockResolvedValue(session);
    const { client } = setup('/documents');
    await waitFor(() => expect(client.getQueryState(workAvailabilityQuery.queryKey)?.status).toBe('error'));
    expect(links()).toEqual(DOCUMENT_LINKS);
    await act(async () => { jest.advanceTimersByTime(AVAILABILITY_RECHECK_MS); });
    await waitFor(() => expect(links()).toEqual(ORGANIZATION_LINKS));
    expect(screen.getByRole('heading', { name: '文書一覧' })).toBeInTheDocument();
    expect(probe).toHaveBeenCalledTimes(2);
    // Once known, it is not asked again.
    await act(async () => { jest.advanceTimersByTime(AVAILABILITY_RECHECK_MS * 3); });
    expect(probe).toHaveBeenCalledTimes(2);
  } finally {
    jest.useRealTimers();
  }
});

test('文書だけのserverと分かれば、確かめ直さない', async () => {
  jest.useFakeTimers();
  try {
    const probe = jest.spyOn(workApi, 'getSession').mockRejectedValue(new WorkApiError(404, 'request_failed'));
    const { client } = setup('/documents');
    await waitFor(() => expect(client.getQueryData(workAvailabilityQuery.queryKey)).toBe('unavailable'));
    await act(async () => { jest.advanceTimersByTime(AVAILABILITY_RECHECK_MS * 3); });
    expect(probe).toHaveBeenCalledTimes(1);
    expect(links()).toEqual(DOCUMENT_LINKS);
  } finally {
    jest.useRealTimers();
  }
});

test.each([['/tasks', 'タスク'], ['/search', '検索']])('%s では画面自身がWork APIを読むので、shellは判定の要求を出さない', async (entry, heading) => {
  const probe = jest.spyOn(workApi, 'getSession').mockResolvedValue(session);
  setup(entry);
  await screen.findByRole('heading', { name: heading });
  expect(links()).toEqual(ORGANIZATION_LINKS);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  expect(probe).not.toHaveBeenCalled();
});

describe('最初の画面（/）', () => {
  const client = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });

  test('Work APIを提供するserverではタスク', async () => {
    jest.spyOn(workApi, 'getSession').mockResolvedValue(session);
    await expect(landingScreen(client())).resolves.toBe('tasks');
  });

  test.each([
    ['文書だけのserver（404）', new WorkApiError(404, 'request_failed')],
    ['通信できない', new WorkApiError(0, 'network_unavailable')],
    ['desktop shellに接続先が無い（503）', new WorkApiError(503, 'request_failed')],
    ['予期しない失敗', new Error('unexpected')],
  ])('%sでは今までどおり文書', async (_, error) => {
    jest.spyOn(workApi, 'getSession').mockRejectedValue(error);
    await expect(landingScreen(client())).resolves.toBe('documents');
  });

  test('判定が上限時間内に終わらなければ文書を開き、遅れて分かった結果はメニューに使う', async () => {
    let answer: (value: typeof session) => void = () => undefined;
    jest.spyOn(workApi, 'getSession').mockReturnValue(new Promise((resolve) => { answer = resolve; }));
    const queries = client();
    await expect(landingScreen(queries, 20)).resolves.toBe('documents');
    answer(session);
    await waitFor(() => expect(queries.getQueryData(workAvailabilityQuery.queryKey)).toBe('available'));
  });

  test('判定の結果は画面のメニューと共有し、二度は要求しない', async () => {
    const probe = jest.spyOn(workApi, 'getSession').mockResolvedValue(session);
    const queries = client();
    await expect(landingScreen(queries)).resolves.toBe('tasks');
    setup('/documents', queries);
    await waitFor(() => expect(links()).toEqual(ORGANIZATION_LINKS));
    expect(probe).toHaveBeenCalledTimes(1);
  });
});
