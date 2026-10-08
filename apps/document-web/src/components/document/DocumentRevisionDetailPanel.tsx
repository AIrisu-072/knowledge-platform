import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { documentApi } from '../../application/document-workspace';
import { denyDocumentRevisionReads } from '../../application/use-document-revisions';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import { formatDateTime } from '../../view-model/date-time';

const sourceLabels = { initialPublication: '初回公開', contentPublication: '内容の公開', metadataRevision: 'メタデータ改訂', withdrawFallback: '取下げ後の復帰', legacyBackfill: '旧データからの履歴' };

export function DocumentRevisionDetailPanel({ documentId, revisionId, expectedVersionId, onClose }: { documentId: string; revisionId: string; expectedVersionId?: string; onClose: () => void }) {
  const client = useQueryClient();
  const [visible, setVisible] = useState(() => document.visibilityState !== 'hidden');
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  const query = useQuery({
    queryKey: ['document-revisions', documentId, 'detail', revisionId],
    retry: false,
    enabled: visible,
    staleTime: 0,
    queryFn: async ({ signal }) => {
      try {
        const detail = await documentApi.getDocumentRevision(documentId, revisionId, signal);
        if (detail.revisionId !== revisionId || detail.documentVersionId !== expectedVersionId) throw new Error('選択した改訂の詳細を確認できません。');
        return detail;
      } catch (error) {
        if (!signal.aborted) denyDocumentRevisionReads(client, documentId, error);
        throw error;
      }
    },
  });
  // A cached snapshot is never shown while the current permission check is pending.
  const detail = !query.isFetching && !query.error ? query.data : undefined;
  if (!visible) return null;
  return <section aria-label="正式改訂の詳細">
    <h3>正式改訂の詳細{detail ? ` · ${detail.label}` : ''}</h3>
    <button type="button" onClick={onClose}>改訂詳細を閉じる</button>
    {(query.isPending || query.isFetching) && <LoadingState label="正式改訂の詳細を読み込み中" />}
    {Boolean(query.error) && <ApiFeedback error={query.error} onRetry={() => void query.refetch()} />}
    {detail && <dl>
      <dt>確定日時</dt><dd>{formatDateTime(detail.createdAt)}</dd>
      <dt>種類</dt><dd>{sourceLabels[detail.sourceKind]}</dd>
      <dt>内容版ID</dt><dd>{detail.documentVersionId}</dd>
      <dt>保存時のメタデータ</dt><dd>{detail.metadataSnapshotStatus === 'unavailableLegacy' || detail.metadataSnapshot === null ? '過去のメタデータは確認できません' : <pre>{JSON.stringify(detail.metadataSnapshot, null, 2)}</pre>}</dd>
      <dt>実行者</dt><dd>{detail.actor ? `${detail.actor.identityProvider} / ${detail.actor.principalId}` : '記録なし'}</dd>
      <dt>理由</dt><dd>{detail.reason ?? '記録なし'}</dd>
    </dl>}
  </section>;
}
