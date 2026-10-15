import type { DisplayFragment, SourceLocator } from '../../application/document-workspace';
import styles from '../../routes/DocumentDetail.module.css';

export function FragmentView({ label, fragment, locator }: { label: string; fragment: DisplayFragment | null; locator?: SourceLocator | null }) {
  return (
    <div className={styles.fragmentView}>
      <h5>{label}</h5>
      {locator && <small className={styles.locatorLabel}>原本の位置: {locatorLabel(locator)}</small>}
      {!fragment && <p className={styles.muted}>比較対象なし</p>}
      {fragment?.kind === 'text' && <><pre>{fragment.text}</pre>{fragment.truncated && <p className={styles.statusWarning}>表示を省略しました。原本を確認してください。</p>}</>}
      {fragment?.kind === 'structural' && <p>{fragment.summary}</p>}
      {fragment?.kind === 'unavailable' && <p className={styles.statusWarning}>この内容は表示できません: {fragment.reason}</p>}
      {fragment?.kind === 'table' && <><div className={styles.fragmentTable} role="table" aria-label={`${label}の表`}>{fragment.cells.map((cell, index) => <div role="row" key={`${cell.row}:${cell.column}:${index}`}><span role="cell">{cell.label ?? `R${cell.row ?? '?'} C${cell.column ?? '?'}`}</span><span role="cell">{cell.value}</span></div>)}</div>{fragment.truncated && <p className={styles.statusWarning}>表示を省略しました。原本を確認してください。</p>}</>}
    </div>
  );
}

export function operationLabel(operation: 'added' | 'removed' | 'modified' | null) {
  return operation === 'added' ? '追加' : operation === 'removed' ? '削除' : operation === 'modified' ? '変更' : '差分';
}

export function unverifiedReason(reason: string) {
  const labels: Record<string, string> = {
    unsupportedSemanticConstruct: '未対応の意味構造',
    corruptedSource: '原本が破損している可能性',
    missingInspectionEvidence: '検査証拠がありません',
    ambiguousAlignment: '対応位置を特定できません',
    resourceLimit: '比較上限に達しました',
  };
  return labels[reason] ?? reason;
}

function locatorLabel(locator: SourceLocator): string {
  switch (locator.kind) {
    case 'contentItem': return 'ファイル全体';
    case 'textSpan': return `${locator.line}行目`;
    case 'csvCell': return `${locator.row}行 ${locator.column}列`;
    case 'htmlNode': return locator.path;
    case 'officePath': return locator.path;
    case 'sheetCell': return `${locator.sheet} · ${locator.cell}`;
    case 'vbaModule': return `${locator.module}${locator.procedure ? ` · ${locator.procedure}` : ''}`;
    case 'slideObject': return `${locator.slide}枚目${locator.object ? ` · ${locator.object}` : ''}`;
    case 'pdfPage': return `${locator.page}ページ`;
  }
}
