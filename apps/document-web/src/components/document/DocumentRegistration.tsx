import { useEffect, useRef, useState } from 'react';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type CreateDocumentResult } from '../../application/document-workspace';
import { creationIds, creationRecovery, creationWasRejected, clearCreationReceipt, readCreationReceipt, saveCreationReceipt, type CreationReceipt } from '../../application/document-registration';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import styles from './DocumentRegistration.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

type Destination = { folderId: string; name: string };
export function DocumentRegistration({ folder, canCreate, capabilityKnown, contextKey, onCreated }: {
  folder?: Destination; canCreate: boolean; capabilityKnown: boolean; contextKey: string;
  onCreated: (result: CreateDocumentResult) => void;
}) {
  const [open, setOpen] = useState(false);
  const [destination, setDestination] = useState<Destination>();
  const [title, setTitle] = useState('');
  const [file, setFile] = useState<File | null>(null);
  const [receipt, setReceipt] = useState<CreationReceipt | null>(readCreationReceipt);
  const receiptRef = useRef(receipt);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [error, setError] = useState<unknown>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const mounted = useRef(false);
  const currentContext = useRef({ key: contextKey, generation: 0 });
  if (currentContext.current.key !== contextKey) currentContext.current = { key: contextKey, generation: currentContext.current.generation + 1 };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    setOpen(false);
    if (!receiptRef.current) { setTitle(''); setFile(null); setError(null); }
  }, [contextKey]);
  useEffect(() => {
    if (!busy) return;
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [busy]);

  const tooLarge = file !== null && file.size > 256 * 1024 * 1024;
  const wrongFolder = destination?.folderId !== folder?.folderId;
  const problem = problemFromUnknown(error);
  const errorMessage = problem ? mapApiProblem(problem).message : '通信結果を確認できません。';

  function remember(value: CreationReceipt | null) {
    if (value) saveCreationReceipt(value); else clearCreationReceipt();
    receiptRef.current = value;
    if (mounted.current) setReceipt(value);
  }

  function close() {
    if (busyRef.current) return;
    setOpen(false);
    if (!receiptRef.current) { setTitle(''); setFile(null); setError(null); }
    requestAnimationFrame(() => trigger.current?.focus());
  }

  function show() {
    const stored = readCreationReceipt();
    receiptRef.current = stored;
    setReceipt(stored);
    setDestination(folder);
    setOpen(true);
  }

  function complete(result: unknown, startedGeneration: number) {
    const ids = creationIds(result);
    if (!ids) throw new Error('登録結果の識別子を確認できません。');
    remember({ state: 'created', ids });
    if (!mounted.current || currentContext.current.generation !== startedGeneration) return;
    clearCreationReceipt();
    onCreated(ids);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (busyRef.current || receiptRef.current || !destination || !file || !title.trim() || tooLarge || wrongFolder || !canCreate) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    const startedGeneration = currentContext.current.generation;
    try {
      // Persist before POST so a reload/back action cannot turn an unknown outcome into a new create.
      remember({ state: 'unknown' });
    } catch (failure) {
      setError(failure); busyRef.current = false; setBusy(false); return;
    }
    try {
      const result = await documentApi.createDocument({ folderId: destination.folderId, title: title.trim(), documentMetadata: {}, versionMetadata: {} }, file);
      complete(result, startedGeneration);
    } catch (failure) {
      const ids = creationRecovery(failure);
      const rejected = creationWasRejected(failure);
      try { remember(rejected ? null : { state: 'unknown', ...(ids ? { ids } : {}) }); } catch { /* The pre-POST marker remains fail-closed. */ }
      if (mounted.current) {
        if (rejected && currentContext.current.generation !== startedGeneration) { setTitle(''); setFile(null); setError(null); }
        else setError(failure);
      }
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  async function recover() {
    const ids = receiptRef.current?.ids;
    if (busyRef.current || !ids) return;
    busyRef.current = true; setBusy(true); setError(null);
    const startedGeneration = currentContext.current.generation;
    try {
      const recovered = await documentApi.recoverDocumentCreation(ids);
      if (recovered.documentId !== ids.documentId || recovered.documentVersionId !== ids.documentVersionId || recovered.fileId !== ids.fileId) throw new Error('登録結果が一致しません。');
      complete(recovered, startedGeneration);
    } catch (failure) {
      if (mounted.current) setError(failure);
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return <>
    <button ref={trigger} className={workspace.primaryButton} type="button" hidden={!receipt && !capabilityKnown} disabled={!receipt && (!folder || !canCreate)} onClick={show}>文書を登録</button>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!busy} isKeyboardDismissDisabled={busy} className={styles.modal}>
      <Dialog aria-labelledby="document-registration-title" className={styles.dialog}>
        <Heading slot="title" id="document-registration-title">文書を登録</Heading>
        <p>原本を下書き（WORKING）として登録します。公開は登録後に別の操作で行います。</p>
        {destination && !receipt && <p>登録先：<strong>{destination.name}</strong></p>}
        <form onSubmit={submit} aria-busy={busy}>
          <label className={workspace.formField}>文書名<input autoFocus value={title} disabled={busy || Boolean(receipt)} onChange={event => setTitle(event.target.value)} required /></label>
          <label className={workspace.formField}>原本ファイル<input type="file" disabled={busy || Boolean(receipt)} onChange={event => { setFile(event.target.files?.[0] ?? null); setError(null); }} required /></label>
          {file && <p>{file.name}</p>}
          {tooLarge && <p role="alert">1ファイルあたり256 MiB以下のファイルを選択してください。</p>}
          {busy ? <p role="status">登録結果を確認しています…</p> : receipt?.state === 'unknown' ? <section role="alert">
            <h3>登録結果を確認できません</h3>
            <p>重複を防ぐため、再登録しないでください。画面を閉じても再送せず、編集作業の一覧や管理者に登録結果を確認してください。</p>
            {Boolean(error) && <p>{errorMessage}</p>}
            {problem?.traceId && <small>照会ID: {problem.traceId}</small>}
          </section> : receipt?.state === 'created' ? <p role="status">文書を下書きとして登録しました。</p> : error ? <p role="alert">{errorMessage}</p> : null}
          <div className={styles.actions}>
            <button type="button" disabled={busy} onClick={close}>{receipt && !busy ? '閉じる' : 'キャンセル'}</button>
            {receipt?.state === 'created' && receipt.ids ? <button type="button" onClick={() => { clearCreationReceipt(); onCreated(receipt.ids!); }}>登録した文書を開く</button>
              : receipt?.ids ? <button type="button" disabled={busy} onClick={() => void recover()}>登録結果を確認</button>
                : !receipt && <button className={workspace.primaryButton} type="submit" disabled={busy || !file || !title.trim() || tooLarge || wrongFolder || !canCreate}>下書きとして登録</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
