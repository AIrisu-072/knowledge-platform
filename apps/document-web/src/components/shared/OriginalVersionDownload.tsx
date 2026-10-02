import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
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
  const filesQuery = useQuery({
    queryKey: ['document-version-files', documentId, versionId, purpose],
    queryFn: () => documentApi.listVersionFiles(documentId, versionId, purpose),
  });
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const file = filesQuery.data?.items[0];

  async function download() {
    if (!file) return;
    setPending(true);
    setError(null);
    try {
      const blob = await documentApi.downloadVersionFile({
        documentId,
        versionId,
        contentItemId: file.contentItemId,
        representationId: file.representationId,
        purpose,
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = file.displayName;
      anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (caught) {
      setError(caught);
    } finally {
      setPending(false);
    }
  }

  if (filesQuery.isPending) return <span role="status">原本情報を取得中…</span>;
  if (filesQuery.error) return <ApiFeedback error={filesQuery.error} onRetry={() => void filesQuery.refetch()} />;
  if (!file) return <span>原本ファイルはありません</span>;

  return (
    <span className={styles.downloadAction}>
      <button
        className={variant === 'primary' ? styles.primary : styles.secondary}
        type="button"
        disabled={pending}
        aria-busy={pending}
        onClick={() => void download()}
      >
        {pending ? '取得中…' : label}
      </button>
      {Boolean(error) && <ApiFeedback error={error} />}
    </span>
  );
}
