import { useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { documentReadStateOperations, type ReadStateOperation } from '../../application/document-read-state';
import { refreshCurrentReadState, retryReadStateOperation, type useDocumentViewReadState } from '../../application/use-document-view-read-state';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { formatDateTime } from '../../view-model/date-time';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentReadState({ read }: { read: ReturnType<typeof useDocumentViewReadState> }) {
  return <section aria-label="本人の既読状態" className={styles.contentSection}>
    <h2>既読状態</h2>
    <p role="status">{read.loading ? '既読状態を確認中…' : read.currentState ? read.currentState.isRead ? '既読' : '未読' : '既読状態は未確認です'}</p>
    <dl className={styles.resultFacts}><dt>初回記録日時</dt><dd>{read.currentState?.firstReadAt ? formatDateTime(read.currentState.firstReadAt, 'Asia/Tokyo') : read.currentState ? '未記録' : '未確認'}</dd></dl>
    <p className={styles.muted}>既読は文書の詳細を表示した記録です。原本の読了や同意を示すものではありません</p>
    {read.currentState?.isRead && <button type="button" disabled={!read.canReset} onClick={() => void read.resetUnread()}>未読に戻す</button>}
    <button type="button" disabled={read.loading} onClick={() => void read.refreshCurrentState().catch(() => undefined)}>現在の既読状態を再取得</button>
    {read.errorMessage && <p role="alert">{read.errorMessage}</p>}
  </section>;
}
export function DocumentReadStateRecovery() {
  const client = useQueryClient(); const store = documentReadStateOperations(client); const operations = useSyncExternalStore(store.subscribe, store.list);
  async function refresh(operation: ReadStateOperation) {
    if (operation.status !== 'succeeded' || operation.refresh === 'pending') return;
    const reading = { ...operation, refresh: 'pending' as const }; store.put(reading);
    let state: 'complete' | 'failed' = 'complete';
    try { await refreshCurrentReadState(client, operation.intent); } catch { state = 'failed'; }
    if (store.get(operation.intent.documentId, operation.intent.versionId) === reading) store.put({ ...reading, refresh: state });
  }
  if (!operations.length) return null;
  return <section aria-label="既読状態の操作結果" className={styles.contentSection}>
    <h2>既読状態の操作結果</h2>
    {operations.map(operation => {
      const { intent } = operation; const problem = problemFromUnknown(operation.error);
      return <div key={`${intent.documentId}:${intent.versionId}`}>
        <p>対象文書：{intent.title} · Version {intent.versionNo}</p>
        {operation.status === 'pending' && <p role="status">既読状態の処理結果を確認しています…</p>}
        {operation.status === 'unknown' && <><p role="alert">結果を確認できません。同じ操作を再試行できます</p><button type="button" onClick={() => void retryReadStateOperation(client, intent)}>同じ操作を再試行</button></>}
        {operation.status === 'rejected' && <p role="alert">{problem ? mapApiProblem(problem).message : '既読状態の操作は拒否されました。'}</p>}
        {operation.status === 'succeeded' && <><p role="status">{intent.kind === 'RESET' ? '未読に戻しました。次に文書の詳細を開くと既読になります' : '文書の詳細を表示した記録を保存しました'}</p>
          {operation.refresh === 'pending' && <p role="status">現在の既読状態を取得しています…</p>}
          {operation.refresh === 'failed' && <><p role="alert">処理結果は確認済みですが、現在の既読状態を取得できません</p><button type="button" onClick={() => void refresh(operation)}>現在の既読状態を再取得</button></>}
        </>}
        <details><summary>送信時の記録を確認</summary><p>操作：{intent.kind === 'RESET' ? '未読に戻す' : '詳細の表示記録'}</p><p>操作ID：{intent.body.operationId}</p><p>送信時の既読状態revision：{intent.body.expectedReadStateRevision}</p><p>この記録は現在の既読状態ではありません。</p></details>
        {(operation.status === 'rejected' || operation.status === 'succeeded' && operation.refresh !== 'pending') && <button type="button" onClick={() => store.clearSettled(intent.documentId, intent.versionId, operation)}>結果表示を閉じる</button>}
      </div>;
    })}
  </section>;
}
