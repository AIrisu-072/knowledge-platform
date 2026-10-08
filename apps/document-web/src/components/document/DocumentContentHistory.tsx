import { Fragment, useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { documentApi, type FileList } from '../../application/document-workspace';
import { useContentHistoryVersion, useDocumentContentHistory } from '../../application/use-document-content-history';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import { versionStatusLabel } from '../../view-model/document-status';
import { formatDateTime } from '../../view-model/date-time';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentContentHistory({ documentId, isDocumentReadable }: { documentId: string; isDocumentReadable?: () => boolean }) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  if (!open) return <button type="button" ref={trigger} onClick={() => setOpen(true)}>コンテンツ版の履歴を開く</button>;
  return <ContentHistoryRead documentId={documentId} isDocumentReadable={isDocumentReadable} onClose={() => {
    setOpen(false);
    window.requestAnimationFrame(() => {
      if (trigger.current?.isConnected && document.activeElement === document.body) trigger.current.focus();
    });
  }} />;
}

function ContentHistoryRead({ documentId, onClose, isDocumentReadable }: { documentId: string; onClose: () => void; isDocumentReadable?: () => boolean }) {
  const read = useDocumentContentHistory(documentId, isDocumentReadable);
  const [versionId, setVersionId] = useState('');
  const selected = read.versions.find(version => version.versionId === versionId);
  return <section className={styles.contentSection} aria-label="コンテンツ版の履歴（閲覧専用）">
    <h2>コンテンツ版の履歴（閲覧専用）</h2>
    <p>選択した版の内容と原本を確認します。公開・編集対象は変わりません。</p>
    <p className={styles.muted}>取得中の変更により、一覧に抜けや重複が生じる場合があります。最新の履歴は最初から読み直してください。</p>
    {read.initialLoading && <LoadingState label="コンテンツ版の履歴を取得中" />}
    {read.busy && !read.initialLoading && <LoadingState label={read.adding ? 'コンテンツ版の履歴の続きを取得中' : 'コンテンツ版の履歴を読み直し中'} />}
    {Boolean(read.error) && <ApiFeedback error={read.error} />}
    <div className={styles.actionRow}>
      {read.canContinue && <button type="button" disabled={read.busy} onClick={read.loadMore}>{read.continuationError ? 'コンテンツ版の履歴の続きを再試行' : 'コンテンツ版の履歴をさらに表示'}</button>}
      <button type="button" disabled={read.busy} onClick={read.restart}>コンテンツ版の履歴を最初から読み直す</button>
      <button type="button" onClick={onClose}>コンテンツ版の履歴を閉じる</button>
    </div>
    {read.ready && <>
      <label>履歴のコンテンツ版を選択
        <select value={versionId} onChange={event => setVersionId(event.target.value)}>
          <option value="">コンテンツ版を選択してください</option>
          {versionId && !selected && <option value={versionId}>未取得の選択</option>}
          {read.versions.map(version => <option key={version.versionId} value={version.versionId}>Version {version.versionNo} · {versionStatusLabel(version)}</option>)}
        </select>
      </label>
      {read.versions.length === 0 && !read.canContinue && <p>表示できるコンテンツ版の履歴はありません。</p>}
      {versionId && !selected && <p>選択したコンテンツ版は未取得です。続きを取得するか、明示的に選び直してください。</p>}
      {selected && <SelectedContentVersion key={versionId} documentId={documentId} versionId={versionId} isDocumentReadable={isDocumentReadable} />}
    </>}
  </section>;
}

function SelectedContentVersion({ documentId, versionId, isDocumentReadable }: { documentId: string; versionId: string; isDocumentReadable?: () => boolean }) {
  const read = useContentHistoryVersion(documentId, versionId);
  const client = useQueryClient();
  const pending = useRef<AbortController | null>(null);
  const [downloading, setDownloading] = useState(false);
  const liveRead = useRef(read); liveRead.current = read;
  const documentReadable = useRef(isDocumentReadable); documentReadable.current = isDocumentReadable;
  useEffect(() => {
    const unsubscribe = client.getQueryCache().subscribe(() => {
      if (pending.current && (!liveRead.current.downloadTarget() || documentReadable.current?.() === false)) { pending.current.abort(); pending.current = null; setDownloading(false); }
    });
    return () => { unsubscribe(); pending.current?.abort(); pending.current = null; };
  }, [client]);
  async function download(file: FileList['items'][number]) {
    const target = read.downloadTarget();
    const documentTarget = documentReadable.current;
    if (pending.current || documentReadable.current?.() === false || !target || target.version?.capabilities.download?.status !== 'available' || file.role !== 'AUTHORITATIVE' || !target.files?.items.includes(file)) return;
    const controller = new AbortController(); pending.current = controller; setDownloading(true);
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, contentItemId: file.contentItemId,
        representationId: file.representationId, purpose: 'history' }, { signal: controller.signal });
      const current = liveRead.current.downloadTarget();
      if (controller.signal.aborted || documentTarget?.() === false || documentReadable.current?.() === false || !current || current.version !== target.version || current.files !== target.files) return;
      const url = URL.createObjectURL(blob); const anchor = document.createElement('a');
      anchor.href = url; anchor.download = file.displayName; anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (error) { if (!controller.signal.aborted) read.stop(error); }
    finally { if (pending.current === controller) { pending.current = null; setDownloading(false); } }
  }
  const version = read.detail;
  const originals = read.files?.items.filter(file => file.role === 'AUTHORITATIVE');
  return <section aria-label="選択したコンテンツ版の詳細">
    {read.busy && <LoadingState label="選択したコンテンツ版を取得中" />}
    {version && <>
      <h3>Version {version.versionNo}</h3><p>{version.title}</p>
      <dl className={styles.metadataGrid}>
        <dt>状態</dt><dd>{versionStatusLabel(version)}</dd>
        <dt>作成日時</dt><dd>{formatDateTime(version.createdAt)}</dd>
        <dt>更新日時</dt><dd>{formatDateTime(version.updatedAt)}</dd>
        {([['承認日時', version.approvedAt], ['予約公開日時', version.scheduledPublishAt], ['公開日時', version.publishedAt], ['取下げ日時', version.withdrawnAt], ['初回記録日時', version.firstReadAt]] as const).map(([label, value]) => <Fragment key={label}><dt>{label}</dt><dd>{value ? formatDateTime(value) : '未記録'}</dd></Fragment>)}
      </dl>
      <h4>コンテンツ版の属性</h4>
      <dl className={styles.metadataGrid}>{Object.entries(version.metadata).map(([key, value]) => <Fragment key={key}><dt>{key}</dt><dd>{typeof value === 'string' ? value : JSON.stringify(value)}</dd></Fragment>)}</dl>
      {originals && <>
        <h4>原本ファイル</h4>
        {originals.length === 0 && <p>原本ファイルはありません</p>}
        <ul>{originals.map(file => <li key={`${file.contentItemId}:${file.representationId}`}>
          <span>{file.displayName} · {file.mediaType} · {file.sizeBytes} bytes</span>
          {version.capabilities.download?.status === 'available' && <button type="button" disabled={downloading} aria-busy={downloading} onClick={() => void download(file)}>履歴の原本を取得: {file.displayName}</button>}
        </li>)}</ul>
      </>}
    </>}
  </section>;
}
