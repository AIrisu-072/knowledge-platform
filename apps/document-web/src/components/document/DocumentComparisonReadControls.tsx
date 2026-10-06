import type { DocumentComparisonRead } from '../../application/use-document-comparison';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentComparisonReadControls({ read }: { read: DocumentComparisonRead }) {
  return <>
    {read.initialLoading && <LoadingState label="比較結果を取得中" />}
    {read.busy && !read.initialLoading && <LoadingState label={read.adding ? '比較結果の続きを取得中' : '比較結果を読み直し中'} />}
    {read.error && <ApiFeedback error={read.error} />}
    <div className={styles.actionRow}>
      {read.canContinue && <button type="button" disabled={read.busy} onClick={read.loadMore}>
        {read.continuationError ? '比較結果の続きを再試行' : '比較結果をさらに表示'}
      </button>}
      <button type="button" disabled={read.busy} onClick={read.restart}>比較結果を最初から読み直す</button>
    </div>
  </>;
}
