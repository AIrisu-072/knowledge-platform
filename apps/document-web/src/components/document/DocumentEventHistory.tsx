import type { DocumentHistoryRead } from '../../application/use-document-history';
import { DocumentHistoryReadControls } from './DocumentHistoryReadControls';
import { formatDateTime as formatDate } from '../../view-model/date-time';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentEventHistory({ read }: { read: DocumentHistoryRead }) {
  return (
    <section aria-labelledby="document-history-heading">
      <div className={styles.sectionHeading}><div><h2 id="document-history-heading">変更履歴</h2><p>記録された操作と由来を表示します。</p></div></div>
      <DocumentHistoryReadControls read={read} />
      {read.empty && <p className={styles.muted}>表示できる履歴はありません。</p>}
      <ol className={styles.historyList}>
        {read.entries.map((entry) => (
          <li key={JSON.stringify([entry.sourceKind, entry.sourceKey])}>
            <div className={styles.historyTitle}><strong>{entry.actionCode}</strong><span>{entry.provenanceQuality === 'operationLedger' ? '操作記録' : entry.provenanceQuality === 'versionFallback' ? '版からの履歴' : '由来不明の履歴'}</span></div>
            <time>{entry.occurredAt ? formatDate(entry.occurredAt, 'Asia/Tokyo') : '日時不明'}</time>
            <p>{entry.actor?.presentation.displayName ?? entry.actor?.principalId ?? '実行者不明'}{entry.actor?.presentation.resolution === 'notFound' ? ' · ディレクトリに存在しません' : entry.actor?.presentation.resolution === 'unavailable' ? ' · 表示情報を取得できません' : ''}</p>
          </li>
        ))}
      </ol>
    </section>
  );
}
