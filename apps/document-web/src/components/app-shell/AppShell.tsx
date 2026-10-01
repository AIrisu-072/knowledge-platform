import type { ReactNode } from 'react';
import styles from './AppShell.module.css';

export type AppShellProps = {
  children: ReactNode;
  contextPanel?: ReactNode;
};

export function AppShell({ children, contextPanel }: AppShellProps) {
  return (
    <div className={styles.shell}>
      <a className={styles.skipLink} href="#main-content">
        メインコンテンツへ
      </a>
      <header className={styles.header}>
        <a className={styles.brand} href="/documents" aria-label="文書管理ホーム">
          Knowledge Platform
        </a>
        <span className={styles.headerTitle}>文書管理</span>
      </header>
      <div className={styles.body}>
        <nav className={styles.navigation} aria-label="メインナビゲーション">
          <a className={styles.navigationLink} href="/documents" aria-current="page">
            文書一覧
          </a>
        </nav>
        <main className={styles.workspace} id="main-content" aria-label="文書ワークスペース" tabIndex={-1}>
          {children}
        </main>
        <aside className={styles.contextPanel} aria-label="文脈情報">
          {contextPanel ?? <p className={styles.contextEmpty}>項目を選択すると詳細が表示されます</p>}
        </aside>
      </div>
    </div>
  );
}
