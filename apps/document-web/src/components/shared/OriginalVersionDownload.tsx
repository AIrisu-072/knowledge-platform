import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { documentApi } from '../../application/document-workspace';
import { ApiFeedback } from './ApiFeedback';
import styles from './OriginalVersionDownload.module.css';

export function OriginalVersionDownload({
  documentId,
  versionId,
  purpose,
  label,
  variant = 'secondary',
}: {
  documentId: string;
  versionId: string;
  purpose: 'published' | 'authoring';
  label: string;
  variant?: 'primary' | 'secondary';
}) {
  const client = useQueryClient();
  const queryKey = ['document-version-files', documentId, versionId, purpose] as const;
  const identity = JSON.stringify(queryKey);
  const liveIdentity = useRef(identity); liveIdentity.current = identity;
  const active = useRef<AbortController | undefined>(undefined);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; active.current?.abort(); }; }, []);
  useEffect(() => () => { active.current?.abort(); }, [identity]);
  const filesQuery = useQuery({
    queryKey,
    queryFn: () => documentApi.listVersionFiles(documentId, versionId, purpose),
  });
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const state = useSyncExternalStore(listener => client.getQueryCache().subscribe(event => { if ((event.type === 'updated' || event.type === 'removed') && JSON.stringify(event.query.queryKey) === identity) listener(); }), () => client.getQueryState(queryKey));
  const readable = state?.status === 'success' && state.fetchStatus === 'idle' && !state.isInvalidated && state.data === filesQuery.data;
  const file = readable ? filesQuery.data?.items[0] : undefined;
  useEffect(() => { if (!readable) active.current?.abort(); }, [readable]);

  async function download() {
    if (!file || active.current && !active.current.signal.aborted) return;
    const controller = new AbortController(); active.current = controller;
    const snapshot = filesQuery.data;
    const current = () => { const read = client.getQueryState(queryKey); return mounted.current && liveIdentity.current === identity && active.current === controller && !controller.signal.aborted && read?.status === 'success' && read.fetchStatus === 'idle' && !read.isInvalidated && read.data === snapshot; };
    setPending(true);
    setError(null);
    try {
      const blob = await documentApi.downloadVersionFile({
        documentId,
        versionId,
        contentItemId: file.contentItemId,
        representationId: file.representationId,
        purpose,
      }, { signal: controller.signal });
      if (!current()) { controller.abort(); return; }
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = file.displayName;
      anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (caught) {
      if (current()) setError(caught);
    } finally {
      if (active.current === controller) { active.current = undefined; if (mounted.current) setPending(false); }
    }
  }

  if (filesQuery.isPending) return <span role="status">原本情報を取得中…</span>;
  if (filesQuery.error) return <ApiFeedback error={filesQuery.error} onRetry={() => void filesQuery.refetch()} />;
  if (!readable) return <span role="status">原本情報の再確認が必要です</span>;
  if (!file) return <span>原本ファイルはありません</span>;

  return (
    <span className={styles.downloadAction}>
      <button
        className={variant === 'primary' ? styles.primary : styles.secondary}
        type="button"
        disabled={pending || !readable}
        aria-busy={pending}
        onClick={() => void download()}
      >
        {pending ? '取得中…' : label}
      </button>
      {Boolean(error) && <ApiFeedback error={error} />}
    </span>
  );
}
