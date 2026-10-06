import type { DocumentRevisionsRead } from '../../application/use-document-revisions';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentRevisionReadControls({ read }: { read: DocumentRevisionsRead }) {
  return <>
    {read.initialLoading && <LoadingState label="正式改訂を取得中" />}
    {read.busy && !read.initialLoading && <LoadingState label={read.adding ? '正式改訂の続きを取得中' : '正式改訂を読み直し中'} />}
    {read.error && <ApiFeedback error={read.error} />}
    <div className={styles.actionRow}>
      {read.canContinue && <button type="button" disabled={read.busy} onClick={read.loadMore}>
        {read.continuationError ? '正式改訂の続きを再試行' : '正式改訂をさらに表示'}
      </button>}
      <button type="button" aria-label="正式改訂を最初から読み直す" disabled={read.busy} onClick={read.restart}>最初から読み直す</button>
    </div>
  </>;
}
