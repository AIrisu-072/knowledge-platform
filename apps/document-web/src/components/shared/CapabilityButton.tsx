import { useId } from 'react';
import type { DocumentDetail } from '../../application/document-workspace';

export function CapabilityButton({ label, availability, disabled = false, className, onClick }: {
  label: string; availability?: DocumentDetail['capabilities']['createVersion']; disabled?: boolean; className?: string; onClick: () => void;
}) {
  const reasonId = useId();
  if (!availability || (availability.status !== 'available' && availability.status !== 'disabled')) return null;
  const serverDisabled = availability.status === 'disabled';
  return <span>
    <button type="button" className={className} disabled={availability.status !== 'available' || disabled}
      aria-describedby={serverDisabled ? reasonId : undefined} onClick={onClick}>{label}</button>
    {serverDisabled && <small id={reasonId}> {availabilityReason(availability.reason)}</small>}
  </span>;
}

export function availabilityReason(reason: string): string {
  const labels: Record<string, string> = {
    permission: '権限がありません',
    lifecycle: '現在の状態では実行できません',
    pendingSchedule: '予約公開中です',
    staleBase: '元の版が更新されています',
    notCurrent: '現在の版ではありません',
    notHumanInteractive: '利用者による操作ではありません',
    unsupported: '現在の画面からは実行できません',
  };
  return labels[reason] ?? '実行できません';
}
