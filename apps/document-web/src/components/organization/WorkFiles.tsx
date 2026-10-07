import { useEffect, useRef, useState } from 'react';
import { workErrorMessage, type WorkFile } from '../../application/work-workspace';
import styles from '../../routes/TaskWorkspace.module.css';

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}

/** Fetches through the authorized Work API and saves it as an attachment; the
 * content is never rendered in the page. */
export function WorkFileDownload({ file, label, read }: { file: WorkFile; label: string; read: (signal: AbortSignal) => Promise<Blob> }) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const active = useRef<AbortController | undefined>(undefined);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; active.current?.abort(); }; }, []);
  async function run() {
    if (active.current && !active.current.signal.aborted) return;
    const controller = new AbortController(); active.current = controller;
    setPending(true); setError(null);
    try {
      const blob = await read(controller.signal);
      if (!mounted.current || controller.signal.aborted) return;
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = file.fileName;
      anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (caught) {
      if (mounted.current && !controller.signal.aborted) setError(caught);
    } finally {
      if (active.current === controller) { active.current = undefined; if (mounted.current) setPending(false); }
    }
  }
  return <span className={styles.actions}>
    <button type="button" aria-label={label} disabled={pending} aria-busy={pending} onClick={() => void run()}>{pending ? '取得中…' : 'ファイルを取得'}</button>
    {Boolean(error) && <span role="alert">{workErrorMessage(error)}</span>}
  </span>;
}

/** Client-side precheck only; the server enforces the same rules. */
export function workFileProblem(file: File, maxBytes: number): string | null {
  if (file.size === 0) return '空のファイルは添付できません。';
  if (file.size > maxBytes) return 'ファイルは8MiB以内にしてください。';
  if (!file.name || file.name === '.' || file.name === '..' || /[\\/\p{Cc}\u061C\u200B-\u200F\u202A-\u202E\u2066-\u2069\uFEFF]/u.test(file.name) || new TextEncoder().encode(file.name).length > 255) return 'ファイル名に使えない文字が含まれているか、長すぎます（UTF-8で255バイト以内）。';
  return null;
}
/** The declared type is metadata only; anything unusual becomes opaque bytes. */
export function declaredMediaType(file: File): string {
  return /^[a-zA-Z0-9!#$&^_.+-]+\/[a-zA-Z0-9!#$&^_.+-]+$/.test(file.type) && file.type.length <= 127 ? file.type : 'application/octet-stream';
}
