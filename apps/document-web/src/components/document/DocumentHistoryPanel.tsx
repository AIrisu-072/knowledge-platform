import { Fragment } from 'react';
import type { DocumentList } from '../../application/document-workspace';
import { useDocumentHistory } from '../../application/use-document-history';
import { DocumentContentHistory } from './DocumentContentHistory';
import { DocumentEventHistory } from './DocumentEventHistory';
import styles from '../../routes/DocumentWorkspace.module.css';

type HistoryDocument = Extract<DocumentList, { view: 'history' }>['items'][number];

export function DocumentHistoryPanel({ document, onClose, isDocumentReadable }: { document: HistoryDocument; onClose: () => void; isDocumentReadable: () => boolean }) {
  const history = useDocumentHistory(document.documentId, true, isDocumentReadable);
  return <section className={styles.panelContent} aria-label="選択した文書の履歴">
    <div className={styles.panelHeader}>
      <h2>選択した文書の履歴</h2>
      <button type="button" aria-label="履歴パネルを閉じる" onClick={onClose}>×</button>
    </div>
    <dl className={styles.summaryList}>
      <dt>代表版の文書名</dt><dd>{document.title}</dd>
      <dt>文書の公開終了</dt><dd>{document.ended ? '公開終了済み' : '公開終了していません'}</dd>
      <dt>代表版の状態</dt><dd>{document.displayVersion.lifecycleState}</dd>
      <dt>代表版</dt><dd>Version {document.displayVersion.versionNo}</dd>
      <dt>フォルダー</dt><dd>{document.folderName ?? '表示できません'}</dd>
      <dt>正式改訂</dt><dd>{document.displayRevision?.label ?? '正式改訂なし'}</dd>
    </dl>
    <p>PUBLISHEDは版に記録された状態です。現在の公開を示すものではありません。</p>
    <h3>現在の文書属性</h3>
    <dl className={styles.summaryList}>{Object.entries(document.metadata ?? {}).map(([key, value]) => <Fragment key={key}><dt>{key}</dt><dd>{typeof value === 'string' ? value : JSON.stringify(value)}</dd></Fragment>)}</dl>
    <p>一覧の代表版と現在の文書属性を表示しています。過去の文書属性ではありません。</p>
    <DocumentContentHistory documentId={document.documentId} isDocumentReadable={isDocumentReadable} />
    <DocumentEventHistory read={history} />
  </section>;
}
