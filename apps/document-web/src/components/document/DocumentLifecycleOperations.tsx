import { useEffect, useRef, useState } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import type { DocumentDetail, VersionDetail } from '../../application/document-workspace';
import { createOperationId } from '../../application/operation-id';
import { lifecycleKey, refreshLifecycleQueries, runLifecycleOperation, type LifecycleIntent, type LifecycleOperation } from '../../application/document-lifecycle-operations';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import styles from '../../routes/DocumentDetail.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

type Draft = { kind: 'withdraw' | 'end'; documentId: string; title: string; versionId: string; versionNo: number; revision: number };
export function DocumentLifecycleOperations({ documentId, document, version, contextKey, showActions }: {
  documentId: string; document?: DocumentDetail; version?: VersionDetail; contextKey: string; showActions: boolean;
}) {
  const client = useQueryClient();
  const key = lifecycleKey(documentId);
  const { data: operation } = useQuery<LifecycleOperation | null>({ queryKey: key, queryFn: skipToken, initialData: null, enabled: false, gcTime: Infinity });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [reason, setReason] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const pending = operation?.status === 'pending';
  const unresolved = operation?.status === 'unknown' || pending;
  const rejected = operation?.status === 'rejected';
  const blocked = unresolved || rejected || refreshing;
  const canWithdraw = document && version?.capabilities.withdraw.status === 'available';
  const canEnd = document?.capabilities.endPublication.status === 'available' && Boolean(document.currentVersionId);
  useEffect(() => { setDraft(null); setReason(''); }, [contextKey]);
  useEffect(() => {
    if (operation?.status === 'succeeded') setDraft(null);
  }, [operation]);
  const problem = problemFromUnknown(operation?.error);
  const failure = problem ? mapApiProblem(problem).message : '通信結果を確認できません。';
  const activeIntent = blocked ? operation?.intent : undefined;
  const selectedKind = activeIntent?.kind ?? draft?.kind;
  const targetTitle = activeIntent?.title ?? draft?.title;
  const targetNo = activeIntent?.versionNo ?? draft?.versionNo;
  const allowed = draft?.documentId === documentId && draft.revision === document?.revision
    && (draft.kind === 'withdraw' ? canWithdraw && draft.versionId === version?.versionId : canEnd && draft.versionId === document?.currentVersionId);

  function restoreFocus() {
    requestAnimationFrame(() => {
      const target = trigger.current;
      if (target?.isConnected && !target.disabled) target.focus();
      else window.document.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')?.focus();
    });
  }
  function close() { if (!pending) { setDraft(null); setReason(''); restoreFocus(); } }
  function open(kind: 'withdraw' | 'end', event: React.MouseEvent<HTMLButtonElement>) {
    if (!document || blocked || (kind === 'withdraw' ? !canWithdraw : !canEnd)) return;
    trigger.current = event.currentTarget;
    client.setQueryData(key, null);
    setReason('');
    setDraft({ kind, documentId, title: document.title, versionId: kind === 'withdraw' ? version!.versionId : document.currentVersionId!,
      versionNo: kind === 'withdraw' ? version!.versionNo : document.displayVersion.versionNo, revision: document.revision });
  }
  function resume(event: React.MouseEvent<HTMLButtonElement>) {
    if (!operation) return;
    trigger.current = event.currentTarget;
    const intent = operation.intent;
    setDraft({ kind: intent.kind, documentId: intent.documentId, title: intent.title, versionId: intent.kind === 'withdraw' ? intent.versionId : intent.body.expectedCurrentVersionId,
      versionNo: intent.versionNo, revision: intent.body.expectedRevision });
  }
  function submit(event: React.FormEvent) {
    event.preventDefault();
    if (pending || rejected || refreshing) return;
    if (operation?.status === 'unknown') { void runLifecycleOperation(client, operation.intent); return; }
    if (!draft || !allowed || !reason.trim()) return;
    const common = { operationId: createOperationId(), expectedRevision: draft.revision, reason: reason.trim() };
    const intent: LifecycleIntent = draft.kind === 'withdraw'
      ? { kind: 'withdraw', documentId, title: draft.title, versionNo: draft.versionNo, versionId: draft.versionId, body: common }
      : { kind: 'end', documentId, title: draft.title, versionNo: draft.versionNo, body: { ...common, expectedCurrentVersionId: draft.versionId } };
    void runLifecycleOperation(client, intent);
  }
  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await refreshLifecycleQueries(client, documentId);
      client.setQueryData(key, null); setDraft(null); setReason(''); restoreFocus();
    } catch { /* Keep the rejected request visible until the current state can be read. */ }
    finally { setRefreshing(false); }
  }
  if (!operation && !draft && !(showActions && (canWithdraw || canEnd))) return null;
  const feedback = <>
    {pending && <p role="status">操作結果を確認しています…</p>}
    {operation?.status === 'unknown' && <section role="alert"><h3>操作結果を確認できません</h3><p>同じ操作IDと内容で結果を照会・再試行します。新しい操作として送り直さないでください。再読み込みやタブを閉じる前に結果を確認してください。</p><p>{failure}</p></section>}
    {rejected && <p role="alert">{failure} 最新状態を確認してから操作をやり直してください。</p>}
    {problem?.traceId && <small>照会ID: {problem.traceId}</small>}
  </>;
  return <section aria-label="公開状態の操作" className={styles.contentSection}>
    {showActions && (canWithdraw || canEnd) && <div className={styles.actionRow}>
      {canWithdraw && <button type="button" disabled={Boolean(blocked)} onClick={event => open('withdraw', event)}>選択版を取下げ</button>}
      {canEnd && <button type="button" disabled={Boolean(blocked)} onClick={event => open('end', event)}>公開を終了</button>}
    </div>}
    {operation?.status === 'succeeded' && <p className={styles.noticeSuccess} role="status">{operation.message}</p>}
    {!draft && blocked && <>{feedback}<button type="button" onClick={resume}>未確認の操作を開く</button></>}
    <Modal isOpen={Boolean(draft)} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.dialogScrim}>
      <Dialog aria-labelledby="document-lifecycle-title" className={styles.confirmDialog}>
        <Heading slot="title" id="document-lifecycle-title">{selectedKind === 'withdraw' ? '版の取下げを確認' : '文書の公開終了を確認'}</Heading>
        <p>{targetTitle}{selectedKind === 'withdraw' ? ` · 版 ${targetNo}` : ' · 文書全体'}</p>
        {selectedKind === 'withdraw' ? <p>選択版を取下げます。現行版の場合は直前の公開版だけが安全性を確認できたときに復帰し、それ以外は現行の公開版が無くなります。過去版の取下げでは現行版は変わりません。関連する公開予約は無効になる場合があります。</p>
          : <p>通常の公開閲覧を終了し、公開予約を無効にします。原本と過去版は保持します。このバージョンでは通常の操作から再公開できません。</p>}
        <form onSubmit={submit} aria-busy={Boolean(pending || refreshing)}>
          <label className={workspace.formField}>理由<textarea required value={activeIntent?.body.reason ?? reason} disabled={Boolean(blocked)} onChange={event => setReason(event.target.value)} /></label>
          {feedback}
          {draft && !allowed && !blocked && <p role="alert">権限または文書の状態が更新されました。閉じて最新状態を確認してください。</p>}
          <div className={styles.actionRow}>
            <button type="button" autoFocus disabled={Boolean(pending || refreshing)} onClick={close}>{operation && operation.status !== 'succeeded' ? '閉じる' : 'キャンセル'}</button>
            {rejected ? <button type="button" disabled={refreshing} onClick={() => void refresh()}>最新状態を確認</button>
              : <button type="submit" disabled={Boolean(pending || refreshing || (!unresolved && (!allowed || !reason.trim())))}>{pending ? '処理中…' : unresolved ? '同じ内容で再試行' : selectedKind === 'withdraw' ? '取下げを確定' : '公開終了を確定'}</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </section>;
}
