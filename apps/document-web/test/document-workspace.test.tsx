import { documentAccessPolicyOperations, sendDocumentAccessPolicyOperation } from '../src/application/document-access-policy';
import { folderAccessPolicyOperations, sendFolderAccessPolicyOperation } from '../src/application/document-folder-access-policy';
import { refreshFolderMoveReads } from '../src/application/document-folder-move';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { defaultStringifySearch, createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { documentApi } from '../src/application/document-workspace';
import type { DocumentDetail, DocumentList, Version } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { DocumentListRouteError } from '../src/routes/DocumentListRouteError';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';
import { metadataOperations } from '../src/application/document-metadata';
import { rootFolderOperations } from '../src/application/document-root-folder';
import { folderRenameOperations } from '../src/application/document-folder-rename';
import { readCreationReceipt, saveCreationReceipt, clearCreationReceipt } from '../src/application/document-registration';

jest.mock('../src/routes/DocumentWorkspace.module.css', () => ({ timestampCell: 'timestamp-cell' }));

jest.mock('../src/application/document-workspace', () => ({
  documentApi: {
    getSession: jest.fn(),
    getRootFolder: jest.fn(),
    listFolderChildren: jest.fn(),
    listDocuments: jest.fn(),
    getDocument: jest.fn(),
    listDocumentVersions: jest.fn(),
    getDocumentVersion: jest.fn(),
    listDocumentRevisions: jest.fn(),
    getDocumentHistory: jest.fn(),
    listVersionFiles: jest.fn(),
    getDocumentAccessPolicy: jest.fn(),
    compareDocumentVersions: jest.fn(),
    compareDocumentRevisions: jest.fn(),
    publishVersion: jest.fn(),
    schedulePublication: jest.fn(),
    cancelPublicationSchedule: jest.fn(),
    setDocumentAccessPolicy: jest.fn(),
    createVersion: jest.fn(),
    updateWorkingVersion: jest.fn(),
    getVersionEditManifest: jest.fn(),
    prepareVersionUpload: jest.fn(),
    rebaseWorkingVersion: jest.fn(),
    downloadVersionFile: jest.fn(),
  },
}));

const documentId = '00000000-0000-4000-8000-000000000010';
const versionId = '00000000-0000-4000-8000-000000000011';
const baseVersionId = '00000000-0000-4000-8000-000000000012';
const revisionId = '00000000-0000-4000-8000-000000000013';
const newerRevisionId = '00000000-0000-4000-8000-000000000014';
const folderId = '00000000-0000-4000-8000-000000000015';
const reviewFolderId = '00000000-0000-4000-8000-000000000018';
const operationAvailable = { status: 'available' as const };
const operationDenied = { status: 'disabled' as const, reason: 'permission' as const };

function mockApi() {
  const api = documentApi as unknown as { [Key in keyof typeof documentApi]: jest.Mock };
  Object.values(api).forEach((mock) => mock.mockReset());
  api.getRootFolder.mockResolvedValue({ folderId, name: 'ルート', revision: 1, parentFolderId: null, capabilities: {} });
  api.listFolderChildren.mockResolvedValue({ items: [], nextCursor: null, capabilities: {} });
  api.listDocuments.mockImplementation(query => {
    if (query.view !== 'history') return Promise.resolve({ view: 'published', items: [listItem('published')], nextCursor: null });
    const { currentVersionId: ignored, ...historyItem } = listItem('published'); void ignored;
    return Promise.resolve({ view: 'history', items: [{ ...historyItem, lifecycleState: 'published', ended: false }], nextCursor: null });
  });
  api.getDocument.mockResolvedValue(documentDetail('published'));
  api.listDocumentVersions.mockResolvedValue({ items: [version()], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue(versionDetail());
  api.listDocumentRevisions.mockResolvedValue({ items: [revision(newerRevisionId, versionId, 2), revision(revisionId, baseVersionId, 1)], nextCursor: null });
  api.getDocumentHistory.mockResolvedValue({ items: [], nextCursor: null });
  api.listVersionFiles.mockResolvedValue({ items: [] });
  api.getDocumentAccessPolicy.mockResolvedValue(policyRead());
  api.compareDocumentRevisions.mockResolvedValue(comparison() as never);
  api.createVersion.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, documentId: id, targetVersionId: body.targetVersionId, versionNo: 3, baseVersionId, resultingRevision: 8 }));
  api.prepareVersionUpload.mockImplementation(() => ({ body: new Blob(['wire']), contentType: 'multipart/form-data; boundary=fixed' }));
  api.getVersionEditManifest.mockImplementation((id, sourceVersionId, purpose) => Promise.resolve({ documentId: id, sourceVersionId, purpose, documentRevision: 7, title: '受入手順', items: [{ contentItemId: 'primary-item', logicalPath: 'primary', ordinal: 0, representations: [{ role: 'authoritative', representationId: 'primary-representation', fileId: 'source-file', mediaType: 'text/plain', originalFilename: 'source.txt', sizeBytes: 8 }] }] }));
  api.publishVersion.mockResolvedValue({ publishOperationId: 'pub', documentId, documentVersionId: versionId, resultingDocumentRevision: 8, publishedAt: '2026-10-01T02:00:00Z' });
  api.schedulePublication.mockResolvedValue({ publishOperationId: 'pub', documentId, targetVersionId: versionId, acceptedRevision: 8, scheduledPublishAt: '2026-10-02T02:00:00Z' });
  api.setDocumentAccessPolicy.mockImplementation((id, body) => Promise.resolve({ operationId: body.operationId, resourceId: id, resultingRevision: body.expectedPolicyRevision + 1, changed: true, occurredAt: '2026-10-01T02:00:00Z' }));
  api.downloadVersionFile.mockResolvedValue(new Blob(['original']));
  return api;
}

function renderAt(entry: string, listError?: Error) {
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage, errorComponent: DocumentListRouteError, ...(listError ? { beforeLoad: () => { throw listError; } } : {}) });
  const detail = createRoute({ getParentRoute: () => root, path: '/documents/$documentId', validateSearch: validateDetailSearch, component: DocumentDetailPage });
  const router = createRouter({
    routeTree: root.addChildren([list, detail]),
    history: createMemoryHistory({ initialEntries: [entry] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  const result = render(<QueryClientProvider client={client}><RouterProvider router={router as never} /></QueryClientProvider>);
  return { ...result, router, client };
}

test('Home timestamp labels its actual local zone and preserves the machine-readable instant', async () => {
  mockApi();
  const { container } = renderAt('/documents?view=published');
  await screen.findByRole('button', { name: /受入手順/ });
  const instant = '2026-10-01T01:00:00Z';
  const time = container.querySelector(`time[datetime="${instant}"]`);
  const local = new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium', timeStyle: 'short' });
  const minutes = -new Date(instant).getTimezoneOffset();
  const offset = `${minutes < 0 ? '-' : '+'}${String(Math.floor(Math.abs(minutes) / 60)).padStart(2, '0')}:${String(Math.abs(minutes) % 60).padStart(2, '0')}`;
  expect(time).toHaveTextContent(`${local.format(new Date(instant))} (${local.resolvedOptions().timeZone}, UTC${offset})`);
  expect(time).toHaveAttribute('datetime', instant);
  expect(time!.closest('[role="cell"]')).toHaveClass('timestamp-cell');
  expect(within(screen.getByRole('complementary', { name: '選択中の文書' })).getByText(time!.textContent!)).toBeVisible();
});

test('Detail timestamp visibly retains Tokyo time with its zone and offset', async () => {
  mockApi();
  renderAt(`/documents/${documentId}?view=published&tab=overview`);
  const timestamps = await screen.findAllByText('2026/10/01 10:00 (Asia/Tokyo, UTC+09:00)');
  expect(timestamps).toHaveLength(2);
  timestamps.forEach(timestamp => expect(timestamp).toBeVisible());
});

test('Home keeps distinct fall-back instants and labels their respective local offsets', async () => {
  const api = mockApi();
  const instants = ['2026-11-01T05:30:00Z', '2026-11-01T06:30:00Z'];
  api.listDocuments.mockResolvedValue({ view: 'published', nextCursor: null, items: instants.map((value, index) => ({
    ...listItem('published'), documentId: `fold-${index}`, title: `秋の公開 ${index + 1}`,
    displayTimestamp: { kind: 'revisionCreatedAt', value },
  })) });
  const { container } = renderAt('/documents?view=published&panel=closed');
  await screen.findByRole('button', { name: '秋の公開 1' });
  const times = instants.map(instant => container.querySelector(`time[datetime="${instant}"]`)!);
  const local = new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium', timeStyle: 'short' });
  for (const [index, instant] of instants.entries()) {
    expect(times[index]).toHaveAttribute('datetime', instant);
    expect(times[index]).toHaveTextContent(local.format(new Date(instant)));
    expect(times[index]).toHaveTextContent(`${local.resolvedOptions().timeZone}, UTC`);
  }
  if (local.resolvedOptions().timeZone === 'America/New_York') {
    expect(times[0]).toHaveTextContent('2026/11/01 1:30 (America/New_York, UTC-04:00)');
    expect(times[1]).toHaveTextContent('2026/11/01 1:30 (America/New_York, UTC-05:00)');
  }
});

test('folder navigation remains visible when the document list request fails', async () => {
  const api = mockApi();
  api.listDocuments.mockRejectedValue({ type: 'about:blank', title: 'Invalid query', status: 400,
    code: 'VALIDATION_FAILED', traceId: 'synthetic', retryable: false });
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, name: 'PoC Shared', revision: 1, parentFolderId: folderId }], nextCursor: null, capabilities: {} });
  renderAt('/documents?view=published');
  expect(await screen.findByRole('button', { name: 'PoC Shared' })).toBeVisible();
  expect(await screen.findByRole('alert')).toHaveTextContent('入力内容を確認してください');
});

test('pending folder query keeps the table empty-data reference stable across rerenders', async () => {
  const api = mockApi();
  api.listDocuments.mockImplementation((query) => query.folderId ? new Promise(() => {})
    : Promise.resolve({ view: 'published', items: [listItem('published')], nextCursor: null }));
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, name: 'PoC Shared', revision: 1, parentFolderId: folderId }], nextCursor: null, capabilities: {} });
  const table = require('@tanstack/react-table') as typeof import('@tanstack/react-table');
  const original = table.useReactTable;
  const pendingData: unknown[][] = [];
  let navigating = false;
  jest.spyOn(table, 'useReactTable').mockImplementation(options => {
    if (navigating && options.data.length === 0) {
      pendingData.push(options.data);
      // Fail promptly rather than letting an unstable-reference reset loop starve the test runner.
      if (pendingData.length > 20) throw new Error('Pending table data caused excessive rerenders');
    }
    return original(options);
  });
  const user = userEvent.setup();
  const view = renderAt('/documents?view=published');
  await screen.findByRole('button', { name: /受入手順/ });
  const folder = await screen.findByRole('button', { name: 'PoC Shared' });
  navigating = true;
  await user.click(folder);
  await waitFor(() => expect(api.listDocuments).toHaveBeenCalledWith(expect.objectContaining({ folderId: reviewFolderId })));
  view.client.setQueryData(['folder-tree', 'root'], { folderId, name: 'ルート更新', revision: 2, parentFolderId: null, capabilities: {} });
  await waitFor(() => expect(pendingData.length).toBeGreaterThan(1));
  expect(new Set(pendingData).size).toBe(1);
});

test('list filters stay in the URL and the detail return restores the selected list context', async () => {
  const api = mockApi();
  api.listDocuments.mockResolvedValue({ view: 'authoring', items: [listItem('authoring')], nextCursor: null });
  api.getDocument.mockResolvedValue(documentDetail('authoring'));
  const user = userEvent.setup();
  renderAt('/documents?view=authoring&titleContains=manual&sort=title_asc&pageSize=25');

  const rowTitle = await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments).toHaveBeenCalledWith(expect.objectContaining({
    view: 'authoring', titleContains: 'manual', sort: 'title_asc', pageSize: 25,
  }));
  expect(screen.getAllByText('下書き').length).toBeGreaterThan(0);
  expect(screen.getAllByText('Version 3').length).toBeGreaterThan(0);

  await user.click(rowTitle);
  const openDocument = await screen.findByRole('button', { name: /詳細を開く/ });
  expect(within(screen.getByRole('complementary', { name: '選択中の文書' })).getByText('受入手順')).toBeVisible();
  await user.click(openDocument);
  expect(await screen.findByRole('heading', { name: '受入手順' })).toBeVisible();
  expect(screen.getByRole('tab', { name: 'アクセス' })).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  const returnedRow = await screen.findByRole('button', { name: /受入手順/ });
  expect(await screen.findByRole('button', { name: /詳細を開く/ })).toBeVisible();
  await waitFor(() => expect(returnedRow).toHaveFocus());
  expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({
    view: 'authoring', titleContains: 'manual', sort: 'title_asc', pageSize: 25,
  }));
});

test('folder selection scopes the document list request', async () => {
  const api = mockApi();
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, name: '審査', revision: 3, parentFolderId: folderId, capabilities: {} }], nextCursor: null, capabilities: {} });
  api.listDocuments.mockResolvedValue({ view: 'published', items: [listItem('published')], nextCursor: null });
  const user = userEvent.setup();
  renderAt('/documents?view=published');

  await user.click(await screen.findByRole('button', { name: '審査' }));

  await waitFor(() => expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({
    view: 'published', folderId: reviewFolderId,
  })));
});

test('実Playwrightのexact role名で編集一覧の詳細を開き、装飾矢印を読上げない', async () => {
  const matches = require('./playwright-role-matcher.cjs')() as (root: Document, role: string, name: string) => HTMLElement[];
  const computedStyle = window.getComputedStyle.bind(window);
  // jsdom has no pseudo-element styles; the production arrow is actual DOM text.
  const style = jest.spyOn(window, 'getComputedStyle').mockImplementation(element => computedStyle(element));
  try {
    const api = mockApi();
    api.listDocuments.mockResolvedValue({ view: 'authoring', items: [listItem('authoring')], nextCursor: null });
    api.getDocument.mockResolvedValue(documentDetail('authoring'));
    const { router } = renderAt('/documents?view=authoring&panel=closed');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: /受入手順/ }));
    const visibleButton = await screen.findByRole('button', { name: /詳細を開く/ });
    expect(visibleButton).toHaveTextContent('詳細を開く →');
    const exact = matches(document, 'button', '詳細を開く');
    expect(exact).toEqual([visibleButton]);
    expect(visibleButton).toHaveAccessibleName('詳細を開く');
    await user.click(exact[0]!);
    expect(await screen.findByRole('heading', { name: '受入手順', level: 1 })).toBeVisible();
    expect(router.state.location.pathname).toBe(`/documents/${documentId}`);
    expect(router.state.location.search).toMatchObject({ view: 'authoring' });
  } finally { style.mockRestore(); }
});

test('versions keep WORKING content separate from numbered revisions', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('authoring'));
  api.listDocumentVersions.mockResolvedValue({ items: [
    version({ versionNo: 3 }),
    version({ versionId: baseVersionId, versionNo: 2, lifecycleState: 'published', isCurrent: true }),
  ], nextCursor: null });
  api.listDocumentRevisions.mockResolvedValue({ items: [revision(newerRevisionId, versionId, 2), revision(revisionId, baseVersionId, 1)], nextCursor: null });
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=authoring&tab=versions`);

  const versionPanel = await screen.findByRole('tabpanel', { name: '版・改訂' });
  expect(await within(versionPanel).findByRole('button', { name: /WORKING · 版 3/ })).toBeInTheDocument();
  expect(await within(versionPanel).findByRole('button', { name: /版 2.*現行版/ })).toBeInTheDocument();
  expect(within(versionPanel).getByRole('heading', { name: '正式改訂' })).toBeInTheDocument();
  expect(within(versionPanel).getAllByText('2.0').length).toBeGreaterThan(0);
  expect(within(versionPanel).getAllByText('1.0').length).toBeGreaterThan(0);

  await user.click(within(versionPanel).getByRole('button', { name: /版 2.*現行版/ }));
  expect(api.getDocumentVersion).toHaveBeenLastCalledWith(documentId, baseVersionId, 'authoring');
});

test('new version unknown retry retains operation, target and full immutable upload', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('published'));
  api.listDocumentVersions.mockResolvedValue({ items: [version({ lifecycleState: 'published', isCurrent: true })], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue({ ...versionDetail(), lifecycleState: 'published', isCurrent: true });
  api.createVersion.mockRejectedValueOnce(new Error('connection lost'));
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=published&tab=versions`);
  const panel = await screen.findByRole('tabpanel', { name: '版・改訂' });
  await user.click(within(panel).getByRole('button', { name: '新しい版を作成' }));
  expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
  expect(screen.queryByRole('complementary', { name: '原本と版' })).not.toBeInTheDocument();
  expect(await screen.findByRole('heading', { name: '新しい版を作成' })).toBeInTheDocument();
  const file = new File(['内容'], '受入手順.txt', { type: 'text/plain' });
  const input = await screen.findByLabelText('差替ファイル: source.txt（固定パス: primary、順序: 0）') as HTMLInputElement;
  await user.upload(input, file); fireEvent.submit(input.form!);
  await screen.findByText('保存結果を確認できません');
  await user.click(screen.getByRole('button', { name: '同じ内容で再試行' }));
  await screen.findByText('新しい作業版を作成しました。');
  expect(api.createVersion).toHaveBeenCalledTimes(2);
  const first = api.createVersion.mock.calls[0]!; const second = api.createVersion.mock.calls[1]!;
  expect(first[1]).toMatchObject({ targetVersionId: expect.stringMatching(/^[0-9a-f-]{36}$/i), expectedRevision: 7, title: '受入手順',
    items: [{ logicalPath: 'primary', ordinal: 0, originalFilename: '受入手順.txt' }] });
  expect(second).toEqual(first); expect(first[2].get(first[1].items[0].partId)).toBe(file);
});

test('an explicit-MIME synthetic replacement preserves its existing manifest anchor without an extension', async () => {
  const api = mockApi();
  api.listDocumentVersions.mockResolvedValue({ items: [version({ lifecycleState: 'published', isCurrent: true })], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue({ ...versionDetail(), lifecycleState: 'published', isCurrent: true });
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=published&tab=versions`);
  const panel = await screen.findByRole('tabpanel', { name: '版・改訂' });
  await user.click(within(panel).getByRole('button', { name: '新しい版を作成' }));
  const file = new File(['Synthetic changed content'], 'primary', { type: 'text/plain' });
  const input = await screen.findByLabelText('差替ファイル: source.txt（固定パス: primary、順序: 0）') as HTMLInputElement;
  await user.upload(input, file); fireEvent.submit(input.form!);
  await screen.findByText('新しい作業版を作成しました。');
  expect(api.createVersion).toHaveBeenCalledTimes(1);
  const [id, body, files] = api.createVersion.mock.calls[0]!;
  expect(id).toBe(documentId);
  expect(body.items).toEqual([expect.objectContaining({ logicalPath: 'primary', ordinal: 0,
    mediaType: 'text/plain', originalFilename: 'primary' })]);
  expect(files.get(body.items[0].partId)).toBe(file);
});

test('publish workspace requires review, then confirmation is keyboard dismissible and restores focus', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('authoring'));
  api.listDocumentVersions.mockResolvedValue({ items: [version()], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue(versionDetail());
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=authoring&tab=versions`);

  await user.click(await screen.findByRole('button', { name: '公開する' }));
  expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
  expect(screen.queryByRole('complementary', { name: '原本と版' })).not.toBeInTheDocument();
  const confirmTarget = screen.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' });
  const publishButton = screen.getByRole('button', { name: '公開する' });
  expect(publishButton).toBeDisabled();
  await user.click(confirmTarget);
  await user.click(publishButton);
  const dialog = await screen.findByRole('dialog', { name: '公開を確認' });
  expect(dialog.contains(document.activeElement)).toBe(true);
  await user.keyboard('{Escape}');

  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await waitFor(() => expect(publishButton).toHaveFocus());
  expect(api.publishVersion).not.toHaveBeenCalled();
});

test('successful publication restores focus to the return control when refreshed capabilities disable its trigger', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('authoring'));
  api.publishVersion.mockImplementation(async () => {
    api.getDocumentVersion.mockResolvedValue(versionDetail({ publish: operationDenied, schedulePublication: operationDenied }));
    return { publishOperationId: 'pub', documentId, documentVersionId: versionId,
      resultingDocumentRevision: 8, publishedAt: '2026-10-01T02:00:00Z' };
  });
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=authoring&tab=versions`);
  await user.click(await screen.findByRole('button', { name: '公開する' }));
  await user.click(screen.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }));
  const trigger = screen.getByRole('button', { name: '公開する' });
  await user.click(trigger);
  const dialog = await screen.findByRole('dialog', { name: '公開を確認' });
  await user.click(within(dialog).getByRole('button', { name: '確定する' }));
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('公開しました'));
  await waitFor(() => expect(trigger).toBeDisabled());
  await waitFor(() => expect(screen.getByRole('button', { name: '版の一覧へ戻る' })).toHaveFocus());
  expect(api.publishVersion).toHaveBeenCalledTimes(1);
});

test('scheduled publication converts JST to UTC after explicit review and confirmation', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('authoring'));
  api.listDocumentVersions.mockResolvedValue({ items: [version()], nextCursor: null });
  api.getDocumentVersion.mockResolvedValue(versionDetail());
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=authoring&tab=versions`);

  await user.click(await screen.findByRole('button', { name: '予約公開する' }));
  await user.click(await screen.findByRole('radio', { name: /日時を指定/ }));
  fireEvent.change(screen.getByLabelText(/公開日時（JST \/ UTC\+09:00）/), { target: { value: '2026-10-02T09:30' } });
  await user.click(screen.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }));
  await user.click(screen.getByRole('button', { name: '公開を予約する' }));
  const dialog = await screen.findByRole('dialog', { name: '予約公開を確認' });
  await user.click(within(dialog).getByRole('button', { name: '確定する' }));

  await screen.findByRole('status');
  expect(api.schedulePublication).toHaveBeenCalledWith(documentId, versionId, expect.objectContaining({
    expectedRevision: 7,
    scheduledPublishAt: '2026-10-02T00:30:00.000Z',
  }));
});

test('partial comparison keeps Unknown and Partial visible and sends the reader to source files', async () => {
  const api = mockApi();
  api.compareDocumentRevisions.mockResolvedValue(comparison() as never);
  api.listVersionFiles.mockResolvedValue({ items: [{ contentItemId: 'content-id', representationId: 'representation-id', logicalPath: 'source.txt', ordinal: 0, role: 'primary', displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 }] });
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=published&tab=compare`);

  expect(await screen.findByText('不明')).toBeInTheDocument();
  expect(screen.getByText('一部のみ')).toBeInTheDocument();
  expect(screen.getByRole('heading', { name: /未比較範囲/ })).toBeInTheDocument();
  expect(screen.getByText(/目視で確認/)).toBeInTheDocument();
  expect(await screen.findByRole('button', { name: '基準原本を確認' })).toBeInTheDocument();
  expect(api.compareDocumentRevisions).toHaveBeenCalledWith(documentId, expect.objectContaining({
    projection: 'display', pageSize: 50,
  }));
  await user.click(screen.getByRole('button', { name: '基準原本を確認' }));
  await waitFor(() => expect(api.downloadVersionFile).toHaveBeenCalledWith(expect.objectContaining({
    documentId, contentItemId: 'content-id', representationId: 'representation-id', purpose: 'published',
  }), { signal: expect.any(AbortSignal) }));
});

test('access changes use existing identities and the current policy revision', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('published', true));
  api.getDocumentAccessPolicy.mockResolvedValue(policyRead());
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=published&tab=overview`);

  await screen.findByRole('heading', { name: '受入手順' });
  await user.click(screen.getByRole('tab', { name: 'アクセス' }));
  await user.click(await screen.findByRole('radio', { name: 'この文書だけに個別設定' }));
  expect(screen.getAllByText('reviewer-role').length).toBeGreaterThan(0);
  expect(screen.queryByRole('button', { name: /ユーザーを追加/ })).not.toBeInTheDocument();
  await user.type(screen.getByLabelText('変更理由'), '担当者の定期見直し');
  await user.click(screen.getByRole('button', { name: 'アクセス設定を保存' }));
  await screen.findByText('アクセス設定を保存しました。');
  expect(api.setDocumentAccessPolicy).toHaveBeenCalledWith(documentId, expect.objectContaining({
    mode: 'explicit', expectedPolicyRevision: 3, reason: '担当者の定期見直し',
    grants: [{ subjectKind: 'role', identityProvider: 'directory', subjectId: 'reviewer-role', actions: ['read', 'write'] }],
  }));
});

test('Access tab stays hidden without the server capability', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('published', false));
  renderAt(`/documents/${documentId}?view=published&tab=access`);

  await screen.findByRole('heading', { name: '受入手順' });
  expect(screen.queryByRole('tab', { name: 'アクセス' })).not.toBeInTheDocument();
  expect(api.getDocumentAccessPolicy).not.toHaveBeenCalled();
});

test('an explicit policy can be changed back to inheritance with a reason and its current revision', async () => {
  const api = mockApi();
  api.getDocument.mockResolvedValue(documentDetail('published', true));
  api.getDocumentAccessPolicy.mockResolvedValue({
    ...policyRead(),
    bindingMode: 'explicit',
    policyId: 'document-policy',
    effectiveSource: { kind: 'document', id: documentId },
  });
  const user = userEvent.setup();
  renderAt(`/documents/${documentId}?view=published&tab=access`);

  await screen.findByRole('heading', { name: '受入手順' });
  await user.click(await screen.findByRole('radio', { name: '親フォルダーのアクセス権を継承' }));
  await user.type(screen.getByLabelText('変更理由'), '親フォルダーの設定へ戻す');
  await user.click(screen.getByRole('button', { name: 'アクセス設定を保存' }));

  await screen.findByText('アクセス設定を保存しました。');
  expect(api.setDocumentAccessPolicy).toHaveBeenCalledWith(documentId, {
    operationId: expect.any(String),
    expectedPolicyRevision: 3,
    reason: '親フォルダーの設定へ戻す',
    mode: 'inherit',
  });
  expect(screen.getByRole('region', { name: '文書アクセス設定の保存結果' })).toHaveTextContent(documentId);
});

function listItem(view: 'published' | 'authoring') {
  const display = baseVersionSummary(view === 'authoring' ? 'WORKING' : 'PUBLISHED', view === 'authoring' ? 3 : 2);
  const base = {
    documentId,
    documentVersionId: versionId,
    title: '受入手順',
    folderId: null,
    folderName: null,
    revision: 7,
    currentVersionId: view === 'published' ? versionId : null,
    metadata: { category: 'operations' },
    displayVersion: display,
    displayRevision: view === 'published' ? revision(revisionId, versionId, 1) : null,
    readState: { isRead: true, firstReadAt: '2026-09-30T03:00:00Z' },
    displayTimestamp: { kind: view === 'authoring' ? 'workingUpdatedAt' as const : 'revisionCreatedAt' as const, value: '2026-10-01T01:00:00Z' },
  };
  return view === 'published' ? { ...base, currentVersionId: versionId } : { ...base, lifecycleState: 'working' as const };
}

function documentDetail(view: 'published' | 'authoring', canManageAccess = true): DocumentDetail {
  const common = {
    documentId,
    documentVersionId: versionId,
    title: '受入手順',
    folderId: null,
    folderName: null,
    revision: 7,
    metadata: { category: 'operations', owner: '総務' },
    createdAt: '2026-09-01T00:00:00Z',
    displayVersion: baseVersionSummary(view === 'authoring' ? 'WORKING' : 'PUBLISHED', view === 'authoring' ? 3 : 2),
    displayRevision: view === 'published' ? revision(revisionId, versionId, 1) : null,
    readState: { isRead: true, firstReadAt: '2026-09-30T03:00:00Z' },
    displayTimestamp: { kind: view === 'authoring' ? 'workingUpdatedAt' as const : 'revisionCreatedAt' as const, value: '2026-10-01T01:00:00Z' },
    capabilities: {
      createVersion: operationAvailable,
      updateMetadata: operationDenied,
      moveDocument: operationDenied,
      endPublication: operationDenied,
      manageAccess: canManageAccess ? operationAvailable : operationDenied,
      compareVersions: operationAvailable,
    },
  };
  return view === 'published'
    ? { ...common, currentVersionId: versionId, unread: false, publishedAt: '2026-09-30T03:00:00Z' }
    : { ...common, currentVersionId: null, lifecycleState: 'working' };
}

function baseVersionSummary(lifecycleState: 'WORKING' | 'PUBLISHED', versionNo: number) {
  return {
    versionId,
    versionNo,
    baseVersionId: baseVersionId,
    lifecycleState,
    isCurrent: lifecycleState === 'PUBLISHED',
    approvedAt: lifecycleState === 'PUBLISHED' ? '2026-09-30T03:00:00Z' : null,
    scheduledPublishAt: null,
    publishedAt: lifecycleState === 'PUBLISHED' ? '2026-09-30T03:00:00Z' : null,
    withdrawnAt: null,
    updatedAt: '2026-10-01T01:00:00Z',
    fileSummary: { authoritativeItemCount: 1, totalSizeBytes: 12, primary: { displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 } },
  };
}

function version(overrides: Partial<Version> = {}) {
  return {
    versionId,
    versionNo: 3,
    baseVersionId,
    lifecycleState: 'working' as const,
    isCurrent: false,
    createdAt: '2026-09-30T00:00:00Z',
    approvedAt: null,
    scheduledPublishAt: null,
    publishedAt: null,
    withdrawnAt: null,
    updatedAt: '2026-10-01T01:00:00Z',
    fileSummary: { authoritativeItemCount: 1, totalSizeBytes: 12, primary: { displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 } },
    firstReadAt: null,
    title: '受入手順',
    metadata: {},
    ...overrides,
  };
}

function versionDetail(overrides: { publish?: typeof operationAvailable | typeof operationDenied; schedulePublication?: typeof operationAvailable | typeof operationDenied } = {}) {
  return {
    ...version(),
    lifecycleState: 'working' as const,
    title: '受入手順',
    metadata: {},
    capabilities: {
      edit: operationAvailable,
      rebase: operationDenied,
      publish: overrides.publish ?? operationAvailable,
      withdraw: operationDenied,
      schedulePublication: overrides.schedulePublication ?? operationAvailable,
      cancelPublicationSchedule: operationDenied,
      download: operationAvailable,
    },
  };
}

function revision(id: string, version: string, major: number) {
  return {
    revisionId: id,
    documentVersionId: version,
    major,
    minor: 0,
    label: `${major}.0`,
    createdAt: `2026-09-${major === 1 ? '30' : '29'}T03:00:00Z`,
    sourceKind: major === 1 ? 'initialPublication' as const : 'contentPublication' as const,
    metadataSnapshotStatus: 'complete' as const,
  };
}

function policyRead() {
  return {
    target: { kind: 'document' as const, id: documentId },
    bindingMode: 'inherit' as const,
    policyId: null,
    policyRevision: 3,
    effectivePolicyId: 'policy-1',
    effectiveSource: { kind: 'folder' as const, id: folderId },
    effectiveGrants: [{
      subjectKind: 'role' as const,
      identityProvider: 'directory',
      subjectId: 'reviewer-role',
      actions: ['read', 'write'] as Array<'read' | 'write'>,
      presentation: {
        ref: { provider: 'directory', kind: 'role' as const, subjectId: 'reviewer-role' },
        displayName: null,
        secondaryText: null,
        resolution: 'unavailable' as const,
      },
    }],
  };
}

function comparison() {
  const baseEvidence = {
    documentId,
    versionId: baseVersionId,
    contentItemId: 'content-id',
    representationId: 'representation-id',
    fileId: 'file-id',
    rawSha256: 'sha256',
    inspectionProfile: 'dsi-v0',
    locator: { kind: 'textSpan' as const, line: 14, byteStart: 0, byteEnd: 20 },
    granularity: 'exact' as const,
    parserProvenance: 'adapter',
  };
  return {
    projection: 'display' as const,
    baseRevision: { revisionId, documentVersionId: baseVersionId, major: 1, minor: 0, createdAt: '2026-09-30T03:00:00Z' },
    targetRevision: { revisionId: newerRevisionId, documentVersionId: versionId, major: 2, minor: 0, createdAt: '2026-10-01T03:00:00Z' },
    contentComparisonStatus: 'differentAuthoritativeVersions' as const,
    verdict: 'unknown' as const,
    coverage: 'partial' as const,
    resultDigest: 'digest',
    changes: [],
    rows: [],
    unverifiedRegions: [{ reason: 'unsupportedSemanticConstruct' as const, base: baseEvidence, target: { ...baseEvidence, versionId }, navigationHint: '14行目の原本を確認' }],
    ancillaryChanges: [],
    metadataComparisonStatus: 'unavailableLegacy' as const,
    metadataChanges: [],
    baseMetadataSnapshotDigest: null,
    targetMetadataSnapshotDigest: null,
    auditEventId: 'audit-1',
    displayItems: [],
    pageSize: 50,
    nextCursor: null,
  };
}


const metadataFields = [
  ['documentType', '文書種別'], ['owningDepartment', '所管部署'], ['category', 'カテゴリ'],
] as const;

async function fillMetadata(values: Record<string, string>) {
  for (const [key, label] of metadataFields) fireEvent.change(screen.getByRole('textbox', { name: label }), { target: { value: values[key] ?? '' } });
}

test('metadata drafts apply explicitly and clearing only metadata discards cursor but keeps list context', async () => {
  const api = mockApi();
  const { router } = renderAt('/documents?view=authoring&titleContains=keep&sort=title_asc&pageSize=25&includeDescendants=true&folderId=' + folderId + '&cursor=old&selectedDocumentId=' + documentId + '&panel=closed');
  await screen.findByRole('button', { name: /受入手順/ });
  const initialCalls = api.listDocuments.mock.calls.length;
  const values = { documentType: '123', owningDepartment: '   ', category: 'e\u0301%_"\\' };
  await fillMetadata(values);
  expect(api.listDocuments).toHaveBeenCalledTimes(initialCalls);
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining(values)));
  expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('cursor');
  expect(router.state.location.search).toMatchObject({ ...values, view: 'authoring', titleContains: 'keep', folderId, includeDescendants: true, sort: 'title_asc', pageSize: 25, panel: 'closed', selectedDocumentId: documentId });
  await act(async () => { await router.navigate({ to: '/documents', search: { ...router.state.location.search, cursor: 'new-page' } as never }); });
  fireEvent.click(screen.getByRole('button', { name: '属性の絞り込みを解除' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('documentType'));
  expect(router.state.location.search).toMatchObject({ view: 'authoring', titleContains: 'keep', folderId, includeDescendants: true, sort: 'title_asc', pageSize: 25, panel: 'closed', selectedDocumentId: documentId });
  for (const key of ['cursor', 'documentType', 'owningDepartment', 'category']) expect(router.state.location.search).not.toHaveProperty(key);
  await waitFor(() => expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({ titleContains: 'keep' })));
  for (const key of ['cursor', 'documentType', 'owningDepartment', 'category']) expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty(key);
});

test.each(metadataFields)('single %s filter omits the two empty fields from GET', async (key, label) => {
  const api = mockApi();
  renderAt('/documents?panel=closed');
  await screen.findByRole('button', { name: /受入手順/ });
  fireEvent.change(screen.getByRole('textbox', { name: label }), { target: { value: 'true' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0][key]).toBe('true'));
  for (const [other] of metadataFields) if (other !== key) expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty(other);
});

test.each([
  ['a'.repeat(1024), null], ['日'.repeat(341) + 'a', null], ['😀'.repeat(256), null],
  ['a'.repeat(1025), '1024 UTF-8 bytes'], ['日'.repeat(342), '1024 UTF-8 bytes'],
  ['a\u0001b', '制御文字'], ['a\u0085b', '制御文字'], ['\ud800', 'Unicode'],
])('metadata form retains %p and blocks invalid GET with a reason', async (value, reason) => {
  const api = mockApi();
  const { router } = renderAt('/documents?panel=closed');
  await screen.findByRole('button', { name: /受入手順/ });
  const calls = api.listDocuments.mock.calls.length;
  const input = screen.getByRole('textbox', { name: '文書種別' });
  fireEvent.change(input, { target: { value } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  expect(input).toHaveValue(value);
  if (reason) {
    expect(await screen.findByRole('alert')).toHaveTextContent(reason);
    expect(api.listDocuments).toHaveBeenCalledTimes(calls);
    expect(router.state.location.search).not.toHaveProperty('documentType');
  } else await waitFor(() => expect(api.listDocuments.mock.lastCall![0].documentType).toBe(value));
});

test.each(['日'.repeat(342), 'a\u0085b'])('invalid metadata URL remains visible and never fetches an unfiltered list: %p', async value => {
  const api = mockApi();
  const { router } = renderAt('/documents' + defaultStringifySearch({ documentType: value, titleContains: 'keep', panel: 'closed' }));
  await screen.findByRole('textbox', { name: '文書種別' });
  expect(screen.getByRole('textbox', { name: '文書種別' })).toHaveValue(value);
  expect(await screen.findByRole('alert')).toBeVisible();
  expect(router.state.location.search).toMatchObject({ documentType: value, titleContains: 'keep' });
  expect(api.listDocuments).not.toHaveBeenCalled();
  expect(screen.queryByRole('heading', { name: '文書がありません' })).not.toBeInTheDocument();
  expect(screen.queryByText('文書を読み込み中')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '属性の絞り込みを解除' }));
  await waitFor(() => expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({ titleContains: 'keep' })));
});

test.each([
  { documentType: '123', owningDepartment: 'true', category: 'null', titleContains: '[1]' },
  { documentType: 'é'.repeat(512), owningDepartment: '日'.repeat(341) + 'a', category: '😀'.repeat(256), titleContains: 'quote"\\' },
])('detail return decodes router-quoted metadata and retains >2KiB filter context', async values => {
  const api = mockApi();
  const { router } = renderAt('/documents' + defaultStringifySearch({ view: 'authoring', ...values, cursor: 'opaque+/=', sort: 'title_asc', pageSize: 25, selectedDocumentId: documentId, panel: 'open' }));
  await screen.findByRole('button', { name: /受入手順/ });
  fireEvent.click(await screen.findByRole('button', { name: /詳細を開く/ }));
  expect(await screen.findByRole('heading', { name: '受入手順', level: 1 })).toBeVisible();
  expect(router.state.location.search).toHaveProperty('returnTo');
  if (values.documentType.length > 100) expect(String(router.state.location.search.returnTo).length).toBeGreaterThan(2048);
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(router.state.location.search).toMatchObject({ ...values, cursor: 'opaque+/=', sort: 'title_asc', pageSize: 25, selectedDocumentId: documentId, panel: 'open' });
  expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining(values));
});

test('metadata URL history and page/sort/folder changes keep exact conditions and independent mutation stores', async () => {
  const api = mockApi();
  api.listDocuments.mockResolvedValue({ view: 'published', items: [listItem('published')], nextCursor: 'next' });
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, parentFolderId: folderId, name: '審査', revision: 3, capabilities: {} }], nextCursor: null, capabilities: {} });
  const { router, client } = renderAt('/documents' + defaultStringifySearch({ documentType: 'old', owningDepartment: '   ', category: '[1]' }));
  const rootStore = rootFolderOperations(client);
  const renameStore = folderRenameOperations(client);
  const metadataStore = metadataOperations(client);
  const creation = { state: 'unknown' as const, ids: { documentId, documentVersionId: versionId, fileId: revisionId } };
  const rootSnapshot = { status: 'unknown' as const, request: { operationId: 'kept-root', folderId: reviewFolderId, parentFolderId: folderId, expectedParentRevision: 1, name: 'kept', reason: 'keep' } };
  const renameSnapshot = { status: 'unknown' as const, targetFolderId: reviewFolderId, request: { operationId: 'kept-rename', expectedFolderRevision: 3, name: 'kept', reason: 'keep' }, context: { kind: 'selected' as const, folderId: reviewFolderId, sourceParentId: folderId, pageLimit: 1, name: '審査' }, currentName: '審査', expectedChanged: true };
  const metadataSnapshot = { status: 'unknown' as const, request: { operationId: 'kept-metadata', expectedDocumentRevision: 7, set: { category: 'kept' }, unset: [], reason: 'keep' } };
  await act(async () => { rootStore.put(rootSnapshot); renameStore.put(renameSnapshot); metadataStore.put(documentId, metadataSnapshot); });
  saveCreationReceipt(creation);
  await screen.findByRole('button', { name: /受入手順/ });
  fireEvent.change(screen.getByRole('textbox', { name: '文書種別' }), { target: { value: 'new' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(router.state.location.search.documentType).toBe('new'));
  await act(async () => router.history.back());
  await waitFor(() => expect(screen.getByRole('textbox', { name: '文書種別' })).toHaveValue('old'));
  await act(async () => router.history.forward());
  await waitFor(() => expect(screen.getByRole('textbox', { name: '文書種別' })).toHaveValue('new'));
  fireEvent.click(screen.getByRole('button', { name: /次のページ/ }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0].cursor).toBe('next'));
  fireEvent.change(screen.getByRole('combobox', { name: '並び順' }), { target: { value: 'title_asc' } });
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('cursor'));
  fireEvent.change(screen.getByRole('combobox', { name: '1ページあたりの件数' }), { target: { value: '25' } });
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0].pageSize).toBe(25));
  fireEvent.click(await screen.findByRole('button', { name: '審査' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0].folderId).toBe(reviewFolderId));
  expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ documentType: 'new', owningDepartment: '   ', category: '[1]', sort: 'title_asc', pageSize: 25 });
  expect(rootStore.get()).toBe(rootSnapshot);
  expect(renameStore.get()).toBe(renameSnapshot);
  expect(metadataStore.get(documentId)).toBe(metadataSnapshot);
  expect(readCreationReceipt()).toEqual(creation);
  await act(async () => {
    rootStore.put({ ...rootSnapshot, status: 'rejected' }); rootStore.clearSettled(rootStore.get()!);
    renameStore.put({ ...renameSnapshot, status: 'rejected' }); renameStore.clearSettled(renameStore.get()!);
    metadataStore.clear(documentId);
  });
  clearCreationReceipt();
});

test.each(metadataFields)('separate %s query keys prevent delayed old results replacing the current list', async (key, label) => {
  const api = mockApi();
  let resolveOld!: (value: DocumentList) => void;
  api.listDocuments.mockImplementation(query => query[key] === 'old' ? new Promise(resolve => { resolveOld = resolve; }) : Promise.resolve({ view: 'published', items: [{ ...listItem('published'), title: '現在条件の結果' }], nextCursor: null }));
  const { router } = renderAt('/documents' + defaultStringifySearch({ [key]: 'old', panel: 'closed' }));
  await waitFor(() => expect(api.listDocuments).toHaveBeenCalledWith(expect.objectContaining({ [key]: 'old' })));
  fireEvent.change(screen.getByRole('textbox', { name: label }), { target: { value: 'new' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await screen.findByRole('button', { name: '現在条件の結果' });
  await act(async () => resolveOld({ view: 'published', items: [{ ...listItem('published'), title: '旧条件の遅延結果' }], nextCursor: null } as DocumentList));
  expect(router.state.location.search[key]).toBe('new');
  expect(screen.getByRole('button', { name: '現在条件の結果' })).toBeVisible();
  expect(screen.queryByRole('button', { name: '旧条件の遅延結果' })).not.toBeInTheDocument();
});

test('metadata GET failure is distinct from a successful empty result and supports explicit retry', async () => {
  const api = mockApi();
  api.listDocuments.mockRejectedValueOnce({ code: 'FORBIDDEN', status: 403 }).mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  renderAt('/documents' + defaultStringifySearch({ category: 'synthetic', panel: 'closed' }));
  expect(await screen.findByRole('alert')).toBeVisible();
  expect(screen.queryByRole('heading', { name: '文書がありません' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '再読み込み' }));
  expect(await screen.findByRole('heading', { name: '文書がありません' })).toBeVisible();
  expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'synthetic' }));
});


test('isolated surrogate URL stops at the existing route error without a replacement-value GET', async () => {
  const api = mockApi();
  // jsdom omits Response; router error classification only needs the browser instanceof boundary.
  const originalResponse = globalThis.Response;
  globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const { router } = renderAt('/documents?documentType=%22%5Cud800%22');
    expect(await screen.findByText('文書種別に不正なUnicode文字が含まれています。入力を確認してください。')).toBeVisible();
    expect(api.listDocuments).not.toHaveBeenCalled();
    expect(screen.queryByRole('textbox', { name: '文書種別' })).not.toBeInTheDocument();
    expect(router.state.matches.find(match => match.routeId === '/documents')?.status).toBe('error');
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});


test.each([
  ['documentType', '文書種別', [1]], ['owningDepartment', '所管部署', null], ['category', 'カテゴリ', true],
])('non-string %s route shows a fixed reason and never requests a broader list', async (key, label, value) => {
  const api = mockApi();
  // jsdom omits Response; router error classification only needs the browser instanceof boundary.
  const originalResponse = globalThis.Response;
  globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    renderAt('/documents' + defaultStringifySearch({ [key]: value }));
    expect(await screen.findByText(`${label}は文字列で指定してください。URLの条件を確認してください。`)).toBeVisible();
    expect(api.listDocuments).not.toHaveBeenCalled();
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});


test('invalid metadata route offers an in-app attribute reset while preserving other list state and unknown operations', async () => {
  const api = mockApi();
  const originalResponse = globalThis.Response; globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const { router, client } = renderAt('/documents' + defaultStringifySearch({ documentType: [1], titleContains: 'keep', view: 'authoring', folderId, sort: 'title_asc', pageSize: 25, cursor: 'discard', selectedDocumentId: documentId, panel: 'closed' }));
    const store = metadataOperations(client);
    const unknown = { status: 'unknown' as const, request: { operationId: 'kept-route-error', expectedDocumentRevision: 7, set: { category: 'kept' }, unset: [], reason: 'keep' } };
    store.put(documentId, unknown);
    const link = await screen.findByRole('link', { name: '条件を解除して一覧へ戻る' });
    expect(api.listDocuments).not.toHaveBeenCalled();
    fireEvent.click(link);
    await screen.findByRole('button', { name: /受入手順/ });
    expect(router.state.location.search).toMatchObject({ titleContains: 'keep', view: 'authoring', folderId, sort: 'title_asc', pageSize: 25, selectedDocumentId: documentId, panel: 'closed' });
    expect(router.state.location.search).not.toHaveProperty('documentType');
    expect(router.state.location.search).not.toHaveProperty('cursor');
    expect(store.get(documentId)).toBe(unknown);
    store.clear(documentId);
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});


test('document route error never renders unknown error text or a stack', async () => {
  mockApi();
  const originalResponse = globalThis.Response; globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    renderAt('/documents', new Error('synthetic-private-unknown-error'));
    expect(await screen.findByText('一覧の条件を確認できません。条件を解除して再度お試しください。')).toBeVisible();
    expect(screen.queryByText(/synthetic-private-unknown-error/)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: '条件を解除して一覧へ戻る' })).toBeVisible();
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});

test.each([
  ['/documents?documentType=%22%5Cud800%22', '文書種別に不正なUnicode文字が含まれています。入力を確認してください。'],
  ['/documents?category=%5B1%5D', 'カテゴリは文字列で指定してください。URLの条件を確認してください。'],
])('invalid returnTo metadata shows its fixed reason and never falls back to an unfiltered list', async (returnTo, reason) => {
  const api = mockApi();
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', tab: 'overview', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  expect(await screen.findByRole('alert')).toHaveTextContent(reason);
  expect(router.state.location.pathname).toBe(`/documents/${documentId}`);
  expect(api.listDocuments).not.toHaveBeenCalled();
});

test.each([
  ['/documents?documentType=a%01b&titleContains=keep&pageSize=0', '文書種別', 'a\u0001b', '制御文字'],
  ['/documents?category=' + 'a'.repeat(1025) + '&titleContains=keep&cursor=', 'カテゴリ', 'a'.repeat(1025), '1024 UTF-8 bytes'],
])('compound invalid metadata URL keeps its input and stops every list GET', async (entry, label, value, reason) => {
  const api = mockApi();
  const { router } = renderAt(entry);
  expect(await screen.findByRole('textbox', { name: label })).toHaveValue(value);
  expect(await screen.findByRole('alert')).toHaveTextContent(reason);
  expect(api.listDocuments).not.toHaveBeenCalled();
  expect(router.state.location.pathname).toBe('/documents');
  expect(screen.queryByRole('heading', { name: '文書がありません' })).not.toBeInTheDocument();
});

test.each([
  ['/documents?documentType=a%01b&titleContains=keep&pageSize=0', '文書種別', 'a\u0001b', '制御文字'],
  ['/documents?category=' + 'a'.repeat(1025) + '&titleContains=keep&cursor=', 'カテゴリ', 'a'.repeat(1025), '1024 UTF-8 bytes'],
])('compound invalid returnTo keeps metadata on the list and cannot fall back to a broader GET', async (returnTo, label, value, reason) => {
  const api = mockApi();
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', tab: 'overview', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  expect(await screen.findByRole('textbox', { name: label })).toHaveValue(value);
  expect(await screen.findByRole('alert')).toHaveTextContent(reason);
  expect(router.state.location.pathname).toBe('/documents');
  expect(api.listDocuments).not.toHaveBeenCalled();
});

test('valid metadata remains in the actual GET when an old condition falls back to defaults', async () => {
  const api = mockApi();
  const metadata = { documentType: '123', owningDepartment: '   ', category: 'e\u0301' };
  const { router } = renderAt('/documents' + defaultStringifySearch({ ...metadata, titleContains: 'keep', pageSize: 0 }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({ ...metadata, pageSize: 50 }));
  // TanStack preserves the original title while merging the old default fallback; keep that contract.
  expect(api.listDocuments.mock.lastCall![0]).toHaveProperty('titleContains', 'keep');
  expect(router.state.location.search).toMatchObject(metadata);
  for (const [key, label] of metadataFields) expect(screen.getByRole('textbox', { name: label })).toHaveValue(metadata[key]);
});


test('valid metadata also survives old-condition fallback when returning from detail', async () => {
  const api = mockApi();
  const metadata = { documentType: '123', owningDepartment: '   ', category: 'e\u0301' };
  const returnTo = '/documents' + defaultStringifySearch({ ...metadata, titleContains: 'keep', pageSize: 0 });
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', tab: 'overview', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments).toHaveBeenLastCalledWith(expect.objectContaining({ ...metadata, pageSize: 50 }));
  expect(router.state.location.search).toMatchObject(metadata);
});

test.each([
  [{ documentType: '', owningDepartment: '', category: '' }, {}],
  [{ documentType: '', owningDepartment: '   ', category: '[1]' }, { owningDepartment: '   ', category: '[1]' }],
  [{ documentType: '123', owningDepartment: '', category: 'e\u0301' }, { documentType: '123', category: 'e\u0301' }],
  [{ documentType: 'null', owningDepartment: '   ', category: '', pageSize: 0 }, { documentType: 'null', owningDepartment: '   ' }],
  [{ documentType: '', owningDepartment: '', category: '', pageSize: 0 }, {}],
])('real route omits only empty metadata from the GUI GET, including old invalid conditions: %p', async (conditions, expectedMetadata) => {
  const api = mockApi();
  renderAt('/documents' + defaultStringifySearch({ ...conditions, titleContains: 'keep', panel: 'closed' }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments).toHaveBeenLastCalledWith({ view: 'published', titleContains: 'keep', sort: 'published_at_desc', pageSize: 50, ...expectedMetadata });
});

test('empty metadata route uses the same Query key and cached query as unspecified metadata', async () => {
  const api = mockApi();
  const { router, client } = renderAt('/documents?documentType=&owningDepartment=&category=&panel=closed');
  await screen.findByRole('button', { name: /受入手順/ });
  const initialQuery = client.getQueryCache().findAll({ queryKey: ['documents'] })[0]!;
  const initialCalls = api.listDocuments.mock.calls.length;
  await act(async () => { await router.navigate({ to: '/documents', search: { view: 'published', includeDescendants: false, sort: 'published_at_desc', pageSize: 50, panel: 'closed' } }); });
  await screen.findByRole('button', { name: /受入手順/ });
  const query = client.getQueryCache().findAll({ queryKey: ['documents'] })[0]!;
  expect(query).toBe(initialQuery);
  expect(api.listDocuments).toHaveBeenCalledTimes(initialCalls);
  expect(query.queryKey[1]).toMatchObject({ documentType: undefined, owningDepartment: undefined, category: undefined });
});

test('published unread draft applies with existing filters, drops cursor on and off, and keeps selection', async () => {
  const api = mockApi();
  const conditions = { titleContains: 'keep', documentType: 'type', owningDepartment: '   ', category: 'category', folderId, includeDescendants: true, sort: 'title_asc', pageSize: 25, cursor: 'old', selectedDocumentId: documentId, panel: 'open' };
  const { router } = renderAt('/documents' + defaultStringifySearch(conditions));
  const checkbox = await screen.findByRole('checkbox', { name: '未読のみ' });
  expect(checkbox).not.toBeChecked();
  const calls = api.listDocuments.mock.calls.length;
  fireEvent.click(checkbox);
  expect(api.listDocuments).toHaveBeenCalledTimes(calls);
  expect(router.state.location.search).not.toHaveProperty('unreadOnly');
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).toHaveProperty('unreadOnly', true));
  const { cursor: oldCursor, ...keptConditions } = conditions; void oldCursor;
  expect(router.state.location.search).toMatchObject({ ...keptConditions, unreadOnly: true });
  expect(router.state.location.search).not.toHaveProperty('cursor');
  await act(async () => { await router.navigate({ to: '/documents', search: { ...router.state.location.search, cursor: 'second' } as never }); });
  fireEvent.click(screen.getByRole('button', { name: '属性の絞り込みを解除' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('documentType'));
  expect(router.state.location.search).toHaveProperty('unreadOnly', true);
  expect(checkbox).toBeChecked();
  await act(async () => { await router.navigate({ to: '/documents', search: { ...router.state.location.search, cursor: 'third' } as never }); });
  fireEvent.click(checkbox);
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('unreadOnly'));
  expect(router.state.location.search).not.toHaveProperty('cursor');
  expect(router.state.location.search).toMatchObject({ titleContains: 'keep', folderId, includeDescendants: true, sort: 'title_asc', pageSize: 25, selectedDocumentId: documentId, panel: 'open' });
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('unreadOnly'));
});

test.each(['authoring', 'history'])('unread checkbox is hidden for %s', async view => {
  mockApi();
  renderAt('/documents' + defaultStringifySearch({ view }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(screen.queryByRole('checkbox', { name: '未読のみ' })).not.toBeInTheDocument();
});

test('raw published false and missing unread share a GET and cached Query despite router merge', async () => {
  const api = mockApi();
  const { router, client } = renderAt('/documents?view=published&unreadOnly=false&panel=closed');
  const checkbox = await screen.findByRole('checkbox', { name: '未読のみ' });
  expect(checkbox).not.toBeChecked();
  expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('unreadOnly');
  const query = client.getQueryCache().findAll({ queryKey: ['documents'] })[0]!;
  const calls = api.listDocuments.mock.calls.length;
  await act(async () => { await router.navigate({ to: '/documents', search: { view: 'published', includeDescendants: false, sort: 'published_at_desc', pageSize: 50, panel: 'closed' } }); });
  expect(client.getQueryCache().findAll({ queryKey: ['documents'] })).toEqual([query]);
  expect(api.listDocuments).toHaveBeenCalledTimes(calls);
  expect(query.queryKey[1]).toMatchObject({ unreadOnly: undefined });
});

test.each([
  [{ unreadOnly: '', documentType: 'keep' }, { documentType: 'keep' }, ['unreadOnly']],
  [{ unreadOnly: false, view: 'authoring', category: 'keep' }, { view: 'authoring', category: 'keep' }, ['unreadOnly']],
  [{ documentType: [1], unreadOnly: true }, { unreadOnly: true }, ['documentType']],
  [{ documentType: [1], unreadOnly: '' }, {}, ['documentType', 'unreadOnly']],
  [{ documentType: null, unreadOnly: false, view: 'history' }, { view: 'history' }, ['documentType', 'unreadOnly']],
])('route error removes each invalid group independently without widening valid conditions: %p', async (conditions, expected, removed) => {
  const api = mockApi();
  const originalResponse = globalThis.Response; globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const { router } = renderAt('/documents' + defaultStringifySearch({ ...conditions, titleContains: 'keep', cursor: 'discard', panel: 'closed' }));
    const link = await screen.findByRole('link', { name: '条件を解除して一覧へ戻る' });
    expect(api.listDocuments).not.toHaveBeenCalled();
    fireEvent.click(link);
    await screen.findByRole('button', { name: /受入手順/ });
    expect(router.state.location.search).toMatchObject({ titleContains: 'keep', ...expected });
    for (const key of [...removed, 'cursor']) expect(router.state.location.search).not.toHaveProperty(key);
    expect(api.listDocuments.mock.lastCall![0]).toMatchObject(expected);
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});

test.each(['/documents?unreadOnly=', '/documents?view=authoring&unreadOnly=false', '/documents?view=invalid&unreadOnly=true'])('invalid unread returnTo stops with a fixed reason instead of a default GET: %p', async returnTo => {
  const api = mockApi();
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  expect(await screen.findByRole('alert')).toHaveTextContent(returnTo.endsWith('=') ? '未読のみはtrueまたはfalseで指定してください。URLの条件を確認してください。' : '未読のみは公開一覧でのみ指定できます。URLの条件を確認してください。');
  expect(router.state.location.pathname).toBe(`/documents/${documentId}`);
  expect(api.listDocuments).not.toHaveBeenCalled();
});

test.each([{ pageSize: 0 }, { cursor: '' }, { sort: 'invalid' }])('unread actual GET survives old-condition fallback and detail return: %p', async invalid => {
  const api = mockApi();
  const returnTo = '/documents' + defaultStringifySearch({ unreadOnly: true, documentType: 'keep', ...invalid });
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  expect(await screen.findByRole('checkbox', { name: '未読のみ' })).toBeChecked();
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ unreadOnly: true, documentType: 'keep' });
  expect(router.state.location.search.unreadOnly).toBe(true);
});

test('unread URL history, detail selection and page/sort/folder changes preserve unknown operations', async () => {
  const api = mockApi();
  api.listDocuments.mockResolvedValue({ view: 'published', items: [listItem('published')], nextCursor: 'next' });
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, parentFolderId: folderId, name: '審査', revision: 3, capabilities: {} }], nextCursor: null, capabilities: {} });
  const { router, client } = renderAt('/documents' + defaultStringifySearch({ documentType: 'keep', unreadOnly: true, selectedDocumentId: documentId }));
  const store = metadataOperations(client);
  const snapshot = { status: 'unknown' as const, request: { operationId: 'unread-kept', expectedDocumentRevision: 7, set: { category: 'kept' }, unset: [], reason: 'keep' } };
  store.put(documentId, snapshot);
  try {
    const checkbox = await screen.findByRole('checkbox', { name: '未読のみ' });
    expect(checkbox).toBeChecked();
    await screen.findByRole('button', { name: /受入手順/ });
    fireEvent.click(screen.getByRole('button', { name: /詳細を開く/ }));
    await screen.findByRole('heading', { name: '受入手順', level: 1 });
    expect(String(router.state.location.search.returnTo)).toContain('unreadOnly=true');
    fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
    expect(await screen.findByRole('checkbox', { name: '未読のみ' })).toBeChecked();
    await screen.findByRole('button', { name: /受入手順/ });
    expect(router.state.location.search).toMatchObject({ unreadOnly: true, documentType: 'keep', selectedDocumentId: documentId });
    fireEvent.click(screen.getByRole('checkbox', { name: '未読のみ' }));
    fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
    await waitFor(() => expect(router.state.location.search).not.toHaveProperty('unreadOnly'));
    await act(async () => router.history.back());
    await waitFor(() => expect(screen.getByRole('checkbox', { name: '未読のみ' })).toBeChecked());
    await act(async () => router.history.forward());
    await waitFor(() => expect(screen.getByRole('checkbox', { name: '未読のみ' })).not.toBeChecked());
    fireEvent.click(screen.getByRole('checkbox', { name: '未読のみ' }));
    fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
    await waitFor(() => expect(router.state.location.search.unreadOnly).toBe(true));
    await screen.findByRole('button', { name: /受入手順/ });
    fireEvent.click(screen.getByRole('button', { name: '次のページ' }));
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].cursor).toBe('next'));
    fireEvent.change(screen.getByRole('combobox', { name: '並び順' }), { target: { value: 'title_asc' } });
    await waitFor(() => expect(router.state.location.search).not.toHaveProperty('cursor'));
    fireEvent.change(screen.getByRole('combobox', { name: '1ページあたりの件数' }), { target: { value: '25' } });
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].pageSize).toBe(25));
    fireEvent.click(await screen.findByRole('button', { name: '審査' }));
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].folderId).toBe(reviewFolderId));
    expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ unreadOnly: true, documentType: 'keep', sort: 'title_asc', pageSize: 25 });
    expect(store.get(documentId)).toBe(snapshot);
  } finally { store.clear(documentId); }
});

test('unread key keeps a delayed old published list out of current results', async () => {
  const api = mockApi();
  let resolveOld!: (value: DocumentList) => void;
  api.listDocuments.mockImplementation(query => query.unreadOnly === true ? Promise.resolve({ view: 'published', items: [{ ...listItem('published'), title: '現在の未読結果' }], nextCursor: null }) : new Promise(resolve => { resolveOld = resolve; }));
  renderAt('/documents?panel=closed');
  await waitFor(() => expect(api.listDocuments).toHaveBeenCalled());
  fireEvent.click(await screen.findByRole('checkbox', { name: '未読のみ' }));
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await screen.findByRole('button', { name: '現在の未読結果' });
  await act(async () => resolveOld({ view: 'published', items: [{ ...listItem('published'), title: '旧条件の遅延結果' }], nextCursor: null } as DocumentList));
  expect(screen.getByRole('button', { name: '現在の未読結果' })).toBeVisible();
  expect(screen.queryByRole('button', { name: '旧条件の遅延結果' })).not.toBeInTheDocument();
});

test('unread GET failure remains distinct from empty unread results and retries the same condition', async () => {
  const api = mockApi();
  api.listDocuments.mockRejectedValueOnce({ code: 'FORBIDDEN', status: 403 }).mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  renderAt('/documents?unreadOnly=true&panel=closed');
  expect(await screen.findByRole('alert')).toBeVisible();
  expect(screen.queryByRole('heading', { name: '文書がありません' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '再読み込み' }));
  expect(await screen.findByRole('heading', { name: '文書がありません' })).toBeVisible();
  expect(api.listDocuments.mock.lastCall![0]).toHaveProperty('unreadOnly', true);
});

test('normal editing and document links have fresh query targets without unread or cursor', async () => {
  mockApi();
  renderAt('/documents?unreadOnly=true&cursor=old');
  await screen.findByRole('button', { name: /受入手順/ });
  expect(screen.getByRole('link', { name: '編集作業' })).toHaveAttribute('href', '/documents' + defaultStringifySearch(validateListSearch({ view: 'authoring' })));
  expect(screen.getByRole('link', { name: '文書' })).toHaveAttribute('href', '/documents' + defaultStringifySearch(validateListSearch({ view: 'published' })));
});


const createdStartLabel = '作成日時の開始（含む）';
const createdEndLabel = '作成日時の終了（含まない）';
const preciseRange = { createdFrom: '2026-10-01T00:00:00.123456+09:00', createdBefore: '2026-10-02T00:00:00Z' };
const minuteRange = { createdFrom: '2026-10-01T00:00:00.000Z', createdBefore: '2026-10-02T00:00:00.000Z' };

test.each(['published', 'authoring', 'history'])('created calendar range applies in %s with explicit JST boundaries and keeps other filters', async view => {
  const api = mockApi();
  const conditions = { view, titleContains: 'keep', documentType: 'type', folderId, includeDescendants: true, sort: 'title_asc', pageSize: 25, cursor: 'old', selectedDocumentId: documentId, panel: 'closed' };
  const { router } = renderAt('/documents' + defaultStringifySearch(conditions));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(screen.getByText('文書自体の作成日時で絞り込みます。JST / UTC+09:00・分単位。開始を含み、終了を含みません。')).toBeVisible();
  const start = screen.getByLabelText(createdStartLabel), end = screen.getByLabelText(createdEndLabel);
  expect(start).toHaveAttribute('type', 'datetime-local');
  expect(end).toHaveAttribute('type', 'datetime-local');
  expect(start).toHaveAttribute('step', '60');
  const calls = api.listDocuments.mock.calls.length;
  fireEvent.change(start, { target: { value: '2026-10-01T09:00' } });
  fireEvent.change(end, { target: { value: '2026-10-02T09:00' } });
  expect(api.listDocuments).toHaveBeenCalledTimes(calls);
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ ...minuteRange, view }));
  const { cursor: oldCursor, ...keptConditions } = conditions; void oldCursor;
  expect(router.state.location.search).toMatchObject({ ...keptConditions, ...minuteRange });
  expect(router.state.location.search).not.toHaveProperty('cursor');
  fireEvent.click(screen.getByRole('button', { name: '属性の絞り込みを解除' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('documentType'));
  expect(router.state.location.search).toMatchObject(minuteRange);
  fireEvent.click(screen.getByRole('button', { name: '日時の条件を解除' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('createdFrom'));
  expect(router.state.location.search).not.toHaveProperty('createdBefore');
  expect(router.state.location.search).toMatchObject({ view, titleContains: 'keep', folderId, selectedDocumentId: documentId, panel: 'closed' });
});

test('created precise URL keeps raw values when applying other filters, replacing, cancelling or clearing one endpoint', async () => {
  const api = mockApi();
  const { router } = renderAt('/documents' + defaultStringifySearch({ ...preciseRange, documentType: 'keep', cursor: 'old', panel: 'closed' }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue(preciseRange.createdFrom);
  expect(screen.getByLabelText(createdStartLabel)).toHaveAttribute('readonly');
  expect(screen.getByLabelText(createdEndLabel)).toHaveValue(preciseRange.createdBefore);
  fireEvent.change(screen.getByRole('textbox', { name: '文書種別' }), { target: { value: 'new' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ ...preciseRange, documentType: 'new' }));
  fireEvent.click(screen.getByRole('button', { name: '作成日時の開始を指定し直す' }));
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue('');
  const calls = api.listDocuments.mock.calls.length;
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('作成日時の開始（含む）をJSTのカレンダーと時刻で指定してください。');
  expect(api.listDocuments).toHaveBeenCalledTimes(calls);
  expect(router.state.location.search).toMatchObject(preciseRange);
  fireEvent.change(screen.getByLabelText(createdStartLabel), { target: { value: '2026-10-03T09:00' } });
  fireEvent.click(screen.getByRole('button', { name: '作成日時の開始の指定し直しを取消' }));
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue(preciseRange.createdFrom);
  fireEvent.click(screen.getByRole('button', { name: '作成日時の開始を指定し直す' }));
  fireEvent.change(screen.getByLabelText(createdStartLabel), { target: { value: '2026-10-03T09:00' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ createdFrom: '2026-10-03T00:00:00.000Z', createdBefore: preciseRange.createdBefore }));
  fireEvent.click(screen.getByRole('button', { name: '作成日時の開始を解除' }));
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(router.state.location.search).not.toHaveProperty('createdFrom'));
  expect(router.state.location.search.createdBefore).toBe(preciseRange.createdBefore);
});

test.each(['createdFrom', 'createdBefore'])('empty merged created %s shares the unspecified query key and GET', async key => {
  const api = mockApi();
  const { router, client } = renderAt('/documents' + defaultStringifySearch({ [key]: '', panel: 'closed' }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty(key);
  const query = client.getQueryCache().findAll({ queryKey: ['documents'] })[0]!;
  expect(query.queryKey[1]).toHaveProperty(key, undefined);
  const calls = api.listDocuments.mock.calls.length;
  await act(async () => { await router.navigate({ to: '/documents', search: { view: 'published', includeDescendants: false, sort: 'published_at_desc', pageSize: 50, panel: 'closed' } }); });
  expect(client.getQueryCache().findAll({ queryKey: ['documents'] })).toEqual([query]);
  expect(api.listDocuments).toHaveBeenCalledTimes(calls);
});


test.each([
  ['2026-10-01T00:00:00.000Z', '2026-10-01T09:00', 'datetime-local'],
  ['2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', 'text'],
  ['2026-10-01T00:00:00.001Z', '2026-10-01T00:00:00.001Z', 'text'],
  ['2026-10-01T09:00:00.000+09:00', '2026-10-01T09:00:00.000+09:00', 'text'],
  ['2026-10-01t00:00:00z', '2026-10-01t00:00:00z', 'text'],
  ['2016-12-31T23:59:60Z', '2016-12-31T23:59:60Z', 'text'],
  ['2026-02-30T00:00:00.000Z', '2026-02-30T00:00:00.000Z', 'text'],
  ['invalid', 'invalid', 'text'],
])('created URL expands into minute calendar only after exact helper roundtrip: %p', async (raw, displayed, type) => {
  const api = mockApi();
  renderAt('/documents' + defaultStringifySearch({ createdFrom: raw, panel: 'closed' }));
  await screen.findByRole('button', { name: /受入手順/ });
  const input = screen.getByLabelText(createdStartLabel);
  expect(input).toHaveValue(displayed);
  expect(input).toHaveAttribute('type', type);
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0].createdFrom).toBe(raw));
});

test('created URL pair changes invalidate both previous drafts and history restores applied fields', async () => {
  const api = mockApi();
  const { router } = renderAt('/documents' + defaultStringifySearch({ ...minuteRange, panel: 'closed' }));
  await screen.findByRole('button', { name: /受入手順/ });
  fireEvent.change(screen.getByLabelText(createdStartLabel), { target: { value: '2026-10-09T09:00' } });
  // Changing just the opposite URL endpoint establishes a new pair; discard the unsent old start.
  const changed = { createdFrom: minuteRange.createdFrom, createdBefore: '2026-10-05T00:00:00.000Z' };
  await act(async () => { await router.navigate({ to: '/documents', search: { ...router.state.location.search, ...changed } as never }); });
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue('2026-10-01T09:00');
  expect(screen.getByLabelText(createdEndLabel)).toHaveValue('2026-10-05T09:00');
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).toMatchObject(changed));
  await act(async () => router.history.back());
  await waitFor(() => expect(screen.getByLabelText(createdEndLabel)).toHaveValue('2026-10-02T09:00'));
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue('2026-10-01T09:00');
  await act(async () => router.history.forward());
  await waitFor(() => expect(screen.getByLabelText(createdEndLabel)).toHaveValue('2026-10-05T09:00'));
});

test.each([
  [{ createdFrom: 1, category: 'keep', unreadOnly: true }, { category: 'keep', unreadOnly: true }],
  [{ createdBefore: 'a'.repeat(129), documentType: [1], unreadOnly: '' }, {}],
  [{ createdFrom: preciseRange.createdFrom, category: [1], unreadOnly: '' }, { createdFrom: preciseRange.createdFrom }],
])('created route recovery clears only invalid groups and cursor: %p', async (conditions, kept) => {
  const api = mockApi();
  const originalResponse = globalThis.Response; globalThis.Response = class {} as typeof Response;
  const warn = jest.spyOn(console, 'warn').mockImplementation(() => {});
  const error = jest.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const { router } = renderAt('/documents' + defaultStringifySearch({ ...conditions, titleContains: 'keep', cursor: 'old', panel: 'closed' }));
    const link = await screen.findByRole('link', { name: '条件を解除して一覧へ戻る' });
    expect(api.listDocuments).not.toHaveBeenCalled();
    fireEvent.click(link);
    await screen.findByRole('button', { name: /受入手順/ });
    expect(router.state.location.search).toMatchObject({ titleContains: 'keep', ...kept });
    expect(router.state.location.search).not.toHaveProperty('cursor');
    expect(api.listDocuments.mock.lastCall![0]).toMatchObject(kept);
    if (!('createdFrom' in kept)) expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('createdFrom');
    expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('createdBefore');
  } finally { globalThis.Response = originalResponse; warn.mockRestore(); error.mockRestore(); }
});

test('created range detail return preserves exact instants and old-condition fallback', async () => {
  const api = mockApi();
  const returnTo = '/documents' + defaultStringifySearch({ ...preciseRange, unreadOnly: true, documentType: 'keep', pageSize: 0 });
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  await screen.findByRole('button', { name: /受入手順/ });
  expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ ...preciseRange, unreadOnly: true, documentType: 'keep', pageSize: 50 });
  expect(router.state.location.search).toMatchObject(preciseRange);
});

test('created invalid returnTo stops with a fixed byte reason instead of a default GET', async () => {
  const api = mockApi();
  const returnTo = '/documents' + defaultStringifySearch({ createdBefore: 'a'.repeat(129) });
  const { router } = renderAt(`/documents/${documentId}` + defaultStringifySearch({ view: 'published', returnTo }));
  await screen.findByRole('heading', { name: '受入手順', level: 1 });
  fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  expect(await screen.findByRole('alert')).toHaveTextContent('作成日時の終了（含まない）は128 UTF-8 bytes以下で入力してください。');
  expect(router.state.location.pathname).toBe(`/documents/${documentId}`);
  expect(api.listDocuments).not.toHaveBeenCalled();
});

test.each(['list', 'detail'])('oversized raw %s context never falls back to a broader list GET', async where => {
  const api = mockApi();
  const longList = '/documents' + defaultStringifySearch({ createdFrom: preciseRange.createdFrom, unknown: 'x'.repeat(81920), panel: 'closed' });
  const { router } = renderAt(where === 'list' ? longList : `/documents/${documentId}` + defaultStringifySearch({ returnTo: longList }));
  if (where === 'detail') {
    await screen.findByRole('heading', { name: '受入手順', level: 1 });
    fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
  }
  expect(await screen.findByRole('alert')).toHaveTextContent('一覧のURLが81920文字を超えています。URLの条件を短くして再度お試しください。');
  expect(api.listDocuments).not.toHaveBeenCalled();
  expect(router.state.location.pathname).toBe(where === 'list' ? '/documents' : `/documents/${documentId}`);
});


test('created conditions keep a delayed previous raw query out of the current list', async () => {
  const api = mockApi();
  let resolveOld!: (value: DocumentList) => void;
  api.listDocuments.mockImplementation(query => query.createdFrom === minuteRange.createdFrom
    ? Promise.resolve({ view: 'published', items: [{ ...listItem('published'), title: '現在の日時結果' }], nextCursor: null })
    : new Promise(resolve => { resolveOld = resolve; }));
  renderAt('/documents?panel=closed');
  await waitFor(() => expect(api.listDocuments).toHaveBeenCalled());
  fireEvent.change(screen.getByLabelText(createdStartLabel), { target: { value: '2026-10-01T09:00' } });
  fireEvent.click(screen.getByRole('button', { name: '絞り込む' }));
  await screen.findByRole('button', { name: '現在の日時結果' });
  await act(async () => resolveOld({ view: 'published', items: [{ ...listItem('published'), title: '旧日時の遅延結果' }], nextCursor: null } as DocumentList));
  expect(screen.getByRole('button', { name: '現在の日時結果' })).toBeVisible();
  expect(screen.queryByRole('button', { name: '旧日時の遅延結果' })).not.toBeInTheDocument();
});

test.each([
  [{ type: 'about:blank', title: 'synthetic-private-title', code: 'VALIDATION_FAILED', status: 422, retryable: false, traceId: 'synthetic-created-422', details: { createdFrom: 'synthetic-private-date' } }, '入力内容を確認してください'],
  [new Error('synthetic-private-network'), '文書サービスに接続できません'],
])('created failure keeps raw conditions distinct from empty results and retries exact query: %p', async (failure, reason) => {
  const api = mockApi();
  api.listDocuments.mockRejectedValueOnce(failure).mockResolvedValue({ view: 'published', items: [], nextCursor: null });
  const { router } = renderAt('/documents' + defaultStringifySearch({ ...preciseRange, documentType: 'keep', panel: 'closed' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(reason as string);
  expect(screen.queryByRole('heading', { name: '文書がありません' })).not.toBeInTheDocument();
  expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
  expect(router.state.location.search).toMatchObject(preciseRange);
  expect(screen.getByLabelText(createdStartLabel)).toHaveValue(preciseRange.createdFrom);
  fireEvent.click(screen.getByRole('button', { name: '再読み込み' }));
  expect(await screen.findByRole('heading', { name: '文書がありません' })).toBeVisible();
  expect(api.listDocuments.mock.lastCall![0]).toMatchObject(preciseRange);
  fireEvent.click(screen.getByRole('button', { name: '日時の条件を解除' }));
  await waitFor(() => expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('createdFrom'));
  expect(api.listDocuments.mock.lastCall![0]).not.toHaveProperty('createdBefore');
  expect(api.listDocuments.mock.lastCall![0]).toHaveProperty('documentType', 'keep');
});

test('created changes preserve detail selection, pagination and all unknown operation stores', async () => {
  const api = mockApi();
  api.listDocuments.mockResolvedValue({ view: 'published', items: [listItem('published')], nextCursor: 'next-created' });
  api.listFolderChildren.mockResolvedValue({ items: [{ folderId: reviewFolderId, parentFolderId: folderId, name: '審査', revision: 3, capabilities: {} }], nextCursor: null, capabilities: {} });
  const { router, client } = renderAt('/documents' + defaultStringifySearch({ ...preciseRange, unreadOnly: true, documentType: 'keep', selectedDocumentId: documentId }));
  const metadataStore = metadataOperations(client), rootStore = rootFolderOperations(client), renameStore = folderRenameOperations(client);
  const metadataSnapshot = { status: 'unknown' as const, request: { operationId: 'created-kept', expectedDocumentRevision: 7, set: { category: 'kept' }, unset: [], reason: 'keep' } };
  const rootSnapshot = { status: 'unknown' as const, request: { operationId: 'created-root-kept', folderId: reviewFolderId, parentFolderId: folderId, expectedParentRevision: 1, name: 'kept', reason: 'keep' } };
  const renameSnapshot = { status: 'unknown' as const, targetFolderId: reviewFolderId, request: { operationId: 'created-rename-kept', expectedFolderRevision: 3, name: 'kept', reason: 'keep' }, context: { kind: 'selected' as const, folderId: reviewFolderId, sourceParentId: folderId, pageLimit: 1, name: '審査' }, currentName: '審査', expectedChanged: true };
  const registrationSnapshot = { state: 'unknown' as const, ids: { documentId, documentVersionId: versionId, fileId: revisionId } };
  metadataStore.put(documentId, metadataSnapshot); rootStore.put(rootSnapshot); renameStore.put(renameSnapshot); saveCreationReceipt(registrationSnapshot);
  try {
    await screen.findByRole('button', { name: /受入手順/ });
    fireEvent.click(screen.getByRole('button', { name: /詳細を開く/ }));
    await screen.findByRole('heading', { name: '受入手順', level: 1 });
    const returnUrl = new URL(String(router.state.location.search.returnTo), 'http://synthetic.invalid');
    expect(returnUrl.searchParams.get('createdFrom')).toBe(preciseRange.createdFrom);
    expect(returnUrl.searchParams.get('createdBefore')).toBe(preciseRange.createdBefore);
    fireEvent.click(screen.getByRole('button', { name: /一覧へ戻る/ }));
    await screen.findByRole('button', { name: /受入手順/ });
    expect(router.state.location.search).toMatchObject({ ...preciseRange, selectedDocumentId: documentId, unreadOnly: true });
    fireEvent.click(screen.getByRole('button', { name: '次のページ' }));
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].cursor).toBe('next-created'));
    fireEvent.change(screen.getByRole('combobox', { name: '並び順' }), { target: { value: 'title_asc' } });
    await waitFor(() => expect(router.state.location.search).not.toHaveProperty('cursor'));
    fireEvent.change(screen.getByRole('combobox', { name: '1ページあたりの件数' }), { target: { value: '25' } });
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].pageSize).toBe(25));
    fireEvent.click(await screen.findByRole('button', { name: '審査' }));
    await waitFor(() => expect(api.listDocuments.mock.lastCall![0].folderId).toBe(reviewFolderId));
    expect(api.listDocuments.mock.lastCall![0]).toMatchObject({ ...preciseRange, unreadOnly: true, documentType: 'keep' });
    expect(metadataStore.get(documentId)).toBe(metadataSnapshot);
    expect(rootStore.get()).toBe(rootSnapshot); expect(renameStore.get()).toBe(renameSnapshot);
    expect(readCreationReceipt()).toEqual(registrationSnapshot);
  } finally {
    await act(async () => {
      metadataStore.clear(documentId);
      rootStore.put({ ...rootSnapshot, status: 'rejected' }); rootStore.clearSettled(rootStore.get()!);
      renameStore.put({ ...renameSnapshot, status: 'rejected' }); renameStore.clearSettled(renameStore.get()!);
    });
    clearCreationReceipt();
  }
});

test.each(['基準原本を確認', '対象原本を確認'])('Folder ACL送信後の詳細routeで%sのBlobと成功receiptが同tickでも保存しない', async label => {
  const api = mockApi(); const { client, router } = renderAt('/documents?view=published');
  await screen.findByRole('button', { name: '受入手順' });
  let resolveReceipt!: (value: { operationId: string; resourceId: string; resultingRevision: number; changed: boolean; occurredAt: string }) => void;
  const receipt = new Promise<{ operationId: string; resourceId: string; resultingRevision: number; changed: boolean; occurredAt: string }>(resolve => { resolveReceipt = resolve; });
  const running = sendFolderAccessPolicyOperation({ store: folderAccessPolicyOperations(client), targetFolderId: reviewFolderId, context: { kind: 'selected', folderId: reviewFolderId, sourceParentId: folderId, pageLimit: 1, name: '資料' }, request: { operationId: 'policy', expectedPolicyRevision: 7, mode: 'inherit', reason: '理由' }, send: () => receipt, invalidate: () => refreshFolderMoveReads(client) });
  await act(async () => { await router.navigate({ to: '/documents/$documentId', params: { documentId }, search: validateDetailSearch({ view: 'published', tab: 'compare' }) }); });
  let resolveBlob!: (value: Blob) => void; api.downloadVersionFile.mockReturnValue(new Promise<Blob>(resolve => { resolveBlob = resolve; }));
  const create = jest.fn().mockReturnValue('blob:synthetic'); Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: create }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); const click = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(await screen.findByRole('button', { name: label }));
  api.getDocument.mockRejectedValue(new Error('current read denied'));
  await act(async () => { resolveReceipt({ operationId: 'policy', resourceId: reviewFolderId, resultingRevision: 8, changed: true, occurredAt: '2026-10-07T00:00:00Z' }); resolveBlob(new Blob(['late'])); await running; });
  expect(folderAccessPolicyOperations(client).get()?.status).toBe('succeeded'); expect(create).not.toHaveBeenCalled(); expect(click).not.toHaveBeenCalled(); client.clear();
});

test.each(['invalidate', 'refetch', 'reset', 'remove'] as const)('比較原本の旧Blobを%s後の同値fresh readで復活させない', async kind => {
  const api = mockApi(); const h = renderAt(`/documents/${documentId}?view=published&tab=compare`);
  let resolveBlob!: (value: Blob) => void; api.downloadVersionFile.mockReturnValue(new Promise<Blob>(resolve => { resolveBlob = resolve; }));
  const create = jest.fn().mockReturnValue('blob:synthetic'); Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: create }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); const click = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(await screen.findByRole('button', { name: '基準原本を確認' }));
  const signal = api.downloadVersionFile.mock.calls[0]![1].signal as AbortSignal;
  await act(async () => {
    if (kind === 'invalidate') await h.client.invalidateQueries({ queryKey: ['revision-comparison'] });
    else if (kind === 'refetch') await h.client.refetchQueries({ queryKey: ['revision-comparison'] });
    else if (kind === 'reset') await h.client.resetQueries({ queryKey: ['revision-comparison'] });
    else h.client.removeQueries({ queryKey: ['revision-comparison'] });
  });
  await act(async () => { resolveBlob(new Blob(['old before ACL reset'])); });
  expect(create).not.toHaveBeenCalled(); expect(click).not.toHaveBeenCalled(); expect(signal.aborted).toBe(true);
  if (kind === 'invalidate') { api.downloadVersionFile.mockResolvedValueOnce(new Blob(['fresh'])); fireEvent.click(await screen.findByRole('button', { name: '基準原本を確認' })); await waitFor(() => expect(create).toHaveBeenCalledTimes(1)); expect(click).toHaveBeenCalledTimes(1); }
  h.client.clear();
});
test('比較原本の旧BlobをACL全read reset後の同値fresh readで復活させない', async () => {
  const api = mockApi(); const h = renderAt(`/documents/${documentId}?view=published&tab=compare`);
  let resolveBlob!: (value: Blob) => void; api.downloadVersionFile.mockReturnValue(new Promise<Blob>(resolve => { resolveBlob = resolve; }));
  const create = jest.fn().mockReturnValue('blob:synthetic'); Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: create }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); const click = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(await screen.findByRole('button', { name: '基準原本を確認' }));
  const signal = api.downloadVersionFile.mock.calls[0]![1].signal as AbortSignal;
  await act(async () => { await refreshFolderMoveReads(h.client); });
  await act(async () => { resolveBlob(new Blob(['old before ACL reset'])); });
  expect(create).not.toHaveBeenCalled(); expect(click).not.toHaveBeenCalled(); expect(signal.aborted).toBe(true); h.client.clear();
});

test.each(['基準原本を確認', '対象原本を確認'])('Document ACL送信後の詳細routeで%sのBlobと成功receiptが同tickでも保存しない', async label => {
  const api = mockApi(); const { client, router } = renderAt('/documents?view=published');
  await screen.findByRole('button', { name: '受入手順' });
  let resolveReceipt!: (value: { operationId: string; resourceId: string; resultingRevision: number; changed: boolean; occurredAt: string }) => void;
  const receipt = new Promise<{ operationId: string; resourceId: string; resultingRevision: number; changed: boolean; occurredAt: string }>(resolve => { resolveReceipt = resolve; });
  const running = sendDocumentAccessPolicyOperation({ store: documentAccessPolicyOperations(client), targetDocumentId: documentId, context: { documentId, title: '資料', view: 'published' }, request: { operationId: 'policy', expectedPolicyRevision: 7, mode: 'inherit', reason: '理由' }, send: () => receipt, invalidate: () => refreshFolderMoveReads(client) });
  await act(async () => { await router.navigate({ to: '/documents/$documentId', params: { documentId }, search: validateDetailSearch({ view: 'published', tab: 'compare' }) }); });
  let resolveBlob!: (value: Blob) => void; api.downloadVersionFile.mockReturnValue(new Promise<Blob>(resolve => { resolveBlob = resolve; }));
  const create = jest.fn().mockReturnValue('blob:synthetic'); Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: create }); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: jest.fn() }); const click = jest.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  fireEvent.click(await screen.findByRole('button', { name: label }));
  api.getDocument.mockRejectedValue(new Error('current read denied'));
  await act(async () => { resolveReceipt({ operationId: 'policy', resourceId: documentId, resultingRevision: 8, changed: true, occurredAt: '2026-10-07T00:00:00Z' }); resolveBlob(new Blob(['late'])); await running; });
  expect(documentAccessPolicyOperations(client).get()?.status).toBe('succeeded'); expect(create).not.toHaveBeenCalled(); expect(click).not.toHaveBeenCalled(); client.clear();
});
