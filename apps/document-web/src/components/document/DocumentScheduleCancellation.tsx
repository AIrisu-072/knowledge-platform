import { useEffect, useRef, useState } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import type { DocumentDetail, VersionDetail } from '../../application/document-workspace';
import { createOperationId } from '../../application/operation-id';
import { refreshScheduleQueries, runScheduleCancellation, scheduleCancelKey, type ScheduleCancelIntent, type ScheduleCancelOperation } from '../../application/document-schedule-cancel';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { formatDateTime } from '../../view-model/date-time';
import styles from '../../routes/DocumentDetail.module.css';

type Draft = Omit<ScheduleCancelIntent, 'body'> & { scheduleId: string; revision: number; contextKey: string };
export function DocumentScheduleCancellation({ document, version, versionId, contextKey, currentRead, view }: {
  document: DocumentDetail; version?: VersionDetail; versionId: string; contextKey: string; currentRead: boolean; view: 'published' | 'authoring';
}) {
  const client = useQueryClient();
  const key = scheduleCancelKey(document.documentId, versionId);
  const { data: operation } = useQuery<ScheduleCancelOperation | null>({ queryKey: key, queryFn: skipToken, initialData: null, enabled: false, gcTime: Infinity });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const generation = useRef(0);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const pending = operation?.status === 'pending';
  const unknown = operation?.status === 'unknown';
  const rejected = operation?.status === 'rejected';
  const unresolved = pending || unknown;
  const blocked = unresolved || rejected || refreshing;
  const canCancel = currentRead && version?.versionId === versionId && version.currentPublicationScheduleId
    && version.scheduledPublishAt && version.capabilities.cancelPublicationSchedule.status === 'available';
  const visibleDraft = draft?.contextKey === contextKey ? draft : null;
  const allowed = Boolean(visibleDraft && canCancel && visibleDraft.documentId === document.documentId
    && visibleDraft.versionId === versionId && visibleDraft.scheduleId === version?.currentPublicationScheduleId
    && visibleDraft.scheduledPublishAt === version?.scheduledPublishAt && visibleDraft.revision === document.revision);
  useEffect(() => { generation.current += 1; setDraft(null); setRefreshing(false); }, [contextKey]);
  useEffect(() => () => { generation.current += 1; }, []);
  useEffect(() => {
    if (operation?.status !== 'succeeded' || draft?.contextKey !== contextKey) return;
    setDraft(null);
    return restoreFocus();
  }, [operation]);
  const active = blocked && operation ? operation.intent : visibleDraft;
  const problem = problemFromUnknown(operation?.error);
  const failure = problem ? mapApiProblem(problem).message : '通信結果を確認できません。';
  function restoreFocus() {
    const fence = generation.current;
    const frame = requestAnimationFrame(() => {
      if (generation.current !== fence) return;
      if (trigger.current?.isConnected && !trigger.current.disabled) trigger.current.focus();
      else window.document.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }
  function close() { if (client.getQueryData<ScheduleCancelOperation>(key)?.status !== 'pending' && !refreshing) { generation.current += 1; setDraft(null); restoreFocus(); } }
  function open(event: React.MouseEvent<HTMLButtonElement>) {
    if (!canCancel || blocked || !version) return;
    generation.current += 1; trigger.current = event.currentTarget; client.setQueryData(key, null);
    setDraft({ documentId: document.documentId, title: document.title, versionId, versionNo: version.versionNo,
      scheduleId: version.currentPublicationScheduleId!, scheduledPublishAt: version.scheduledPublishAt!, revision: document.revision, contextKey });
  }
  function resume(event: React.MouseEvent<HTMLButtonElement>) {
    if (!operation) return;
    generation.current += 1; trigger.current = event.currentTarget;
    setDraft({ ...operation.intent, scheduleId: operation.intent.body.publishOperationId, revision: operation.intent.body.expectedRevision, contextKey });
  }
  function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!visibleDraft || pending || rejected || refreshing) return;
    if (unknown && operation) { void runScheduleCancellation(client, operation.intent); return; }
    const reads = [['document', document.documentId, view], ['document-version', document.documentId, versionId, view]];
    if (reads.some(queryKey => { const state = client.getQueryState(queryKey); return state?.status !== 'success' || state.fetchStatus !== 'idle'; })) return;
    const liveDocument = client.getQueryData<DocumentDetail>(['document', document.documentId, view]);
    const liveVersion = client.getQueryData<VersionDetail>(['document-version', document.documentId, versionId, view]);
    if (!allowed || liveDocument?.revision !== visibleDraft.revision
      || liveVersion?.versionId !== visibleDraft.versionId || liveVersion.currentPublicationScheduleId !== visibleDraft.scheduleId
      || liveVersion.scheduledPublishAt !== visibleDraft.scheduledPublishAt
      || liveVersion.capabilities.cancelPublicationSchedule.status !== 'available') return;
    const intent: ScheduleCancelIntent = { documentId: visibleDraft.documentId, title: visibleDraft.title, versionId: visibleDraft.versionId,
      versionNo: visibleDraft.versionNo, scheduledPublishAt: visibleDraft.scheduledPublishAt,
      body: { operationId: createOperationId(), publishOperationId: visibleDraft.scheduleId, expectedRevision: visibleDraft.revision } };
    void runScheduleCancellation(client, intent);
  }
  async function refresh() {
    if (refreshing) return;
    const fence = generation.current; const rejectedOperation = operation;
    setRefreshing(true);
    try {
      await refreshScheduleQueries(client, document.documentId);
      if (generation.current !== fence) return;
      if (client.getQueryData(key) === rejectedOperation) client.setQueryData(key, null);
      setDraft(null); restoreFocus();
    } catch { /* Preserve rejection until the authoritative read succeeds. */ }
    finally { if (generation.current === fence) setRefreshing(false); }
  }
  if (!operation && !visibleDraft && !canCancel) return null;
  const feedback = <>
    {pending && <p role="status">取消結果を確認しています…</p>}
    {unknown && <section role="alert"><h3>取消結果を確認できません</h3><p>同じ操作ID・予約ID・内容で結果を照会・再試行します。再読み込みやタブを閉じる前に結果を確認してください。</p><p>{failure}</p></section>}
    {rejected && <p role="alert">{failure} 最新状態を確認してから操作をやり直してください。</p>}
    {problem?.traceId && <small>照会ID: {problem.traceId}</small>}
  </>;
  return <section aria-label="公開予約の取消操作" className={styles.contentSection}>
    {canCancel && <button type="button" disabled={Boolean(blocked)} onClick={open}>公開予約を取り消す</button>}
    {operation?.status === 'succeeded' && <p role="status" className={styles.noticeSuccess}>公開予約を取り消しました</p>}
    {!visibleDraft && blocked && <>{feedback}<button type="button" onClick={resume}>未確認の取消を開く</button></>}
    <Modal isOpen={Boolean(visibleDraft)} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending && !refreshing} isKeyboardDismissDisabled={pending || refreshing} className={styles.dialogScrim}>
      <Dialog aria-labelledby="schedule-cancel-heading" className={styles.confirmDialog}>
        <Heading slot="title" id="schedule-cancel-heading">公開予約の取消</Heading>
        <p>{active?.title} · 版 {active?.versionNo}</p>
        {active && <p>予約日時: <time dateTime={active.scheduledPublishAt}>{formatDateTime(active.scheduledPublishAt, 'Asia/Tokyo')}</time></p>}
        <p>この公開予約を取り消します。作業版と原本は保持されます。再予約は取消後に別の操作で行ってください。</p>
        <form onSubmit={submit} aria-busy={pending || refreshing}>
          {feedback}
          {!blocked && !allowed && <p role="alert">予約または権限が更新されました。戻って最新状態を確認してください。</p>}
          <div className={styles.actionRow}>
            <button type="button" autoFocus disabled={pending || refreshing} onClick={close}>戻る</button>
            {rejected ? <button type="button" disabled={refreshing} onClick={() => void refresh()}>最新状態を確認</button>
              : <button type="submit" disabled={pending || refreshing || (!unknown && !allowed)}>{pending ? '処理中…' : unknown ? '同じ内容で再試行' : '予約を取り消す'}</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </section>;
}
