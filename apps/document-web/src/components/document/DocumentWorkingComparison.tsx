import { useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useWorkingComparison, workingComparisonSource } from '../../application/use-working-comparison';
import { FragmentView, operationLabel, unverifiedReason } from './DocumentComparisonFragments';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';

export function DocumentWorkingComparison({ documentId, versionId }: { documentId: string; versionId: string }) {
  const client = useQueryClient(); const [open, setOpen] = useState(false); const trigger = useRef<HTMLButtonElement>(null);
  if (open) return <WorkingComparisonRead documentId={documentId} versionId={versionId} onClose={() => {
    setOpen(false); window.requestAnimationFrame(() => { if (trigger.current?.isConnected && document.activeElement === document.body) trigger.current.focus(); });
  }} />;
  if (!workingComparisonSource(client, documentId, versionId)) return null;
  return <button type="button" ref={trigger} onClick={() => setOpen(true)}>現行公開版とこの作業版を比較</button>;
}
function WorkingComparisonRead({ documentId, versionId, onClose }: { documentId: string; versionId: string; onClose: () => void }) {
  const read = useWorkingComparison(documentId, versionId); const comparison = read.comparison;
  return <section className={styles.contentSection} aria-label="公開前の内容比較">
    <h2>公開前の内容比較</h2>
    <p>表示した版ID同士の内容比較です。比較前後の読取で現行公開版と作業版を確認しています。公開時まで現行版が変わらないことを保証するものではありません。</p>
    {read.busy && <LoadingState label={comparison ? '内容比較の続きを取得中' : '内容比較を取得中'} />}
    {Boolean(read.error) && <ApiFeedback error={read.error} />}
    <div className={styles.actionRow}>
      {read.canContinue && <button type="button" disabled={read.busy} onClick={read.loadMore}>{read.continuationError ? '内容比較の続きを再試行' : '内容比較をさらに表示'}</button>}
      <button type="button" disabled={read.busy} onClick={read.restart}>内容比較を最初から読み直す</button>
      <button type="button" onClick={onClose}>内容比較を閉じる</button>
    </div>
    {comparison && read.canonical && <div className={styles.comparisonResult}>
      <dl className={styles.resultFacts}>
        <dt>基準</dt><dd>Version {read.canonical.base.versionNo} · {read.canonical.base.title}</dd>
        <dt>対象</dt><dd>WORKING · Version {read.canonical.target.versionNo} · {read.canonical.target.title}</dd>
        <dt>基準版ID</dt><dd>{read.canonical.base.versionId}</dd><dt>対象版ID</dt><dd>{read.canonical.target.versionId}</dd>
        <dt>本文</dt><dd>{comparison.verdict === 'same' ? '同一' : comparison.verdict === 'different' ? '差分あり' : '不明'}</dd>
        <dt>比較範囲</dt><dd>{comparison.coverage === 'full' ? '全範囲' : comparison.coverage === 'partial' ? '一部のみ' : '比較できていません'}</dd>
      </dl>
      <section className={styles.diffSection}><h3>本文の変更</h3>
        {comparison.items.length === 0 && <p>{comparison.nextCursor ? 'このページには表示できる差分がありません。続きの比較結果を確認してください。' : '表示できる差分はありません。判定は上記の比較範囲を確認してください。'}</p>}
        <ol className={styles.diffItems}>{comparison.items.map(item => <li key={`${item.changeIndex}:${item.facet}`}>
          <h4>{item.facet} · {operationLabel(item.operation)}{item.relocation ? ` · ${item.relocation}` : ''}</h4>
          <div className={styles.diffColumns}><FragmentView label="基準" fragment={item.base} locator={item.baseLocator} /><FragmentView label="対象" fragment={item.target} locator={item.targetLocator} /></div>
        </li>)}</ol>
      </section>
      {(comparison.coverage !== 'full' || comparison.unverifiedRegions.length > 0) && <section className={styles.unverifiedSection} aria-label="内容比較の未比較範囲">
        <h3>未比較範囲 · 取得済み{comparison.unverifiedRegions.length}件</h3>
        <p>この範囲の意味は比較できていません。「コンテンツ版の履歴を開く」から表示した版IDの原本を確認してください。</p>
        <ul>{comparison.unverifiedRegions.map((region, index) => <li key={index}><span>{unverifiedReason(region.reason)}</span>{region.navigationHint && <span>{region.navigationHint}</span>}</li>)}</ul>
      </section>}
    </div>}
  </section>;
}
