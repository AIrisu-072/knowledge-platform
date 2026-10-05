import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';
import { validateListSearch } from '../../application/search-state';
import { validateTaskSearch } from '../../application/work-workspace';
import { useOrganizationContext } from '../../application/organization-context';
import styles from './AppShell.module.css';

export type AppShellProps = {
  children: ReactNode;
  contextPanel?: ReactNode;
  contextPanelLabel?: string;
  navigationContent?: ReactNode;
  activeNavigation?: 'documents' | 'editing' | 'tasks' | 'search';
  mainLabel?: string;
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
  mainLabel = '文書ワークスペース',
}: AppShellProps) {
  const organization = useOrganizationContext();
  const taskSearch = validateTaskSearch(Object.fromEntries(new URLSearchParams(organization.taskHref.split('?')[1] ?? '')));
  const organizationMode = Boolean(organization.session) || activeNavigation === 'tasks' || activeNavigation === 'search';
  return (
    <div className={styles.shell} data-context-open={showContextPanel} data-organization={organizationMode}>
      <a className={styles.skipLink} href="#main-content">
        メインコンテンツへ
      </a>
      <header className={styles.header}>
        <a className={styles.brand} href={organizationMode ? organization.taskHref : '/documents'} aria-label={organizationMode ? 'タスクホーム' : '文書管理ホーム'}>
          <span className={styles.brandMark} aria-hidden="true">K</span>
          <span className={styles.brandName}>Knowledge Platform<small>{organizationMode ? 'Organization Client' : '文書管理'}</small></span>
        </a>
        <div className={styles.headerContext}>{headerContext}</div>
      </header>
      <div className={styles.body}>
        <nav className={styles.navigation} aria-label="メインナビゲーション">
          <div className={styles.primaryNavigation}>
            {organizationMode && <Link className={styles.navigationLink} to="/tasks" search={taskSearch} aria-current={activeNavigation === 'tasks' ? 'page' : undefined}>タスク</Link>}
            {organizationMode ? <Link className={styles.navigationLink} to="/documents" search={validateListSearch({ view: 'published' })} aria-current={activeNavigation === 'documents' ? 'page' : undefined}>文書</Link> : <a className={styles.navigationLink} href="/documents?view=published" aria-current={activeNavigation === 'documents' ? 'page' : undefined}>
              <span aria-hidden="true">▯</span>文書
            </a>}
            {organizationMode ? <Link className={styles.navigationLink} to="/documents" search={validateListSearch({ view: 'authoring' })} aria-current={activeNavigation === 'editing' ? 'page' : undefined}>編集作業</Link> : <a className={styles.navigationLink} href="/documents?view=authoring" aria-current={activeNavigation === 'editing' ? 'page' : undefined}>
              <span aria-hidden="true">✎</span>編集作業
            </a>}
            {organizationMode && <Link className={styles.navigationLink} to="/search" aria-current={activeNavigation === 'search' ? 'page' : undefined}>検索</Link>}
          </div>
          {navigationContent && <div className={styles.navigationContent}>{navigationContent}</div>}
        </nav>
        <main className={styles.workspace} id="main-content" aria-label={mainLabel} tabIndex={-1}>
          {children}
        </main>
        <aside className={styles.contextPanel} aria-label={contextPanelLabel} aria-hidden={!showContextPanel} inert={!showContextPanel}>
          {contextPanel ?? <p className={styles.contextEmpty}>項目を選択すると詳細が表示されます</p>}
        </aside>
      </div>
    </div>
  );
}
