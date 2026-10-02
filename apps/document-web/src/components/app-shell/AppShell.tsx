import type { ReactNode } from 'react';
import styles from './AppShell.module.css';

export type AppShellProps = {
  children: ReactNode;
  contextPanel?: ReactNode;
  contextPanelLabel?: string;
  navigationContent?: ReactNode;
  activeNavigation?: 'documents' | 'editing';
  headerContext?: ReactNode;
  showContextPanel?: boolean;
};

export function AppShell({
  children,
  contextPanel,
  contextPanelLabel = '文脈情報',
  navigationContent,
  activeNavigation = 'documents',
  headerContext = '文書',
  showContextPanel = true,
}: AppShellProps) {
  return (
    <div className={styles.shell} data-context-open={showContextPanel}>
      <a className={styles.skipLink} href="#main-content">
        メインコンテンツへ
      </a>
      <header className={styles.header}>
        <a className={styles.brand} href="/documents" aria-label="文書管理ホーム">
          <span className={styles.brandMark} aria-hidden="true">K</span>
          <span className={styles.brandName}>Knowledge Platform<small>文書管理</small></span>
        </a>
        <div className={styles.headerContext}>{headerContext}</div>
      </header>
      <div className={styles.body}>
        <nav className={styles.navigation} aria-label="メインナビゲーション">
          <div className={styles.primaryNavigation}>
            <a className={styles.navigationLink} href="/documents?view=published" aria-current={activeNavigation === 'documents' ? 'page' : undefined}>
              <span aria-hidden="true">▯</span>文書
            </a>
            <a className={styles.navigationLink} href="/documents?view=authoring" aria-current={activeNavigation === 'editing' ? 'page' : undefined}>
              <span aria-hidden="true">✎</span>編集作業
            </a>
          </div>
          {navigationContent && <div className={styles.navigationContent}>{navigationContent}</div>}
        </nav>
        <main className={styles.workspace} id="main-content" aria-label="文書ワークスペース" tabIndex={-1}>
          {children}
        </main>
        <aside className={styles.contextPanel} aria-label={contextPanelLabel} aria-hidden={!showContextPanel} inert={!showContextPanel}>
          {contextPanel ?? <p className={styles.contextEmpty}>項目を選択すると詳細が表示されます</p>}
        </aside>
      </div>
    </div>
  );
}
