import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { documentApi } from '../src/application/document-workspace';
import type { DocumentDetail, DocumentList, Version } from '../src/application/document-workspace';
import { DocumentDetailPage } from '../src/routes/DocumentDetailPage';
import { DocumentHomePage } from '../src/routes/DocumentHomePage';
import { validateDetailSearch, validateListSearch } from '../src/application/search-state';

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
  api.listDocuments.mockResolvedValue({ view: 'published', items: [listItem('published')], nextCursor: null });
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
  api.setDocumentAccessPolicy.mockResolvedValue({ operationId: 'op', resourceId: documentId, resultingRevision: 5, changed: true, occurredAt: '2026-10-01T02:00:00Z' });
  api.downloadVersionFile.mockResolvedValue(new Blob(['original']));
  return api;
}

function renderAt(entry: string) {
  const root = createRootRoute({ component: Outlet });
  const list = createRoute({ getParentRoute: () => root, path: '/documents', validateSearch: validateListSearch, component: DocumentHomePage });
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
  })));
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
  expect(screen.queryByText(documentId)).not.toBeInTheDocument();
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
