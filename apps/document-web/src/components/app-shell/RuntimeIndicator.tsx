import { Link } from '@tanstack/react-router';
import { useRuntime } from '../../runtime/runtime-context';
import styles from './AppShell.module.css';

/**
 * Runtime capability indication in the shell header. The browser runtime
 * renders nothing so existing browser screens stay unchanged; the desktop
 * runtime links to its local Workspace screen (not a primary navigation entry).
 */
export function RuntimeIndicator() {
  const runtime = useRuntime();
  if (runtime.kind !== 'desktop') return null;
  return (
    <div className={styles.runtimeIndicator}>
      <span>デスクトップで実行中</span>
      <Link to="/local-workspaces">ローカルWorkspace</Link>
    </div>
  );
}
