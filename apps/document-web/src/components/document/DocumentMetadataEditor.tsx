import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type DocumentDetail } from '../../application/document-workspace';
import { metadataDraft, metadataFields, metadataOperations, metadataPatch, metadataReason, metadataValidation, sendMetadataOperation, type MetadataDraft } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

export function DocumentMetadataEditor({ document, reload }: { document: DocumentDetail; reload: () => Promise<unknown> }) {
  const client = useQueryClient();
  const store = metadataOperations(client);
  const operation = useSyncExternalStore(store.subscribe, () => store.get(document.documentId));
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<MetadataDraft>(() => metadataDraft(document.metadata ?? {}));
  const [revision, setRevision] = useState(document.revision);
  const [reason, setReason] = useState('');
  const [localError, setLocalError] = useState('');
  const trigger = useRef<HTMLButtonElement>(null);
  const opening = useRef(0);
  useEffect(() => () => { opening.current += 1; }, []);
  const capability = document.capabilities.updateMetadata;
  const allowed = capability?.status === 'available';
  const pending = operation?.status === 'pending';
  const unknown = operation?.status === 'unknown';
  const locked = Boolean(operation);
  const validation = metadataValidation(draft, reason);
  const problem = problemFromUnknown(operation?.error);

  function show() {
    opening.current += 1;
    const fields = metadataDraft(document.metadata ?? {});
    if (operation) for (const { key } of metadataFields) {
      if (typeof operation.request.set[key] === 'string') fields[key].value = operation.request.set[key] as string;
      fields[key].remove = operation.request.unset.includes(key);
    }
    setDraft(fields); setRevision(document.revision); setReason(operation?.request.reason ?? ''); setLocalError(''); setOpen(true);
  }
  function close() {
    if (store.get(document.documentId)?.status === 'pending') return;
    const generation = ++opening.current;
    const target = trigger.current;
    if (operation?.status === 'succeeded' || operation?.status === 'rejected') store.clear(document.documentId);
    setOpen(false);
    requestAnimationFrame(() => {
      if (opening.current === generation && target?.isConnected && !target.disabled
        && target.ownerDocument.activeElement === target.ownerDocument.body) target.focus();
    });
  }
  async function reloadCurrentOpening() {
    const generation = opening.current;
    try {
      await reload();
      if (opening.current === generation) close();
    } catch {
      if (opening.current === generation) setLocalError('最新の内容を取得できません。もう一度読み直してください。');
    }
  }
  async function invalidate() {
    await Promise.all([
      ...['document', 'document-versions', 'document-revisions', 'document-history', 'revision-comparison', 'document-version', 'document-version-files'].map(key => client.invalidateQueries({ queryKey: [key, document.documentId] })),
      client.invalidateQueries({ queryKey: ['documents'] }),
    ]);
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault();
    const saved = store.get(document.documentId);
    if (!allowed || (saved && saved.status !== 'unknown') || (!saved && validation)) return;
    try {
      await sendMetadataOperation({ store, documentId: document.documentId,
        request: saved?.request ?? { operationId: createOperationId(), expectedDocumentRevision: revision, ...metadataPatch(draft), reason: metadataReason(reason) },
        send: documentApi.patchDocumentMetadata, invalidate });
    } catch { setLocalError('操作を開始できませんでした。画面を閉じて再度お試しください。'); }
  }
  function change(key: keyof MetadataDraft, update: Partial<MetadataDraft[keyof MetadataDraft]>) {
    if (locked) return;
    setDraft(previous => ({ ...previous, [key]: { ...previous[key], ...update } }));
  }
  return <>
    {(capability || operation) && <button ref={trigger} type="button" disabled={!allowed && !operation} onClick={show}>メタデータを編集</button>}
    {capability?.status === 'disabled' && <p className={styles.hint}>{capability.reason === 'pendingSchedule' ? '予約公開中はメタデータを編集できません。' : '現在の権限または文書状態ではメタデータを編集できません。'}</p>}
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="metadata-editor-title" className={styles.dialog}>
        <Heading slot="title" id="metadata-editor-title">メタデータを編集</Heading>
        <p>{document.title}</p>
        <p>空文字・空白も値として保存します。値を消す場合は「削除」にチェックしてください。その他の属性は保持します。</p>
        <form onSubmit={submit} aria-busy={pending}>
          {metadataFields.map(({ key, label }, index) => <div key={key} className={styles.field}>
            <label className={workspace.formField}>{label}<textarea aria-label={label} autoFocus={index === 0} rows={2} value={draft[key].value} disabled={locked || draft[key].remove || !allowed} onChange={event => change(key, { value: event.target.value, touched: true })} /></label>
            {draft[key].present && typeof draft[key].original !== 'string' && <p className={styles.currentValue}>現在の値（文字列以外・未編集なら保持）：<span>{JSON.stringify(draft[key].original)}</span></p>}
            {!draft[key].present && <small>現在は未設定です。</small>}
            <label className={styles.remove}><input type="checkbox" checked={draft[key].remove} disabled={locked || !allowed} onChange={event => change(key, { remove: event.target.checked })} />{label}を削除</label>
          </div>)}
          <label className={workspace.formField}>変更理由<textarea aria-label="変更理由" rows={2} value={reason} disabled={locked || !allowed} onChange={event => setReason(event.target.value)} /></label>
          {!locked && reason && validation && <p role="alert">{validation}</p>}
          {!allowed && <p role="alert">現在の権限または文書状態では保存できません。</p>}
          {localError && <p role="alert">{localError}</p>}
          {pending && <p role="status">保存結果を確認しています…</p>}
          {unknown && <section role="alert"><h3>保存結果を確認できません</h3><p>同じ操作ID・同じ内容で再送して結果を確認してください。値が一致するだけでは、この操作の成功は確認できません。</p><p>この操作は開いているアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了後は同じ操作を再送できなくなるため、先に結果を確認してください。</p></section>}
          {Boolean(operation?.error) && problem && <p role={unknown ? undefined : 'alert'}>{mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <p role="status">{operation.result?.changed ? 'メタデータを更新しました。' : '変更はありませんでした。'}</p>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation && !pending ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" disabled={!allowed} onClick={() => void submit()}>同じ内容で再送</button>
              : operation?.status === 'rejected' ? <button type="button" onClick={() => void reloadCurrentOpening()}>最新の内容を読み直す</button>
                : !operation || pending ? <button className={workspace.primaryButton} type="submit" disabled={pending || !allowed || Boolean(validation)}>保存する</button> : null}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
