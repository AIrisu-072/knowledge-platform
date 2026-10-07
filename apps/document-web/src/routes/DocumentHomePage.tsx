import { DocumentAccessPolicyRecovery } from '../components/document/DocumentAccessPolicy';
import { documentAccessPolicyOperations } from '../application/document-access-policy';
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useNavigate, useRouter, useRouterState, useSearch } from '@tanstack/react-router';
import { problemFromUnknown } from '../application/problem-mapping';
import { documentApi, type DocumentList, type Folder } from '../application/document-workspace';
import { ApiFeedback, LoadingState } from '../components/shared/ApiFeedback';
import { AppShell } from '../components/app-shell/AppShell';
import { OriginalVersionDownload } from '../components/shared/OriginalVersionDownload';
import { DocumentHistoryPanel } from '../components/document/DocumentHistoryPanel';
import { DocumentRegistration } from '../components/document/DocumentRegistration';
import type { SelectedFolderContext } from '../application/document-root-folder';
import { FolderNode } from '../components/document/FolderNode';
import { FolderRename } from '../components/document/FolderRename';
import { folderRenameOperations } from '../application/document-folder-rename';
import { DocumentMove } from '../components/document/DocumentMove';
import { documentMoveOperations } from '../application/document-move';
import { FolderAccessPolicy } from '../components/document/FolderAccessPolicy';
import { folderAccessPolicyOperations } from '../application/document-folder-access-policy';
import { FolderMove } from '../components/document/FolderMove';
import { folderMoveOperations } from '../application/document-folder-move';
import { RootFolderCreate } from '../components/document/RootFolderCreate';
import { createdRangeFields, createdRangeLocalValue, createdRangeRouteError, initialCreatedRangeDraft, currentCreatedRangeDraft, resolveCreatedRangeDraft, documentListUrlError } from '../application/document-created-range';
import { validateListSearch, type ListSearch } from '../application/search-state';
import { metadataFilterFields, metadataFilterValidation, type MetadataFilters } from '../application/document-metadata-filters';
import { documentListStatusLabel } from '../view-model/document-status';
import { formatDateTime as formatDate } from '../view-model/date-time';
import styles from './DocumentWorkspace.module.css';

type DocumentRow = DocumentList['items'][number];

export function DocumentHomePage() {
  const search = useSearch({ from: '/documents' }) as ListSearch;
  const navigate = useNavigate({ from: '/documents' });
  const currentUrl = useRouterState({ select: (state) => state.location.href });
  const queryClient = useQueryClient();
  const router = useRouter();
  const [chosenFolder, setChosenFolder] = useState<Folder | undefined>();
  const [folderContext, setFolderContext] = useState<SelectedFolderContext>();
  const renameStore = folderRenameOperations(queryClient);
  const rename = useSyncExternalStore(renameStore.subscribe, renameStore.get);
  const documentMoveStore = documentMoveOperations(queryClient);
  const documentMove = useSyncExternalStore(documentMoveStore.subscribe, documentMoveStore.get);
  const documentPolicyStore = documentAccessPolicyOperations(queryClient);
  const documentPolicyOperation = useSyncExternalStore(documentPolicyStore.subscribe, documentPolicyStore.get);
  const policyStore = folderAccessPolicyOperations(queryClient);
  const policyOperation = useSyncExternalStore(policyStore.subscribe, policyStore.get);
  const moveStore = folderMoveOperations(queryClient);
  const move = useSyncExternalStore(moveStore.subscribe, moveStore.get);
  useEffect(() => {
    if (move?.status !== 'succeeded' && documentMove?.status !== 'succeeded' && policyOperation?.status !== 'succeeded' && documentPolicyOperation?.status !== 'succeeded') return;
    // Global authorization revision invalidates every query-external selection provenance.
    // Leave the current URL/navigation alone and require another actual tree selection.
    setChosenFolder(undefined); setFolderContext(undefined);
  }, [move?.status, move?.request.operationId, documentMove?.status, documentMove?.request.operationId, policyOperation?.status, policyOperation?.request.operationId, documentPolicyOperation?.status, documentPolicyOperation?.request.operationId]);
  useEffect(() => {
    if (rename?.status !== 'succeeded') return;
    const targetId = rename.targetFolderId;
    // Preserve the URL/filter and a different selection; discard only stale, query-external target copies.
    setChosenFolder(current => current?.folderId === targetId ? undefined : current);
    setFolderContext(current => current?.folderId === targetId ? undefined : current);
  }, [rename?.status, rename?.request.operationId]);
  const [selectionGeneration, setSelectionGeneration] = useState(0);
  const [draftUnread, setDraftUnread] = useState(search.unreadOnly === true);
  useEffect(() => setDraftUnread(search.unreadOnly === true), [search.unreadOnly]);
  const [draftTitle, setDraftTitle] = useState(search.titleContains ?? '');
  useEffect(() => setDraftTitle(search.titleContains ?? ''), [search.titleContains]);
  const [draftMetadata, setDraftMetadata] = useState<MetadataFilters>({ documentType: search.documentType ?? '', owningDepartment: search.owningDepartment ?? '', category: search.category ?? '' });
  const [filterError, setFilterError] = useState<string | null>(null);
  useEffect(() => {
    setDraftMetadata({ documentType: search.documentType ?? '', owningDepartment: search.owningDepartment ?? '', category: search.category ?? '' });
    setFilterError(null);
  }, [search.documentType, search.owningDepartment, search.category]);
  const [createdDraft, setCreatedDraft] = useState(() => initialCreatedRangeDraft(search));
  useEffect(() => {
    setCreatedDraft(initialCreatedRangeDraft(search));
    setFilterError(null);
  }, [search.createdFrom, search.createdBefore]);
  // A changed URL pair invalidates both drafts during render, before effect synchronization.
  const currentCreatedDraft = currentCreatedRangeDraft(search, createdDraft);
  const appliedFilterError = metadataFilterValidation(search) ?? createdRangeRouteError(search) ?? documentListUrlError(currentUrl);
  const listKey = ['documents', {
    view: search.view,
    unreadOnly: search.unreadOnly === true ? true : undefined,
    titleContains: search.titleContains,
    documentType: search.documentType || undefined,
    owningDepartment: search.owningDepartment || undefined,
    category: search.category || undefined,
    createdFrom: search.createdFrom || undefined,
    createdBefore: search.createdBefore || undefined,
    folderId: search.folderId,
    includeDescendants: search.includeDescendants,
    sort: search.sort,
    pageSize: search.pageSize,
    cursor: search.cursor,
  }] as const;
  const historyList = search.view === 'history';
  // Keep only the refusal across normal read resets; successful rows are never an authorization cache.
  const refusalKey = ['document-history-list-refusal', listKey[1]] as const;
  const refusal = useQuery<{ error: unknown } | null>({ queryKey: refusalKey, queryFn: skipToken, initialData: null, gcTime: Infinity });
  const listIdentity = JSON.stringify(listKey);
  useEffect(() => {
    if (!historyList) return;
    return () => {
      void queryClient.cancelQueries({ queryKey: listKey, exact: true }, { revert: false });
      queryClient.removeQueries({ queryKey: listKey, exact: true });
    };
  }, [queryClient, historyList, listIdentity]);
  const listQuery = useQuery({
    enabled: !appliedFilterError && (!historyList || !refusal.data),
    queryKey: listKey,
    ...(historyList ? { retry: false, refetchOnMount: 'always' as const } : {}),
    queryFn: async ({ signal }) => {
      const blocked = historyList && queryClient.getQueryData<{ error: unknown }>(refusalKey);
      if (blocked) throw blocked.error;
      try {
        const result = await documentApi.listDocuments({
          view: search.view,
          sort: search.sort,
          pageSize: search.pageSize,
          ...(search.unreadOnly === true ? { unreadOnly: true } : {}),
          ...(search.titleContains ? { titleContains: search.titleContains } : {}),
          ...(search.documentType ? { documentType: search.documentType } : {}),
          ...(search.owningDepartment ? { owningDepartment: search.owningDepartment } : {}),
          ...(search.category ? { category: search.category } : {}),
          ...(search.createdFrom ? { createdFrom: search.createdFrom } : {}),
          ...(search.createdBefore ? { createdBefore: search.createdBefore } : {}),
          ...(search.folderId ? { folderId: search.folderId } : {}),
          ...(search.includeDescendants ? { includeDescendants: true } : {}),
          ...(search.cursor ? { cursor: search.cursor } : {}),
        });
        if (historyList && result.view !== 'history') throw new Error('履歴一覧の応答を確認できません。');
        return result;
      } catch (error) {
        const problem = problemFromUnknown(error);
        if (historyList && !signal.aborted && problem && [401, 403, 404].includes(problem.status)) {
          queryClient.setQueryData(refusalKey, { error });
        }
        throw error;
      }
    },
  });
  const listState = useSyncExternalStore(
    listener => queryClient.getQueryCache().subscribe(listener),
    () => historyList ? queryClient.getQueryState<DocumentList>(listKey) : undefined,
  );
  const listError = historyList ? refusal.data?.error ?? listQuery.error : listQuery.error;
  const historyListReady = !appliedFilterError && !listError && listQuery.isSuccess && listQuery.fetchStatus === 'idle'
    && listQuery.isFetchedAfterMount && !listState?.isInvalidated && listQuery.data.view === 'history';
  function historySelectionReadable() {
    const state = queryClient.getQueryState<DocumentList>(listKey);
    let liveSearch: ListSearch;
    try { liveSearch = validateListSearch(router.state.location.search); } catch { return false; }
    const sameSelection = [...new Set([...Object.keys(search), ...Object.keys(liveSearch)])]
      .every(key => search[key as keyof ListSearch] === liveSearch[key as keyof ListSearch]);
    return sameSelection && liveSearch.view === 'history' && liveSearch.panel === 'open' && router.history.location.href === currentUrl && !queryClient.getQueryData(refusalKey)
      && state?.status === 'success' && !state.isInvalidated && state.fetchStatus === 'idle'
      && state.data === listQuery.data && state.data?.view === 'history'
      && state.data.items.some(item => item.documentId === search.selectedDocumentId);
  }
  function restartHistoryList() {
    if (queryClient.getQueryState(listKey)?.fetchStatus === 'fetching') return;
    void queryClient.cancelQueries({ queryKey: listKey, exact: true }, { revert: false });
    // Reset the successful data before clearing the barrier, never revive old rows.
    void queryClient.resetQueries({ queryKey: listKey, exact: true });
    queryClient.setQueryData(refusalKey, null);
  }

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
  const items = useMemo(() => appliedFilterError || historyList && !historyListReady ? [] : listQuery.data?.items ?? [], [listQuery.data?.items, appliedFilterError, historyList, historyListReady]);
  const panelOpen = search.panel === 'open';
  const selected = items.find((item) => item.documentId === search.selectedDocumentId) ?? (panelOpen && search.view !== 'history' ? items[0] : undefined);
  const detailView = search.view === 'authoring' ? 'authoring' : 'published';
  const selectedDetailQuery = useQuery({
    queryKey: ['document', selected?.documentId, detailView],
    queryFn: () => documentApi.getDocument(selected!.documentId, detailView),
    enabled: Boolean(selected && panelOpen && search.view !== 'history'),
  });
  const selectedVersionQuery = useQuery({
    queryKey: ['document-version', selected?.documentId, selected?.displayVersion.versionId, detailView],
    queryFn: () => documentApi.getDocumentVersion(selected!.documentId, selected!.displayVersion.versionId, detailView),
    enabled: Boolean(selected && panelOpen && search.view !== 'history'),
  });

  function updateSearch(patch: Partial<ListSearch>) {
    void navigate({
      search: (previous) => {
        const next = { ...previous, ...patch } as ListSearch;
        if (historyList && Object.keys(patch).some(key => key !== 'panel' && key !== 'selectedDocumentId')) {
          next.panel = 'closed';
        }
        if (patch.unreadOnly === undefined && Object.prototype.hasOwnProperty.call(patch, 'unreadOnly')) delete next.unreadOnly;
        if (patch.cursor === undefined && Object.prototype.hasOwnProperty.call(patch, 'cursor')) delete next.cursor;
        if (patch.folderId === undefined && Object.prototype.hasOwnProperty.call(patch, 'folderId')) delete next.folderId;
        for (const { key } of [...metadataFilterFields, ...createdRangeFields]) {
          if (patch[key] === undefined && Object.prototype.hasOwnProperty.call(patch, key)) delete next[key];
        }
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
      cell: ({ row }) => historyList && 'ended' in row.original
        ? <><span className={styles.stateBadge}>{row.original.ended ? '公開終了済み' : '公開終了していません'}</span><span>代表版: {row.original.displayVersion.lifecycleState}</span></>
        : <span className={styles.stateBadge}>{documentListStatusLabel(row.original)}</span>,
    },
    {
      id: 'folder',
      header: 'フォルダー',
      cell: ({ row }) => row.original.folderName ?? (search.view === 'history' ? '表示できません' : 'すべての文書'),
    },
    {
      id: 'version',
      header: '版',
      cell: ({ row }) => row.original.displayVersion.versionNo,
    },
    {
      id: 'timestamp',
      header: historyList ? '表示日時' : search.view === 'authoring' ? '更新日時' : '公開日時',
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
    const error = documentListUrlError(currentUrl);
    if (error) { setFilterError(error); return; }
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

  const panel = selected && historyList && 'ended' in selected && panelOpen ? (
    <DocumentHistoryPanel key={selected.documentId} document={selected} isDocumentReadable={historySelectionReadable} onClose={() => {
      updateSearch({ panel: 'closed' });
      document.querySelector<HTMLButtonElement>(`[data-document-id="${selected.documentId}"]`)?.focus();
    }} />
  ) : selected && !historyList ? (
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
      <DocumentMove contextKey={currentUrl} />
      <RootFolderCreate root={rootQuery.data} readReady={rootQuery.isSuccess && !rootQuery.isFetching} contextKey={`${currentUrl}:${selectionGeneration}`} selectedFolderId={search.folderId}
        selected={folderContext && folderContext.folderId === search.folderId && chosenFolder ? {
          context: folderContext, folder: { ...chosenFolder, parentFolderId: folderContext.sourceParentId,
            capabilities: registrationFolderQuery.data?.capabilities },
          readReady: registrationFolderQuery.isSuccess && !registrationFolderQuery.isFetching,
        } : undefined} reload={async () => {
        const result = await rootQuery.refetch({ throwOnError: true });
        if (!result.data || result.isError) throw new Error('System Rootを取得できません。');
        return result.data;
      }} />
      <FolderRename root={!search.folderId ? rootQuery.data : undefined} contextKey={`${currentUrl}:${selectionGeneration}`}
        selected={folderContext && folderContext.folderId === search.folderId && chosenFolder ? {
          context: folderContext, folder: { ...chosenFolder, parentFolderId: folderContext.sourceParentId,
            capabilities: registrationFolderQuery.data?.capabilities },
          readReady: registrationFolderQuery.isSuccess && !registrationFolderQuery.isFetching,
        } : undefined} />
      <FolderAccessPolicy root={rootQuery.data} contextKey={`${currentUrl}:${selectionGeneration}`}
        selected={folderContext && folderContext.folderId === search.folderId && chosenFolder ? {
          context: folderContext, folder: { ...chosenFolder, parentFolderId: folderContext.sourceParentId,
            capabilities: registrationFolderQuery.data?.capabilities },
          readReady: registrationFolderQuery.isSuccess && !registrationFolderQuery.isFetching,
        } : undefined} />
      <FolderMove root={rootQuery.data} contextKey={`${currentUrl}:${selectionGeneration}`}
        selected={folderContext && folderContext.folderId === search.folderId && chosenFolder ? {
          context: folderContext, folder: { ...chosenFolder, parentFolderId: folderContext.sourceParentId,
            capabilities: registrationFolderQuery.data?.capabilities },
          readReady: registrationFolderQuery.isSuccess && !registrationFolderQuery.isFetching,
        } : undefined} />
      {rootQuery.isPending && <LoadingState label="フォルダーを読み込み中" />}
      {rootQuery.error && <ApiFeedback error={rootQuery.error} onRetry={() => void rootQuery.refetch()} />}
      {rootQuery.data && (
        <ul className={styles.folderTree}>
          <FolderNode
            folder={rootQuery.data}
            selectedFolderId={search.folderId}
            onSelect={(folderId, folder, context) => { setChosenFolder(folder); setFolderContext(context); setSelectionGeneration(value => value + 1); updateSearch({ folderId, includeDescendants: false, cursor: undefined }); }}
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
      activeNavigation={search.view === 'history' ? 'history' : search.view === 'authoring' ? 'editing' : 'documents'}
      contextPanel={panel}
      contextPanelLabel={search.view === 'history' ? '文書履歴パネル' : '選択中の文書'}
      headerContext={<><span>文書</span>{selected?.folderName && <>　/　{selected.folderName}</>}</>}
      navigationContent={navigationContent}
      showContextPanel={Boolean(selected && panelOpen)}
    >
      <div className={styles.pageHeader}>
        <div>
          <h1>{search.view === 'history' ? '文書履歴' : selected?.folderName ? `${selected.folderName}の文書` : search.view === 'authoring' ? '編集作業' : '文書一覧'}</h1>
        </div>
        <DocumentRegistration folder={registrationFolder} capabilityKnown={Boolean(registrationCapability)} canCreate={registrationCapability?.status === 'available'} contextKey={currentUrl} onCreated={result => {
          void queryClient.invalidateQueries({ queryKey: ['documents'] });
          void navigate({ to: '/documents/$documentId', params: { documentId: result.documentId }, search: { tab: 'versions', view: 'authoring', versionId: result.documentVersionId, returnTo: currentUrl } });
        }} />
      </div>

      <DocumentAccessPolicyRecovery />
      <div className={styles.listLayout}>
        <section className={styles.listMain} aria-label="文書">
          <form className={styles.filterBar} onSubmit={(event) => {
            event.preventDefault();
            const created = resolveCreatedRangeDraft(currentCreatedDraft);
            const error = metadataFilterValidation(draftMetadata) ?? created.error;
            setFilterError(error);
            if (error || created.error !== null) return;
            updateSearch({ ...created.range, unreadOnly: search.view === 'published' && draftUnread ? true : undefined, titleContains: draftTitle || undefined, documentType: draftMetadata.documentType || undefined, owningDepartment: draftMetadata.owningDepartment || undefined, category: draftMetadata.category || undefined, cursor: undefined });
          }}>
            <label className={styles.searchField}>
              <span>文書名で絞り込み</span>
              <input type="search" value={draftTitle} onChange={(event) => setDraftTitle(event.target.value)} placeholder="文書名で検索" />
            </label>
            {metadataFilterFields.map(({ key, label }) => (
              <label className={styles.searchField} key={key}>
                <span>{label}</span>
                <input type="text" aria-label={label} aria-describedby="metadata-filter-help" value={draftMetadata[key] ?? ''} onChange={event => { setDraftMetadata(previous => ({ ...previous, [key]: event.target.value })); setFilterError(null); }} />
              </label>
            ))}
            {createdRangeFields.map(({ key, label, name }) => {
              const endpoint = currentCreatedDraft[key];
              const local = endpoint.intent === 'set' ? endpoint.local : endpoint.intent === 'clear' ? '' : createdRangeLocalValue(currentCreatedDraft.base[key]);
              return <div key={key} className={styles.searchField}>
                <label className={styles.searchField}>
                  <span>{label}</span>
                  {local === null
                    ? <input type="text" aria-label={label} aria-describedby="created-range-help" value={currentCreatedDraft.base[key] ?? ''} readOnly />
                    : <input type="datetime-local" step="60" aria-label={label} aria-describedby="created-range-help" value={local} onChange={event => {
                      setCreatedDraft({ ...currentCreatedDraft, [key]: { intent: 'set', local: event.target.value } }); setFilterError(null);
                    }} />}
                </label>
                {local === null && <button type="button" onClick={() => {
                  setCreatedDraft({ ...currentCreatedDraft, [key]: { intent: 'set', local: '' } }); setFilterError(null);
                }}>{name}を指定し直す</button>}
                {endpoint.intent !== 'keep' && <button type="button" onClick={() => {
                  setCreatedDraft({ ...currentCreatedDraft, [key]: { intent: 'keep' } }); setFilterError(null);
                }}>{name}の指定し直しを取消</button>}
                <button type="button" onClick={() => {
                  setCreatedDraft({ ...currentCreatedDraft, [key]: { intent: 'clear' } }); setFilterError(null);
                }}>{name}を解除</button>
              </div>;
            })}
            <button className={styles.secondaryButton} type="button" onClick={() => {
              setCreatedDraft(initialCreatedRangeDraft({})); setFilterError(null);
              updateSearch({ createdFrom: undefined, createdBefore: undefined, cursor: undefined });
            }}>日時の条件を解除</button>
            {search.view === 'published' && <label className={styles.checkLine}>
              <input type="checkbox" checked={draftUnread} onChange={event => setDraftUnread(event.target.checked)} />
              未読のみ
            </label>}
            <button className={styles.secondaryButton} type="submit">絞り込む</button>
            <button className={styles.secondaryButton} type="button" onClick={() => {
              setDraftMetadata({ documentType: '', owningDepartment: '', category: '' }); setFilterError(null);
              updateSearch({ documentType: undefined, owningDepartment: undefined, category: undefined, cursor: undefined });
            }}>属性の絞り込みを解除</button>
            <label className={styles.sortField}>
              <span>並び順</span>
              <select value={search.sort} onChange={(event) => updateSearch({ sort: event.target.value as ListSearch['sort'], cursor: undefined })}>
                <option value="created_at_desc">作成日時が新しい順</option>
                <option value="title_asc">文書名の昇順</option>
                {search.view === 'published' && <option value="published_at_desc">公開日時が新しい順</option>}
              </select>
            </label>
          </form>

          <p id="metadata-filter-help" className={styles.muted}>属性は完全一致で、複数の条件はすべて一致する文書を表示します。空欄は未指定です。空白も値として扱います。</p>
          <p id="created-range-help" className={styles.muted}>文書自体の作成日時で絞り込みます。JST / UTC+09:00・分単位。開始を含み、終了を含みません。</p>
          {(filterError || appliedFilterError) && <p role="alert">{filterError || appliedFilterError}</p>}
          {!appliedFilterError && (!historyList || !listError) && listQuery.isPending && <LoadingState label="文書を読み込み中" />}
          {!appliedFilterError && Boolean(listError) && <ApiFeedback error={listError} {...(!historyList ? { onRetry: () => void listQuery.refetch() } : {})} />}
          {historyList && <button type="button" disabled={listQuery.isFetching} onClick={restartHistoryList}>文書履歴一覧を読み直す</button>}
          {!appliedFilterError && listQuery.data && (!historyList || historyListReady) && items.length === 0 && (
            <div className={styles.emptyState}>
              <h2>文書がありません</h2>
              <p>検索条件やフォルダーを変更してください。</p>
            </div>
          )}
          {!appliedFilterError && listQuery.data && items.length > 0 && (
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
