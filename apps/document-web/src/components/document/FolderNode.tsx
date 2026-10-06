import { useMemo, useState } from 'react';
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { documentApi, type Folder } from '../../application/document-workspace';
import type { SelectedFolderContext } from '../../application/document-root-folder';
import { ApiFeedback } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentWorkspace.module.css';

export function FolderNode({
  folder,
  selectedFolderId,
  onSelect,
  isRoot = false,
  sourceParentId,
  sourcePageLimit,
  rootSelected = !selectedFolderId,
}: {
  folder: Folder;
  selectedFolderId?: string;
  onSelect: (folderId?: string, folder?: Folder, context?: SelectedFolderContext) => void;
  isRoot?: boolean;
  sourceParentId?: string;
  sourcePageLimit?: number;
  rootSelected?: boolean;
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
        <button type="button" className={styles.folderButton} aria-current={(isRoot ? rootSelected : selectedFolderId === folder.folderId) ? 'location' : undefined} onClick={() => onSelect(isRoot ? undefined : folder.folderId, folder, !isRoot && sourceParentId && sourcePageLimit ? { kind: 'selected', folderId: folder.folderId, name: folder.name, sourceParentId, pageLimit: sourcePageLimit } : undefined)}>{folder.name}</button>
      </div>
      {childrenQuery.error && expanded && <ApiFeedback error={childrenQuery.error} onRetry={childrenQuery.isFetchNextPageError ? undefined : () => void childrenQuery.refetch({ cancelRefetch: false })} />}
      {expanded && children.length > 0 && (
        <ul className={styles.folderChildren}>
          {children.map((child) => <FolderNode key={child.folderId} folder={child} selectedFolderId={selectedFolderId} onSelect={onSelect} sourceParentId={folder.folderId} sourcePageLimit={childrenQuery.data?.pages.length} />)}
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
