import { useEffect, useMemo, useRef, useState } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useNavigate, useRouterState, useSearch } from '@tanstack/react-router';
import { documentApi, type DocumentList, type Folder } from '../application/document-workspace';
import { ApiFeedback, LoadingState } from '../components/shared/ApiFeedback';
import { AppShell } from '../components/app-shell/AppShell';
import { OriginalVersionDownload } from '../components/shared/OriginalVersionDownload';
import { DocumentRegistration } from '../components/document/DocumentRegistration';
import { RootFolderCreate } from '../components/document/RootFolderCreate';
import type { ListSearch } from '../application/search-state';
import { documentListStatusLabel } from '../view-model/document-status';
import { formatDateTime as formatDate } from '../view-model/date-time';
import styles from './DocumentWorkspace.module.css';

type DocumentRow = DocumentList['items'][number];
type FolderItem = Folder;

export function DocumentHomePage() {
  const search = useSearch({ from: '/documents' }) as ListSearch;
  const navigate = useNavigate({ from: '/documents' });
  const currentUrl = useRouterState({ select: (state) => state.location.href });
  const queryClient = useQueryClient();
  const [chosenFolder, setChosenFolder] = useState<Folder | undefined>();
  const [draftTitle, setDraftTitle] = useState(search.titleContains ?? '');
  useEffect(() => setDraftTitle(search.titleContains ?? ''), [search.titleContains]);
  const listQuery = useQuery({
    queryKey: ['documents', {
      view: search.view,
      titleContains: search.titleContains,
      folderId: search.folderId,
      includeDescendants: search.includeDescendants,
      sort: search.sort,
      pageSize: search.pageSize,
      cursor: search.cursor,
    }],
    queryFn: () => documentApi.listDocuments({
      view: search.view,
      sort: search.sort,
      pageSize: search.pageSize,
      ...(search.titleContains ? { titleContains: search.titleContains } : {}),
      ...(search.folderId ? { folderId: search.folderId } : {}),
      ...(search.includeDescendants ? { includeDescendants: true } : {}),
      ...(search.cursor ? { cursor: search.cursor } : {}),
    }),
  });
  const rootQuery = useQuery({ queryKey: ['folder-tree', 'root'], queryFn: documentApi.getRootFolder });
  const registrationFolderQuery = useQuery({
    queryKey: ['folder-tree', search.folderId],
    queryFn: () => documentApi.listFolderChildren(search.folderId!),
    enabled: Boolean(search.folderId),
  });
  const registrationFolder = search.folderId
    ? { folderId: search.folderId, name: chosenFolder?.folderId === search.folderId ? chosenFolder.name : `選択中のフォルダー（${search.folderId}）` }
    : rootQuery.data;
  const registrationCapability = search.folderId ? registrationFolderQuery.data?.capabilities.createDocument : rootQuery.data?.capabilities.createDocument;
  const items = useMemo(() => listQuery.data?.items ?? [], [listQuery.data?.items]);
  const panelOpen = search.panel === 'open' && search.view !== 'history';
  const selected = items.find((item) => item.documentId === search.selectedDocumentId) ?? (panelOpen ? items[0] : undefined);
  const detailView = search.view === 'authoring' ? 'authoring' : 'published';
  const selectedDetailQuery = useQuery({
    queryKey: ['document', selected?.documentId, detailView],
    queryFn: () => documentApi.getDocument(selected!.documentId, detailView),
    enabled: Boolean(selected && panelOpen),
  });
  const selectedVersionQuery = useQuery({
    queryKey: ['document-version', selected?.documentId, selected?.displayVersion.versionId, detailView],
    queryFn: () => documentApi.getDocumentVersion(selected!.documentId, selected!.displayVersion.versionId, detailView),
    enabled: Boolean(selected && panelOpen),
  });

  function updateSearch(patch: Partial<ListSearch>) {
    void navigate({
      search: (previous) => {
        const next = { ...previous, ...patch } as ListSearch;
        if (patch.cursor === undefined && Object.prototype.hasOwnProperty.call(patch, 'cursor')) delete next.cursor;
        if (patch.folderId === undefined && Object.prototype.hasOwnProperty.call(patch, 'folderId')) delete next.folderId;
        if (patch.titleContains === undefined && Object.prototype.hasOwnProperty.call(patch, 'titleContains')) delete next.titleContains;
        if (patch.selectedDocumentId === undefined && Object.prototype.hasOwnProperty.call(patch, 'selectedDocumentId')) delete next.selectedDocumentId;
        return next;
      },
    });
  }

  const selectDocument = (documentId: string) => {
    updateSearch({ selectedDocumentId: documentId, panel: 'open' });
  };

  const columns = useMemo<ColumnDef<DocumentRow>[]>(() => [
    {
      id: 'title',
      header: '文書名',
      cell: ({ row }) => (
        <button
          type="button"
          className={styles.documentTitle}
          aria-pressed={row.original.documentId === selected?.documentId}
          data-document-id={row.original.documentId}
          onClick={() => selectDocument(row.original.documentId)}
        >
          <span>{row.original.title}</span>
          {row.original.readState.isRead === false && <span className={styles.unreadMark}>未読</span>}
        </button>
      ),
    },
    {
      id: 'state',
      header: '状態',
      cell: ({ row }) => <span className={styles.stateBadge}>{documentListStatusLabel(row.original)}</span>,
    },
    {
      id: 'folder',
      header: 'フォルダー',
      cell: ({ row }) => row.original.folderName ?? 'すべての文書',
    },
    {
      id: 'version',
      header: '版',
      cell: ({ row }) => row.original.displayVersion.versionNo,
    },
    {
      id: 'timestamp',
      header: search.view === 'authoring' ? '更新日時' : '公開日時',
      cell: ({ row }) => (
        <time dateTime={row.original.displayTimestamp.value}>
          {formatDate(row.original.displayTimestamp.value)}
        </time>
      ),
    },
    {
      id: 'read',
      header: '既読',
      cell: ({ row }) => row.original.readState.isRead === false
        ? <span className={styles.unreadMark}>● 未読</span>
        : row.original.readState.isRead === true ? '既読' : '—',
    },
  ], [selected?.documentId, search.view]);

  const table = useReactTable({ data: items, columns, getCoreRowModel: getCoreRowModel() });
  const tableScrollerRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: table.getRowModel().rows.length,
    getScrollElement: () => tableScrollerRef.current,
    estimateSize: () => 48,
    overscan: 5,
  });
  const selectedRowIndex = selected
    ? table.getRowModel().rows.findIndex((row) => row.original.documentId === selected.documentId)
    : -1;
  const virtualItems = virtualizer.getVirtualItems();
  // A zero-sized viewport can occur before layout (and in non-layout test DOMs).
  // Keep the first viewport visible until the virtualizer receives real dimensions.
  const fallbackIndexes = Array.from(new Set([
    ...table.getRowModel().rows.slice(0, 12).map((_, index) => index),
    ...(selectedRowIndex >= 0 ? [selectedRowIndex] : []),
  ]));
  const renderedItems = virtualItems.length > 0
    ? virtualItems
    : fallbackIndexes.map((index) => ({ key: table.getRowModel().rows[index]!.id, index, start: index * 48 }));

  useEffect(() => {
    if (!panelOpen || selectedRowIndex < 0 || !selected) return;
    virtualizer.scrollToIndex(selectedRowIndex, { align: 'auto' });
    const frame = requestAnimationFrame(() => {
      document.querySelector<HTMLButtonElement>(`[data-document-id="${selected.documentId}"]`)?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [panelOpen, selected?.documentId, selectedRowIndex]);

  function openDetail(documentId: string) {
    void navigate({
      to: '/documents/$documentId',
      params: { documentId },
      search: {
        tab: 'overview',
        view: detailView,
        returnTo: currentUrl,
      },
    });
  }

  const panel = selected ? (
    <div className={styles.panelContent}>
      <div className={styles.panelHeader}>
        <div><small>選択中の文書</small><h2>{selected.title}</h2></div>
        <button type="button" aria-label="詳細パネルを閉じる" onClick={() => { updateSearch({ panel: 'closed' }); document.querySelector<HTMLButtonElement>(`[data-document-id="${selected.documentId}"]`)?.focus(); }}>×</button>
      </div>
      {selectedDetailQuery.isPending && <LoadingState label="文書情報を読み込み中" />}
      {selectedDetailQuery.error && <ApiFeedback error={selectedDetailQuery.error} onRetry={() => void selectedDetailQuery.refetch()} />}
      <div className={styles.panelVersion}><span className={styles.stateBadge}>{documentListStatusLabel(selected)}</span><span>Version {selected.displayVersion.versionNo}</span></div>
      <dl className={styles.summaryList}>
        <dt>フォルダー</dt><dd>{selected.folderName ?? rootQuery.data?.name ?? 'すべての文書'}</dd>
        {typeof selectedDetailQuery.data?.metadata?.owning_department === 'string' && <><dt>所管部署</dt><dd>{selectedDetailQuery.data.metadata.owning_department}</dd></>}
        {typeof selectedDetailQuery.data?.metadata?.document_type === 'string' && <><dt>文書種別</dt><dd>{selectedDetailQuery.data.metadata.document_type}</dd></>}
        <dt>{selected.displayTimestamp.kind === 'workingUpdatedAt' ? '更新日時' : '公開日時'}</dt><dd>{formatDate(selected.displayTimestamp.value)}</dd>
      </dl>
      <section className={styles.panelSection}>
        <h3>現行ファイル</h3>
        {selected.displayVersion.fileSummary.primary ? (
          <div className={styles.fileCard}>
            <span className={styles.fileMark} aria-hidden="true">FILE</span>
            <span><strong>{selected.displayVersion.fileSummary.primary.displayName}</strong><small>{selected.displayVersion.fileSummary.primary.mediaType}{typeof selected.displayVersion.fileSummary.primary.sizeBytes === 'number' ? ` · ${formatBytes(selected.displayVersion.fileSummary.primary.sizeBytes)}` : ''}</small></span>
          </div>
        ) : <p className={styles.muted}>原本ファイルはありません。</p>}
        {selectedVersionQuery.data?.capabilities.download.status === 'available' && <OriginalVersionDownload documentId={selected.documentId} versionId={selected.displayVersion.versionId} purpose={detailView} label="ファイルを取得" />}
      </section>
      <section className={styles.panelSection}>
        <h3>版の概要</h3>
        <dl className={styles.summaryList}><dt>正式改訂</dt><dd>{selected.displayRevision?.label ?? '正式改訂なし'}</dd><dt>ファイル数</dt><dd>{selected.displayVersion.fileSummary.authoritativeItemCount}</dd></dl>
      </section>
      {selectedDetailQuery.data && <CapabilitySummary document={selectedDetailQuery.data} />}
      <button className={styles.primaryButton} type="button" onClick={() => openDetail(selected.documentId)}>詳細を開く <span aria-hidden="true">→</span></button>
    </div>
  ) : (
    <p className={styles.emptyPanel}>一覧から文書を選択すると、改訂とコンテンツ版の情報が表示されます。</p>
  );

  const navigationContent = (
    <section className={styles.folderRail} aria-label="フォルダー">
      <h2>フォルダー</h2>
      <RootFolderCreate root={rootQuery.data} readReady={rootQuery.isSuccess && !rootQuery.isFetching} contextKey={currentUrl} reload={async () => {
        const result = await rootQuery.refetch({ throwOnError: true });
        if (!result.data || result.isError) throw new Error('System Rootを取得できません。');
        return result.data;
      }} />
      {rootQuery.isPending && <LoadingState label="フォルダーを読み込み中" />}
      {rootQuery.error && <ApiFeedback error={rootQuery.error} onRetry={() => void rootQuery.refetch()} />}
      {rootQuery.data && (
        <ul className={styles.folderTree}>
          <FolderNode
            folder={rootQuery.data}
            selectedFolderId={search.folderId}
            onSelect={(folderId, folder) => { setChosenFolder(folder); updateSearch({ folderId, includeDescendants: false, cursor: undefined }); }}
            isRoot
          />
        </ul>
      )}
      <label className={styles.checkLine}>
        <input type="checkbox" checked={search.includeDescendants} disabled={!search.folderId} onChange={(event) => updateSearch({ includeDescendants: event.target.checked, cursor: undefined })} />
        配下も含める
      </label>
    </section>
  );

  return (
    <AppShell
      activeNavigation={search.view === 'authoring' ? 'editing' : 'documents'}
      contextPanel={panel}
      contextPanelLabel="選択中の文書"
      headerContext={<><span>文書</span>{selected?.folderName && <>　/　{selected.folderName}</>}</>}
      navigationContent={navigationContent}
      showContextPanel={Boolean(selected && panelOpen)}
    >
      <div className={styles.pageHeader}>
        <div>
          <h1>{selected?.folderName ? `${selected.folderName}の文書` : search.view === 'authoring' ? '編集作業' : '文書一覧'}</h1>
        </div>
        <DocumentRegistration folder={registrationFolder} capabilityKnown={Boolean(registrationCapability)} canCreate={registrationCapability?.status === 'available'} contextKey={currentUrl} onCreated={result => {
          void queryClient.invalidateQueries({ queryKey: ['documents'] });
          void navigate({ to: '/documents/$documentId', params: { documentId: result.documentId }, search: { tab: 'versions', view: 'authoring', versionId: result.documentVersionId, returnTo: currentUrl } });
        }} />
      </div>

      <div className={styles.listLayout}>
        <section className={styles.listMain} aria-label="文書">
          <form className={styles.filterBar} onSubmit={(event) => { event.preventDefault(); updateSearch({ titleContains: draftTitle || undefined, cursor: undefined }); }}>
            <label className={styles.searchField}>
              <span>文書名で絞り込み</span>
              <input type="search" value={draftTitle} onChange={(event) => setDraftTitle(event.target.value)} placeholder="文書名で検索" />
            </label>
            <button className={styles.secondaryButton} type="submit">絞り込む</button>
            <label className={styles.sortField}>
              <span>並び順</span>
              <select value={search.sort} onChange={(event) => updateSearch({ sort: event.target.value as ListSearch['sort'], cursor: undefined })}>
                <option value="created_at_desc">作成日時が新しい順</option>
                <option value="title_asc">文書名の昇順</option>
                {search.view === 'published' && <option value="published_at_desc">公開日時が新しい順</option>}
              </select>
            </label>
          </form>

          {listQuery.isPending && <LoadingState label="文書を読み込み中" />}
          {listQuery.error && <ApiFeedback error={listQuery.error} onRetry={() => void listQuery.refetch()} />}
          {listQuery.data && items.length === 0 && (
            <div className={styles.emptyState}>
              <h2>文書がありません</h2>
              <p>検索条件やフォルダーを変更してください。</p>
            </div>
          )}
          {listQuery.data && items.length > 0 && (
              <div className={styles.tableCard}>
              <div className={styles.tableScroller} data-document-table-scroll ref={tableScrollerRef} role="region" aria-label="文書一覧、上下にスクロールできます" tabIndex={0}>
                <div role="table" aria-label="文書一覧" aria-rowcount={items.length + 1}>
                  {table.getHeaderGroups().map((group) => (
                    <div className={`${styles.tableRow} ${styles.tableHeader}`} role="row" key={group.id}>
                      {group.headers.map((header) => <div role="columnheader" key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</div>)}
                    </div>
                  ))}
                  <div className={styles.virtualRows} role="rowgroup" style={{ height: virtualizer.getTotalSize() }}>
                    {renderedItems.map((virtualRow) => {
                      const row = table.getRowModel().rows[virtualRow.index];
                      if (!row) return null;
                      return (
                        <div className={styles.tableRow} role="row" aria-selected={row.original.documentId === selected?.documentId} key={row.id} style={{ transform: `translateY(${virtualRow.start}px)` }}>
                          {row.getVisibleCells().map((cell) => <div role="cell" key={cell.id} className={cell.column.id === 'timestamp' ? styles.timestampCell : undefined}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</div>)}
                        </div>
                      );
                    })}
                  </div>
                </div>
              </div>
              <div className={styles.pagination}>
                <span>{items.length} 件を表示</span>
                <label>1ページあたり <select aria-label="1ページあたりの件数" value={search.pageSize} onChange={(event) => updateSearch({ pageSize: Number(event.target.value), cursor: undefined })}><option value={25}>25</option><option value={50}>50</option><option value={100}>100</option></select> 件</label>
                <button type="button" disabled={!search.cursor} onClick={() => updateSearch({ cursor: undefined })}>最初のページ</button>
                <button type="button" disabled={!listQuery.data.nextCursor} onClick={() => updateSearch({ cursor: listQuery.data.nextCursor ?? undefined })}>次のページ</button>
              </div>
            </div>
          )}
        </section>
      </div>
    </AppShell>
  );
}

function FolderNode({
  folder,
  selectedFolderId,
  onSelect,
  isRoot = false,
}: {
  folder: FolderItem;
  selectedFolderId?: string;
  onSelect: (folderId?: string, folder?: Folder) => void;
  isRoot?: boolean;
}) {
  const [expanded, setExpanded] = useState(isRoot);
  const queryClient = useQueryClient();
  // Keep paged data separate from the ordinary registration-capability query.
  // The existing parent prefix still invalidates both after a Root create.
  const queryKey = ['folder-tree', folder.folderId, 'pages'];
  const childrenQuery = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => documentApi.listFolderChildren(folder.folderId, pageParam),
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined,
    enabled: expanded,
  });
  const children = useMemo(() => {
    const unique = new Map<string, Folder>();
    // Live pages are not a snapshot: a moved/renamed row can reappear later.
    for (const page of childrenQuery.data?.pages ?? []) {
      for (const child of page.items) unique.set(child.folderId, child);
    }
    return [...unique.values()];
  }, [childrenQuery.data]);
  function restartChildren() {
    if (queryClient.getQueryState(queryKey)?.fetchStatus === 'fetching') return;
    // Reset only this read; never clear the QueryClient or unresolved mutations.
    void queryClient.resetQueries({ queryKey, exact: true });
  }
  return (
    <li>
      <div className={styles.folderEntry}>
        <button type="button" className={styles.expandButton} aria-label={`${folder.name}の子フォルダーを${expanded ? '閉じる' : '開く'}`} aria-expanded={expanded} onClick={() => setExpanded((value) => !value)}>{expanded ? '▾' : '▸'}</button>
        <button type="button" className={styles.folderButton} aria-current={(isRoot ? !selectedFolderId : selectedFolderId === folder.folderId) ? 'location' : undefined} onClick={() => onSelect(isRoot ? undefined : folder.folderId, folder)}>{folder.name}</button>
      </div>
      {childrenQuery.error && expanded && <ApiFeedback error={childrenQuery.error} onRetry={childrenQuery.isFetchNextPageError ? undefined : () => void childrenQuery.refetch({ cancelRefetch: false })} />}
      {expanded && children.length > 0 && (
        <ul className={styles.folderChildren}>
          {children.map((child) => <FolderNode key={child.folderId} folder={child} selectedFolderId={selectedFolderId} onSelect={onSelect} />)}
        </ul>
      )}
      {expanded && childrenQuery.hasNextPage && (!childrenQuery.error || childrenQuery.isFetchNextPageError) && (
        <button type="button" className={styles.secondaryButton} disabled={childrenQuery.isFetching}
          aria-label={`${folder.name}の子フォルダー${childrenQuery.isFetchNextPageError ? 'の続きを再試行' : 'をさらに表示'}`}
          onClick={() => { if (!childrenQuery.isFetching) void childrenQuery.fetchNextPage({ cancelRefetch: false }); }}>
          {childrenQuery.isFetchNextPageError ? '続きを再試行' : 'さらに表示'}
        </button>
      )}
      {expanded && (childrenQuery.hasNextPage || (childrenQuery.data?.pages.length ?? 0) > 1 || childrenQuery.error) && (
        <button type="button" className={styles.secondaryButton} disabled={childrenQuery.isFetching}
          aria-label={`${folder.name}の子フォルダーを最初から読み直す`} onClick={restartChildren}>最初から読み直す</button>
      )}
      {expanded && childrenQuery.isFetching && <span className={styles.folderLoading} role="status">読み込み中</span>}
    </li>
  );
}

function CapabilitySummary({ document }: { document: Awaited<ReturnType<typeof documentApi.getDocument>> }) {
  const capabilities = document.capabilities;
  const available = [
    capabilities.createVersion.status === 'available' && '新しい版を作成できます',
    capabilities.compareVersions.status === 'available' && '版を比較できます',
    capabilities.manageAccess.status === 'available' && 'アクセス設定を管理できます',
  ].filter(Boolean);
  return available.length > 0 ? <p className={styles.capabilityHint}>{available.join(' · ')}</p> : null;
}

function viewLabel(view: ListSearch['view']): string {
  return view === 'published' ? '公開中' : view === 'authoring' ? '編集中' : '履歴';
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
