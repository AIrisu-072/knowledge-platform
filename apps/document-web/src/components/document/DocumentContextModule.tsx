import { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { documentApi, type DocumentDetail, type FileList } from '../../application/document-workspace';
import { useTaskTransient } from '../../application/organization-context';
import { validateDetailSearch } from '../../application/search-state';
import type { TaskDetail, WorkSession } from '../../application/work-workspace';
import styles from './DocumentContextModule.module.css';

// Only metadata from one complete published read enters the task-scoped cache.
// Provider failures never retain a previous successful title, revision or file list.
type PublishedRead = { state: 'available'; document: DocumentDetail; files: FileList['items'] }
  | { state: 'unpublished' | 'unavailable' };

export function DocumentContextModule({ session, task }: { session: WorkSession; task: TaskDetail }) {
  const [transient, setTransient] = useTaskTransient(`${session.principalId}:${session.actingAssignmentId}:${task.id}:${task.attemptId}`);
  const selectedId = transient.selectedDocumentId ?? task.inputResources[0]?.documentId ?? '';
  useEffect(() => {
    if (transient.selectedDocumentId === undefined) setTransient((previous) => ({ ...previous, selectedDocumentId: selectedId }));
  }, [transient.selectedDocumentId, selectedId, setTransient]);
  const selected = task.inputResources.find((input) => input.documentId === selectedId);
  const scope = ['organization', session.principalId, session.actingAssignmentId ?? 'none', 'document-context', task.id, task.attemptId];
  return <section className={styles.module} aria-label="共有の入力文書">
    <h2>共有の入力文書</h2><p>文書側の現在の権限で確認します。作業文案とは別の共有資料です。</p>
    {task.inputResources.length === 0 ? <p>入力文書はありません</p> : <>
      <label>確認する入力文書<select value={selected?.documentId ?? ''} onChange={(event) => { const selectedDocumentId = event.target.value; setTransient((previous) => ({ ...previous, selectedDocumentId })); }}><option value="" disabled>入力文書を選択</option>{task.inputResources.map((input) => <option key={input.documentId} value={input.documentId}>{input.label}</option>)}</select></label>
      {selected && <PublishedDocument key={JSON.stringify([...scope, selected.documentId])} queryKey={[...scope, selected.documentId]} documentId={selected.documentId} />}
      <h3>文書の詳細・比較</h3><ul>{task.inputResources.map((input) => <li key={input.documentId}><Link to="/documents/$documentId" params={{ documentId: input.documentId }} search={validateDetailSearch({ view: 'published' })}>{input.label}</Link></li>)}</ul>
      <p>改訂の履歴・比較は既存の文書画面で確認できます。戻ると選択したタスクに戻ります。</p>
    </>}
  </section>;
}

function PublishedDocument({ queryKey, documentId }: { queryKey: string[]; documentId: string }) {
  const client = useQueryClient();
  const fence = useRef(0);
  const content = useRef<HTMLElement>(null);
  const reloadButton = useRef<HTMLButtonElement>(null);
  const mounted = useRef(false);
  const downloading = useRef<number | null>(null);
  const [pending, setPending] = useState<{ key: string; fence: number } | null>(null);
  const [downloadError, setDownloadError] = useState(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; fence.current += 1; }; }, []);
  const source = useQuery({
    queryKey,
    queryFn: async ({ signal }): Promise<PublishedRead> => {
      // Fence downloads even when a fast focus/retry refetch never renders loading.
      fence.current += 1;
      try {
        const document = await documentApi.getDocument(documentId, 'published');
        signal.throwIfAborted();
        if (!document.displayRevision) return { state: 'unpublished' };
        if (document.documentId !== documentId || document.displayRevision.documentVersionId !== document.documentVersionId || document.currentVersionId !== document.documentVersionId) return { state: 'unavailable' };
        const files = await documentApi.listVersionFiles(documentId, document.displayRevision.documentVersionId, 'published');
        signal.throwIfAborted();
        return { state: 'available', document, files: files.items.filter((file) => file.role === 'AUTHORITATIVE') };
      } catch {
        signal.throwIfAborted();
        return { state: 'unavailable' };
      }
    },
    staleTime: 0, gcTime: 0, retry: false,
  });
  const data = source.isSuccess && !source.isFetching && source.data.state === 'available' ? source.data : undefined;
  async function download(file: FileList['items'][number]) {
    if (!data || downloading.current === fence.current) return;
    const requestFence = fence.current;
    downloading.current = requestFence;
    setPending({ key: `${file.contentItemId}:${file.representationId}`, fence: requestFence });
    setDownloadError(false);
    const current = () => mounted.current && fence.current === requestFence;
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId: data.document.displayRevision!.documentVersionId, contentItemId: file.contentItemId, representationId: file.representationId, purpose: 'published' });
      if (!current()) return;
      const url = URL.createObjectURL(blob);
      try {
        const anchor = window.document.createElement('a');
        anchor.href = url;
        anchor.download = safeFilename(file.displayName);
        anchor.click();
      } finally { window.setTimeout(() => URL.revokeObjectURL(url), 1000); }
    } catch (error) {
      if (!current()) return;
      if (error && typeof error === 'object' && 'status' in error && [401, 403, 404].includes(Number(error.status))) {
        fence.current += 1;
        if (content.current?.contains(window.document.activeElement)) reloadButton.current?.focus();
        client.setQueryData<PublishedRead>(queryKey, { state: 'unavailable' });
      } else setDownloadError(true);
    } finally {
      if (mounted.current && downloading.current === requestFence) { downloading.current = null; setPending(null); }
    }
  }
  function reload() {
    fence.current += 1;
    setDownloadError(false);
    void source.refetch();
  }
  const pendingKey = pending?.fence === fence.current ? pending.key : null;
  return <>
    <button ref={reloadButton} type="button" disabled={source.isFetching} onClick={reload}>公開文書を再読込</button>
    {source.isPending || source.isFetching ? <p role="status">公開文書を確認中…</p> : data ? <section ref={content} aria-label="公開文書の内容">
      <h3>{data.document.title}</h3>
      <p>公開改訂 {data.document.displayRevision!.label}<br />{data.document.displayRevision!.revisionId}</p>
      <p>内容の版（Version） {data.document.displayRevision!.documentVersionId}</p>
      <h4>原本ファイル</h4><p>明示操作で取得します。原本の内容はこの画面内では実行・表示しません。</p>
      {data.files.length === 0 ? <p>原本ファイルはありません</p> : <ul>{data.files.map((file) => <li key={`${file.contentItemId}:${file.representationId}`}>
        <span>{safeFilename(file.displayName)}</span><br /><small>{file.mediaType} · {file.sizeBytes} bytes</small><br />
        <button type="button" aria-label={`原本を取得 ${safeFilename(file.displayName)}`} aria-busy={pendingKey === `${file.contentItemId}:${file.representationId}`} disabled={pendingKey !== null} onClick={() => void download(file)}>{pendingKey === `${file.contentItemId}:${file.representationId}` ? '取得中…' : '原本を取得'}</button>
      </li>)}</ul>}
      {downloadError && <p role="alert">原本を取得できません。再度取得してください。</p>}
    </section> : source.data?.state === 'unpublished' ? <p>公開改訂はありません</p> : <p role="alert">公開文書を利用できません。現在の権限または接続状態を確認して再読込してください。</p>}
  </>;
}

function safeFilename(value: string) {
  return (value.split(/[\\/]/).pop() ?? '').replace(/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/g, '').trim().slice(0, 200) || 'original';
}
