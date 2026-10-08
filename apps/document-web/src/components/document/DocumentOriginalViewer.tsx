import { useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { documentApi, type FileList } from '../../application/document-workspace';
import { decodeViewerText, viewerElementVisible, viewerFileProblem, VIEWER_MAX_BYTES } from '../../application/document-original-viewer';
import { openPdfViewer, type PdfViewerSession } from '../../application/pdf-renderer';
import styles from './DocumentOriginalViewer.module.css';

// A QueryClient owns at most one decoded original and one PDF worker.
const viewerOwners = new WeakMap<object, { token: object; close: () => void }>();
type FileItem = FileList['items'][number];
export function DocumentOriginalViewer({ documentId, versionId, purpose, file }: { documentId: string; versionId: string; purpose: 'published' | 'authoring'; file: FileItem }) {
  const displayButton = useRef<HTMLButtonElement>(null); const restoreFocus = useRef(false);
  const client = useQueryClient(); const host = useRef<HTMLDivElement>(null); const canvas = useRef<HTMLCanvasElement>(null);
  const downloading = useRef<AbortController | undefined>(undefined); const [downloadPending, setDownloadPending] = useState(false); const [downloadError, setDownloadError] = useState('');
  const controller = useRef<AbortController | undefined>(undefined); const pdf = useRef<PdfViewerSession | undefined>(undefined);
  const ownerToken = useRef({}); const pageBusy = useRef(false); const epoch = useRef(0); const mounted = useRef(true);
  const [open, setOpen] = useState(false); const [text, setText] = useState<string>(); const [error, setError] = useState('');
  const [pending, setPending] = useState(false); const [page, setPage] = useState(1); const [pages, setPages] = useState(0);
  const identity = JSON.stringify([documentId, versionId, purpose, file.contentItemId, file.representationId]);
  const currentIdentity = useRef(identity); currentIdentity.current = identity;
  const docKey = ['document', documentId, purpose] as const; const filesKey = ['document-version-files', documentId, versionId, purpose] as const;
  const readable = () => {
    const doc = client.getQueryState<{ currentVersionId?: string; displayVersion: { versionId: string } }>(docKey); const files = client.getQueryState<FileList>(filesKey);
    return [doc, files].every(read => read?.status === 'success' && read.fetchStatus === 'idle' && !read.isInvalidated)
      && (purpose === 'published' ? doc?.data?.currentVersionId ?? doc?.data?.displayVersion.versionId : doc?.data?.displayVersion.versionId) === versionId && files?.data?.items.some(row => row.contentItemId === file.contentItemId && row.representationId === file.representationId && row.mediaType === file.mediaType && row.sizeBytes === file.sizeBytes);
  };
  const dispose = () => { if (viewerOwners.get(client)?.token === ownerToken.current) viewerOwners.delete(client); epoch.current += 1; downloading.current?.abort(); downloading.current = undefined; setDownloadPending(false); setDownloadError(''); controller.current?.abort(); controller.current = undefined; pdf.current?.destroy(); pdf.current = undefined;
    if (canvas.current) { canvas.current.width = 0; canvas.current.height = 0; } pageBusy.current = false; setText(undefined); setPages(0); setPending(false); setOpen(false); };
  useEffect(() => { mounted.current = true; const stop = client.getQueryCache().subscribe(() => { if (!readable()) dispose(); });
    const visibility = () => { if (!viewerElementVisible(host.current)) dispose(); };
    document.addEventListener('visibilitychange', visibility);
    // CSS/hidden changes can invalidate a display without a route or query change.
    const observer = new MutationObserver(visibility); observer.observe(document.documentElement, { attributes: true, subtree: true, attributeFilter: ['style', 'class', 'hidden'] });
    return () => { if (viewerOwners.get(client)?.token === ownerToken.current) viewerOwners.delete(client); mounted.current = false; epoch.current += 1; downloading.current?.abort(); controller.current?.abort(); pdf.current?.destroy(); if (canvas.current) { canvas.current.width = 0; canvas.current.height = 0; } stop(); observer.disconnect(); document.removeEventListener('visibilitychange', visibility); };
  }, [identity, client]);
  useEffect(() => { if (!open && restoreFocus.current) { restoreFocus.current = false; displayButton.current?.focus(); } }, [open]);
  const problem = viewerFileProblem(file);
  async function display() {
    if (controller.current || problem || !readable() || !viewerElementVisible(host.current)) return;
    viewerOwners.get(client)?.close(); viewerOwners.set(client, { token: ownerToken.current, close: dispose });
    const active = new AbortController(); controller.current = active; const token = ++epoch.current;
    const docSnapshot = client.getQueryData(docKey); const fileSnapshot = client.getQueryData(filesKey);
    const current = () => mounted.current && currentIdentity.current === identity && epoch.current === token && !active.signal.aborted && readable() && viewerElementVisible(host.current)
      && client.getQueryData(docKey) === docSnapshot && client.getQueryData(filesKey) === fileSnapshot;
    setOpen(true); setPending(true); setError(''); setText(undefined); setPage(1);
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, purpose, contentItemId: file.contentItemId, representationId: file.representationId }, { signal: active.signal, maxBytes: VIEWER_MAX_BYTES });
      if (!current()) { if (epoch.current === token) dispose(); return; }
      if (blob.size > VIEWER_MAX_BYTES) throw new Error('表示は10 MiBまでです。原本をダウンロードしてください。');
      const type = file.mediaType.split(';')[0]!.trim().toLowerCase();
      if (blob.type.split(';')[0] !== type) throw new Error('原本の形式を確認できません。ダウンロードして確認してください。');
      const bytes = new Uint8Array(await blob.arrayBuffer()); if (!current()) { if (epoch.current === token) dispose(); return; }
      if (type === 'text/plain') setText(decodeViewerText(bytes));
      else {
        const session = await openPdfViewer(bytes, active.signal); if (!current()) { session.destroy(); return; }
        pdf.current = session; setPages(session.pages);
        if (!canvas.current) throw new Error('PDFの表示領域を確認できません。');
        await session.render(1, canvas.current); if (!current()) { if (epoch.current === token) dispose(); return; }
      }
    } catch (caught) { if (current()) { pdf.current?.destroy(); pdf.current = undefined; setError(caught instanceof Error ? caught.message : '原本を表示できません。ダウンロードして確認してください。'); } }
    finally { if (current()) setPending(false); }
  }
  async function download() {
    if (downloading.current || !readable() || !viewerElementVisible(host.current)) return;
    const active = new AbortController(); downloading.current = active; const docSnapshot = client.getQueryData(docKey); const fileSnapshot = client.getQueryData(filesKey);
    const current = () => mounted.current && currentIdentity.current === identity && downloading.current === active && !active.signal.aborted && readable() && viewerElementVisible(host.current)
      && client.getQueryData(docKey) === docSnapshot && client.getQueryData(filesKey) === fileSnapshot;
    setDownloadPending(true); setDownloadError('');
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, purpose, contentItemId: file.contentItemId, representationId: file.representationId }, { signal: active.signal });
      if (!current()) return;
      const url = URL.createObjectURL(blob); const link = document.createElement('a'); link.href = url; link.download = file.displayName;
      try { link.click(); } finally { window.setTimeout(() => URL.revokeObjectURL(url), 1000); }
    } catch (caught) { if (current()) setDownloadError(caught instanceof Error ? caught.message : '原本を取得できません。'); }
    finally { if (downloading.current === active) { downloading.current = undefined; if (mounted.current) setDownloadPending(false); } }
  }
  async function changePage(next: number) {
    if (pageBusy.current || pending || !pdf.current || !canvas.current || !readable() || !viewerElementVisible(host.current)) return;
    pageBusy.current = true; const token = epoch.current; setPending(true); setError('');
    try { await pdf.current.render(next, canvas.current); if (token === epoch.current && readable()) setPage(next); }
    catch (caught) { if (token === epoch.current) { pdf.current?.destroy(); pdf.current = undefined; setError(caught instanceof Error ? caught.message : 'PDFを表示できません。'); } }
    finally { if (token === epoch.current) { pageBusy.current = false; setPending(false); } }
  }
  return <div ref={host}>
    {problem ? <p>{problem}</p> : <button ref={displayButton} type="button" disabled={open} onClick={() => void display()}>{file.displayName}を表示</button>}
    <button type="button" disabled={downloadPending} onClick={() => void download()}>{downloadPending ? '取得中…' : `${file.displayName}をダウンロード`}</button>
    {downloadError && <p role="alert">{downloadError}</p>}
    {open && <section aria-label={`${file.displayName}の原本表示`} className={styles.viewer}>
      <div className={styles.controls}><strong>{file.displayName}</strong><button type="button" onClick={() => { restoreFocus.current = true; dispose(); }}>原本表示を閉じる</button></div>
      <p>原本の表示は読了や同意の証明ではありません。</p>
      {pending && <p role="status">原本を表示中…</p>}{error && <p role="alert">{error}</p>}
      {text !== undefined && <pre className={styles.text} data-testid="original-viewer-text">{text}</pre>}
      <canvas ref={canvas} className={styles.canvas} hidden={!pages || Boolean(error)} aria-label={`PDF ${page}ページ目`} />
      {pages > 0 && !error && <p>大きい画像や対応できないPDF表現は描画されない場合があります。表示が欠ける場合は原本をダウンロードして確認してください。</p>}
      {pages > 0 && !error && <div className={styles.controls}><button type="button" disabled={pending || page <= 1} onClick={() => void changePage(page - 1)}>前のページ</button><span>{page} / {pages}ページ</span><button type="button" disabled={pending || page >= pages} onClick={() => void changePage(page + 1)}>次のページ</button></div>}
    </section>}
  </div>;
}
