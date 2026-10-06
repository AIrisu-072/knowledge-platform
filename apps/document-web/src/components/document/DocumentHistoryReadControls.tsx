import type { DocumentHistoryRead } from '../../application/use-document-history';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentHistoryReadControls({ read }: { read: DocumentHistoryRead }) {
  return <>
    {read.initialLoading && <LoadingState label="履歴を読み込み中" />}
    {read.busy && !read.initialLoading && <LoadingState label={read.adding ? '変更履歴の続きを取得中' : '変更履歴を読み直し中'} />}
    {Boolean(read.error) && <ApiFeedback error={read.error} />}
    <div className={styles.actionRow}>
      {read.canContinue && <button type="button" disabled={read.busy} onClick={read.loadMore}>
        {read.continuationError ? '変更履歴の続きを再試行' : '変更履歴をさらに表示'}
      </button>}
      <button type="button" disabled={read.busy} onClick={read.restart}>変更履歴を最初から読み直す</button>
    </div>
  </>;
}
