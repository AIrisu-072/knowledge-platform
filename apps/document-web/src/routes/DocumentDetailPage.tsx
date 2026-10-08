import { DocumentReadState, DocumentReadStateRecovery } from '../components/document/DocumentReadState';
import { useDocumentViewReadState } from '../application/use-document-view-read-state';
import { invalidateDocumentReadStateViews } from '../application/document-view-navigation';
import { DocumentAccessPolicy, DocumentAccessPolicyRecovery } from '../components/document/DocumentAccessPolicy';
import { validateDocumentPolicy } from '../application/document-access-policy';
import { createdRangeRouteError, documentListUrlError } from '../application/document-created-range';
import { Fragment, useEffect, useRef, useState } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { defaultParseSearch, useLocation, useNavigate, useParams, useSearch } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import {
  documentApi,
  type DocumentDetail,
  type DocumentRevisionSummary,
  type FileList,
  type RevisionComparisonResponse,
  type Version,
  type VersionDetail,
} from '../application/document-workspace';
import { ApiFeedback, LoadingState } from '../components/shared/ApiFeedback';
import { denyDocumentRevisionReads, useDocumentRevisions, type DocumentRevisionsRead } from '../application/use-document-revisions';
import { denyDocumentHistoryReads, useDocumentHistory } from '../application/use-document-history';
import { DocumentEventHistory } from '../components/document/DocumentEventHistory';
import { DocumentContentHistory } from '../components/document/DocumentContentHistory';
import { useDocumentContentHistory, type DocumentContentHistoryRead, discardContentHistoryReads, denyDocumentContentHistoryReads } from '../application/use-document-content-history';
import { DocumentRevisionDetailPanel } from '../components/document/DocumentRevisionDetailPanel';
import { DocumentRevisionReadControls } from '../components/document/DocumentRevisionReadControls';
import { useDocumentComparison, type DocumentComparisonRead } from '../application/use-document-comparison';
import { FragmentView, operationLabel, unverifiedReason } from '../components/document/DocumentComparisonFragments';
import { DocumentWorkingComparison } from '../components/document/DocumentWorkingComparison';
import { DocumentComparisonReadControls } from '../components/document/DocumentComparisonReadControls';
import { DocumentScheduleCancellation } from '../components/document/DocumentScheduleCancellation';
import { CapabilityButton, availabilityReason } from '../components/shared/CapabilityButton';
import { OriginalVersionDownload } from '../components/shared/OriginalVersionDownload';
import { DocumentWorkingVersionEditor } from '../components/document/DocumentWorkingVersionEditor';
import { DocumentMove } from '../components/document/DocumentMove';
import { DocumentMetadataEditor } from '../components/document/DocumentMetadataEditor';
import { DocumentLifecycleOperations } from '../components/document/DocumentLifecycleOperations';
import { AppShell } from '../components/app-shell/AppShell';
import { workingEditorSource, workingOperationKey, unresolvedWorkingOperation, type WorkingOperation } from '../application/document-working-version';
import { createOperationId } from '../application/operation-id';
import { documentStatusLabel, versionStatusLabel } from '../view-model/document-status';
import { jstDateTimeLocalToUtc } from '../application/schedule-time';
import { formatDateTime } from '../view-model/date-time';
import { unreadFilterRouteError } from '../application/document-unread-filter';
import { metadataFilterRouteError } from '../application/document-metadata-filters';
import { validateListSearch, type DetailSearch, type DocumentDetailTab, type VersionWorkflow } from '../application/search-state';
import styles from './DocumentDetail.module.css';
import workspaceStyles from './DocumentWorkspace.module.css';

const tabs: Array<{ id: DocumentDetailTab; label: string }> = [
  { id: 'overview', label: '概要' },
  { id: 'versions', label: '版・改訂' },
  { id: 'compare', label: '新旧比較' },
  { id: 'history', label: '履歴' },
  { id: 'access', label: 'アクセス' },
];

export function DocumentDetailPage() {
  const { documentId } = useParams({ from: '/documents/$documentId' });
  const search = useSearch({ from: '/documents/$documentId' }) as DetailSearch;
  const location = useLocation();
  const navigate = useNavigate({ from: '/documents/$documentId' });
  const queryClient = useQueryClient();
  const [editingMode, setEditingMode] = useState<{ contextKey: string; mode: 'create' | 'update' } | null>(null);
  const [returnError, setReturnError] = useState<string | null>(null);
  useEffect(() => setReturnError(null), [search.returnTo]);
  const [publicationMethod, setPublicationMethod] = useState<'now' | 'scheduled'>('now');
  const detailQuery = useQuery({
    queryKey: ['document', documentId, search.view],
    queryFn: async ({ signal }) => {
      try { return await documentApi.getDocument(documentId, search.view); }
      catch (error) {
        if (!signal.aborted) {
          invalidateDocumentReadStateViews(queryClient, documentId);
          denyDocumentRevisionReads(queryClient, documentId, error);
          denyDocumentHistoryReads(queryClient, documentId, error);
          denyDocumentContentHistoryReads(queryClient, documentId, error);
        }
        throw error;
      }
    },
  });
  const document = detailQuery.error ? undefined : detailQuery.data;
  const canManageAccess = document?.capabilities.manageAccess.status === 'available';
  const visibleTabs = tabs.filter((tab) => tab.id !== 'access' || canManageAccess);
  const activeTab = visibleTabs.some((tab) => tab.id === search.tab) ? search.tab : 'overview';
  const [versionHistory, setVersionHistory] = useState<{ documentId: string; view: string } | null>(null);
  const historyVersions = versionHistory?.documentId === documentId && versionHistory.view === search.view;
  const versionPurpose = historyVersions ? 'history' : search.view;
  const versionsQuery = useQuery({
    queryKey: ['document-versions', documentId, search.view],
    queryFn: () => documentApi.listDocumentVersions(documentId, search.view),
    retry: false,
    enabled: Boolean(document && !historyVersions && (activeTab === 'versions' || activeTab === 'compare')),
  });
  const contentHistoryRead = useDocumentContentHistory(documentId, () => Boolean(document && historyVersions && activeTab === 'versions'));
  const historyReadsActive = useRef(false);
  useEffect(() => {
    const active = historyVersions && activeTab === 'versions';
    if (historyReadsActive.current && !active) discardContentHistoryReads(queryClient, documentId);
    historyReadsActive.current = active;
  }, [queryClient, documentId, historyVersions, activeTab]);
  const revisionRead = useDocumentRevisions(documentId, Boolean(document && (activeTab === 'versions' || activeTab === 'compare')));
  const historyRead = useDocumentHistory(documentId, Boolean(document && activeTab === 'history'));
  const accessQuery = useQuery({
    queryKey: ['document-access-policy', documentId],
    queryFn: async () => { const policy = await documentApi.getDocumentAccessPolicy(documentId); if (!validateDocumentPolicy(policy, documentId)) throw new Error('最新の文書とアクセス設定を確認できません。'); return policy; },
    enabled: Boolean(document && canManageAccess && activeTab === 'access'),
  });
  const versionItems = historyVersions ? contentHistoryRead.versions : versionsQuery.error ? [] : versionsQuery.data?.items ?? [];
  const selectedVersion = chooseVersion(document, versionItems, search.versionId);
  const currentFileVersionId = search.view === 'authoring' ? document?.displayVersion.versionId : document?.currentVersionId ?? document?.displayVersion.versionId;
  const detailVersionId = activeTab === 'overview' ? currentFileVersionId : selectedVersion?.versionId;
  const detailPurpose = activeTab === 'overview' ? search.view : versionPurpose;
  const versionDetailQuery = useQuery({
    queryKey: ['document-version', documentId, detailVersionId, detailPurpose],
    // Normal-purpose reads keep the existing reset contract: a Document reset
    // may temporarily detach their observer while the same GET is still pending.
    // Only history reads consume the signal to suppress cancelled old denials.
    queryFn: detailPurpose !== 'history' ? async () => {
      const version = await documentApi.getDocumentVersion(documentId, detailVersionId!, detailPurpose);
      if (version.versionId !== detailVersionId) throw new Error('選択したコンテンツ版を確認できません。');
      return version;
    } : async ({ signal }) => {
      try {
        const version = await documentApi.getDocumentVersion(documentId, detailVersionId!, detailPurpose);
        if (version.versionId !== detailVersionId) throw new Error('選択したコンテンツ版を確認できません。');
        return version;
      } catch (error) {
        if (!signal.aborted && detailPurpose === 'history') {
          denyDocumentContentHistoryReads(queryClient, documentId, error);
          denyDocumentRevisionReads(queryClient, documentId, error);
          denyDocumentHistoryReads(queryClient, documentId, error);
        }
        throw error;
      }
    },
    retry: false,
    enabled: Boolean(detailVersionId && document && (activeTab === 'versions' || activeTab === 'overview')),
  });
  const versionDetailContextKey = `${documentId}:${detailVersionId ?? ''}:${detailPurpose}`;
  const versionDetailContext = useRef(versionDetailContextKey);
  versionDetailContext.current = versionDetailContextKey;
  const filesQuery = useQuery({
    queryKey: ['document-version-files', documentId, currentFileVersionId, search.view],
    queryFn: () => documentApi.listVersionFiles(documentId, currentFileVersionId!, search.view),
    enabled: Boolean(document && currentFileVersionId && activeTab === 'overview'),
  });
  const overviewRef = useRef<HTMLDivElement>(null);
  const readState = useDocumentViewReadState({ document, view: search.view, activeTab, workflow: search.workflow,
    detailReady: detailQuery.isSuccess && !detailQuery.isFetching && versionDetailQuery.isSuccess && !versionDetailQuery.isFetching && !versionDetailQuery.error,
    filesReady: filesQuery.isSuccess && !filesQuery.isFetching && !filesQuery.error, overviewRef });
  const revisions = revisionRead.revisions;
  const revisionPair = chooseRevisionPair(revisions, search.baseRevisionId, search.targetRevisionId);
  const canCompare = Boolean(document && activeTab === 'compare' && revisionRead.ready && revisionPair.base && revisionPair.target && revisionPair.base.revisionId !== revisionPair.target.revisionId);
  const comparisonRead = useDocumentComparison(documentId, revisionPair.base?.revisionId, revisionPair.target?.revisionId, canCompare);

  function updateSearch(patch: Partial<DetailSearch>) {
    void navigate({
      search: (previous) => {
        const next = { ...previous, ...patch } as DetailSearch;
        for (const key of ['versionId', 'baseRevisionId', 'targetRevisionId', 'returnTo', 'workflow'] as const) {
          if (patch[key] === undefined && Object.prototype.hasOwnProperty.call(patch, key)) delete next[key];
        }
        return next;
      },
    });
  }

  async function goBack() {
    let listSearch = validateListSearch({});
    if (search.returnTo) {
      const lengthError = documentListUrlError(search.returnTo);
      if (lengthError) { setReturnError(lengthError); return; }
      try {
        const returnUrl = new URL(search.returnTo, window.location.origin);
        if (returnUrl.origin === window.location.origin && returnUrl.pathname === '/documents') {
          const parsed = defaultParseSearch(returnUrl.search);
          const error = metadataFilterRouteError(parsed) ?? unreadFilterRouteError(parsed) ?? createdRangeRouteError(parsed);
          if (error) { setReturnError(error); return; }
          listSearch = validateListSearch(parsed);
        }
      } catch {
        // The route validator has already rejected non-local return targets.
      }
    }
    await navigate({ to: '/documents', search: listSearch });
  }

  function retryAll() {
    void detailQuery.refetch();
  }

  const headingId = 'document-workspace-heading';
  const workingContextKey = `${documentId}:${search.view}:${activeTab}:${search.versionId ?? ''}:${search.workflow ?? ''}`;
  const workingMode = editingMode?.contextKey === workingContextKey ? editingMode.mode : workingEditorSource(document, versionDetailQuery.data).mode;
  const showContextPanel = !search.workflow && activeTab !== 'compare' && activeTab !== 'access';
  const contextPanel = document ? (
    <div className={styles.contextPanelContent}>
      <p className={styles.contextEyebrow}>{document.title}</p>
      <h2>原本と版</h2>
      <dl className={styles.contextSummary}>
        <dt>対象</dt><dd>現行版 · Version {document.displayVersion.versionNo}</dd>
        <dt>ファイル</dt><dd>{document.displayVersion.fileSummary.primary?.displayName ?? '原本ファイルなし'}</dd>
      </dl>
      {currentFileVersionId && versionDetailQuery.data?.capabilities.download.status === 'available' && (
        <OriginalVersionDownload documentId={documentId} purpose={search.view} versionId={currentFileVersionId} label="現行ファイルを取得" variant="primary" />
      )}
      <div className={styles.contextActions}>
        <button type="button" onClick={() => updateSearch({ tab: 'versions' })}>版の一覧を確認</button>
        <button type="button" onClick={() => updateSearch({ tab: 'compare' })}>新旧比較</button>
      </div>
      <p className={styles.contextHint}>ファイル取得は認可・監査後に提供されます。</p>
    </div>
  ) : <LoadingState label="文書情報を読み込み中" />;
  const versionsPanel = document ? (
    <VersionsTab
      key={`${documentId}:${search.view}`}
      requestedVersionId={search.versionId}
      historyVersions={historyVersions}
      historyRead={historyVersions ? contentHistoryRead : undefined}
      onToggleHistory={() => { setVersionHistory(historyVersions ? null : { documentId, view: search.view }); updateSearch({ versionId: undefined }); }}
      document={document}
      versions={versionItems}
      revisions={revisions}
      selectedVersion={selectedVersion}
      versionError={selectedVersion ? versionDetailQuery.error : null}
      onVersionRetry={() => { if (versionDetailContext.current === versionDetailContextKey && detailVersionId) void versionDetailQuery.refetch(); }}
      versionDetail={versionDetailQuery.error || versionDetailQuery.isFetching ? undefined : versionDetailQuery.data}
      versionsLoading={historyVersions ? contentHistoryRead.initialLoading : versionsQuery.isPending}
      versionsError={historyVersions ? contentHistoryRead.error : versionsQuery.error}
      revisionRead={revisionRead}
      revisionPair={revisionPair}
      onRetry={() => { if (historyVersions) contentHistoryRead.restart(); else void versionsQuery.refetch(); }}
      updateSearch={updateSearch}
      workflow={search.workflow}
      publicationMethod={publicationMethod}
      setPublicationMethod={setPublicationMethod}
      invalidate={async () => {
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ['document', documentId] }),
          queryClient.invalidateQueries({ queryKey: ['document-versions', documentId] }),
          queryClient.invalidateQueries({ queryKey: ['document-revisions', documentId] }),
          queryClient.invalidateQueries({ queryKey: ['documents'] }),
        ]);
      }}
    />
  ) : null;
  return (
    <AppShell
      activeNavigation={search.view === 'authoring' ? 'editing' : 'documents'}
      contextPanel={contextPanel}
      contextPanelLabel="原本と版"
      headerContext={<><span>文書</span>{document?.folderName && <>　/　{document.folderName}</>}</>}
      showContextPanel={Boolean(document && showContextPanel)}
    >
          <DocumentAccessPolicyRecovery />
          <DocumentReadStateRecovery />
          {search.workflow || activeTab === 'compare' ? (
            <header className={styles.workflowHeader}>
              <button type="button" onClick={() => search.workflow ? updateSearch({ workflow: undefined }) : updateSearch({ tab: 'versions' })}>{search.workflow ? '← 版の一覧へ戻る' : '← 版・改訂へ戻る'}</button>
              <div>
                <h1 id={headingId}>{search.workflow === 'newVersion' ? (workingMode === 'update' ? '作業版を編集' : '新しい版を作成') : search.workflow === 'publication' ? '公開・予約公開' : '新旧比較'}</h1>
                <p>{document?.title} · Version {document?.displayVersion.versionNo}</p>
          </div>
        </header>
      ) : (
        <header className={styles.detailHeader}>
          <div className={styles.breadcrumbRow}>
            <button type="button" onClick={() => void goBack()}>← 一覧へ戻る</button>
            <span aria-hidden="true">/</span>
            <span>文書ワークスペース</span>
          </div>
          {returnError && <p role="alert">{returnError}</p>}
          {document ? (
            <div className={styles.titleLine}>
              <div className={styles.titleWithState}>
                <div className={styles.titleHeading}>
                  <h1 id={headingId}>{document.title}</h1>
                  <span className={styles.titleStatus}>{documentStatusLabel(document)}</span>
                </div>
                <p className={styles.versionContext}>Version {document.displayVersion.versionNo}{document.folderName && <>　·　{document.folderName}</>}</p>
              </div>
              <CapabilityButton label="新しい版を作成" availability={document.capabilities.createVersion} className={styles.headerAction} onClick={() => updateSearch({ tab: 'versions', workflow: 'newVersion' })} />
            </div>
          ) : <h1 id={headingId}>文書ワークスペース</h1>}
        </header>
      )}

      <DocumentWorkingVersionEditor
        key={`working:${documentId}`}
        documentId={documentId}
        document={document}
        version={versionDetailQuery.error ? undefined : versionDetailQuery.data}
        purpose={search.view}
        active={search.workflow === 'newVersion' && activeTab === 'versions'}
        contextKey={workingContextKey}
        onModeChange={setEditingMode}
        showActions={activeTab === 'versions' && !search.workflow}
        onOpen={() => updateSearch({ tab: 'versions', workflow: 'newVersion' })}
        onClose={() => updateSearch({ workflow: undefined })}
      />
      <DocumentLifecycleOperations
        key={documentId}
        documentId={documentId}
        document={document}
        version={versionDetailQuery.error || versionDetailQuery.isFetching || (activeTab === 'versions' && versionDetailQuery.data?.versionId !== selectedVersion?.versionId) ? undefined : versionDetailQuery.data}
        contextKey={`${documentId}:${search.view}:${activeTab}:${search.versionId ?? ''}:${search.workflow ?? ''}`}
        showActions={activeTab === 'versions' && !search.workflow}
      />
      <DocumentMove document={document} purpose={search.view} currentRead={detailQuery.isSuccess && !detailQuery.isFetching} contextKey={location.href} />
      {detailQuery.isPending && <LoadingState label="文書情報を読み込み中" />}
      {detailQuery.error && <ApiFeedback error={detailQuery.error} onRetry={retryAll} />}
      {document && (
        <>
          {search.workflow || activeTab === 'compare' ? (
            search.workflow ? (
            <section className={styles.workflowPanel} aria-label={search.workflow === 'newVersion' ? '新版作成' : '公開・予約公開'}>
              {activeTab === 'versions' && search.workflow !== 'newVersion' && versionsPanel}
            </section>
            ) : (
              <section className={styles.wideWorkflowPanel} aria-label="新旧比較">
                <CompareTab
                  documentId={documentId}
                  purpose={search.view}
                  revisions={revisions}
                  pair={revisionPair}
                  revisionRead={revisionRead}
                  comparisonRead={canCompare ? comparisonRead : undefined}
                  updateSearch={updateSearch}
                />
              </section>
            )
          ) : (
            <>
              <div role="tablist" aria-label="文書の詳細" className={styles.tabs} onKeyDown={(event) => handleTabKeyDown(event, visibleTabs, updateSearch)}>
                {visibleTabs.map((tab) => (
                  <button key={tab.id} type="button" role="tab" id={`tab-${tab.id}`} aria-selected={activeTab === tab.id} aria-controls="document-tab-panel" tabIndex={activeTab === tab.id ? 0 : -1} onClick={() => updateSearch({ tab: tab.id, workflow: undefined })}>
                    {tab.label}
                  </button>
                ))}
              </div>
              <section id="document-tab-panel" role="tabpanel" aria-labelledby={`tab-${activeTab}`} tabIndex={0} className={styles.tabPanel}>
                {activeTab === 'overview' && <div ref={overviewRef}>{search.view === 'published' && !search.workflow && <DocumentReadState read={readState} />}<OverviewTab key={location.href} document={document} filesQuery={filesQuery} reload={async () => { const result = await detailQuery.refetch(); if (result.error) throw result.error; }} /></div>}
                {activeTab === 'versions' && search.workflow !== 'newVersion' && <>{versionsPanel}{search.view === 'authoring' && selectedVersion && (!search.versionId || search.versionId === selectedVersion.versionId) && <DocumentWorkingComparison key={`working-comparison:${location.href}:${selectedVersion.versionId}`} documentId={documentId} versionId={selectedVersion.versionId} /> }{!historyVersions && <DocumentContentHistory key={location.href} documentId={documentId} />}{selectedVersion && <DocumentScheduleCancellation key={`${documentId}:${selectedVersion.versionId}`} document={document} view={search.view} versionId={selectedVersion.versionId} version={versionDetailQuery.data} contextKey={`${documentId}:${search.view}:${activeTab}:${selectedVersion.versionId}`} currentRead={!detailQuery.isFetching && !detailQuery.isError && !versionDetailQuery.isFetching && !versionDetailQuery.isError} />}</>}
                {activeTab === 'history' && <DocumentEventHistory read={historyRead} />}
                {activeTab === 'access' && canManageAccess && <DocumentAccessPolicy document={document} view={search.view} policy={accessQuery.data} loading={accessQuery.isPending} error={accessQuery.error} />}
              </section>
            </>
          )}
        </>
      )}
    </AppShell>
  );
}

function OverviewTab({ document, filesQuery, reload }: {
  reload: () => Promise<unknown>;
  document: DocumentDetail;
  filesQuery: { data?: FileList; isPending: boolean; error: unknown; refetch: () => Promise<unknown> };
}) {
  const metadata = document.metadata ?? {};
  const mainMetadataKeys = new Set(['owning_department', 'document_type', 'category']);
  const additionalMetadata = Object.entries(metadata).filter(([key]) => !mainMetadataKeys.has(key));
  return (
    <div className={styles.overviewGrid}>
      <section className={styles.overviewSection}>
        <h2>基本情報</h2>
        <DocumentMetadataEditor document={document} reload={reload} />
        <dl className={styles.metadataGrid}>
          <dt>状態</dt><dd>{documentStatusLabel(document)}</dd>
          <dt>現行Version</dt><dd>Version {document.displayVersion.versionNo}</dd>
          <dt>正式改訂</dt><dd>{document.displayRevision?.label ?? '未発行'}</dd>
          <dt>{document.displayTimestamp.kind === 'workingUpdatedAt' ? '更新日時' : '公開日時'}</dt><dd>{formatDate(document.displayTimestamp.value)}</dd>
          <dt>更新日時</dt><dd>{formatDate(document.displayVersion.updatedAt)}</dd>
          <dt>フォルダー</dt><dd>{document.folderName ?? '所属フォルダーを確認できません'}</dd>
          {typeof metadata.owning_department === 'string' && <><dt>所管部署</dt><dd>{metadata.owning_department}</dd></>}
          {typeof metadata.document_type === 'string' && <><dt>文書種別</dt><dd>{metadata.document_type}</dd></>}
          {typeof metadata.category === 'string' && <><dt>カテゴリ</dt><dd>{metadata.category}</dd></>}
        </dl>
      </section>
      <div className={styles.overviewStack}>
        <section className={styles.overviewSection}>
          <h2>現行ファイル</h2>
          {filesQuery.isPending && <LoadingState label="ファイル一覧を読み込み中" />}
          {Boolean(filesQuery.error) && <ApiFeedback error={filesQuery.error} onRetry={() => void filesQuery.refetch()} />}
          {filesQuery.data?.items.length === 0 && <p className={styles.muted}>この版にファイルはありません。</p>}
          {filesQuery.data && filesQuery.data.items.length > 0 && (
            <ul className={styles.fileList}>
              {filesQuery.data.items.map((file) => <li key={`${file.contentItemId}:${file.representationId}`}>
                <div><strong>{file.displayName}</strong><small>{file.mediaType} · {formatBytes(file.sizeBytes)}</small></div>
              </li>)}
            </ul>
          )}
          <p className={styles.muted}>内容を確認するときは、現行版の原本を参照してください。</p>
        </section>
        <section className={styles.overviewSection}>
          <h2>その他の属性</h2>
          {additionalMetadata.length === 0
            ? <p className={styles.muted}>追加のメタデータはありません。</p>
            : <dl className={styles.metadataGrid}>{additionalMetadata.map(([key, value]) => <Fragment key={key}><dt>{key}</dt><dd>{formatValue(value)}</dd></Fragment>)}</dl>}
        </section>
      </div>
      <details className={styles.technicalDisclosure}>
        <summary>記録・技術情報を確認</summary>
        <dl className={styles.metadataGrid}>
          <dt>Document ID</dt><dd><code>{document.documentId}</code></dd>
          <dt>Document revision</dt><dd>{'revision' in document ? document.revision : '—'}</dd>
        </dl>
      </details>
    </div>
  );

}

function VersionsTab({
  versionError,
  onVersionRetry,
  requestedVersionId,
  historyVersions,
  historyRead,
  onToggleHistory,
  document,
  versions,
  revisions,
  selectedVersion,
  versionDetail,
  versionsLoading,
  versionsError,
  revisionRead,
  revisionPair,
  onRetry,
  updateSearch,
  workflow,
  publicationMethod,
  setPublicationMethod,
  invalidate,
}: {
  versionError: unknown;
  onVersionRetry: () => void;
  requestedVersionId?: string;
  historyVersions: boolean;
  historyRead?: DocumentContentHistoryRead;
  onToggleHistory: () => void;
  document: DocumentDetail;
  versions: Version[];
  revisions: DocumentRevisionSummary[];
  selectedVersion?: Version;
  versionDetail?: VersionDetail;
  versionsLoading: boolean;
  versionsError: unknown;
  revisionRead: DocumentRevisionsRead;
  revisionPair: ReturnType<typeof chooseRevisionPair>;
  onRetry: () => void;
  updateSearch: (patch: Partial<DetailSearch>) => void;
  workflow?: VersionWorkflow;
  publicationMethod: 'now' | 'scheduled';
  setPublicationMethod: (method: 'now' | 'scheduled') => void;
  invalidate: () => Promise<void>;
}) {
  const [selectedRevisionId, setSelectedRevisionId] = useState<string | null>(null);
  const [publicationConfirmed, setPublicationConfirmed] = useState(false);
  const [action, setAction] = useState<'publish' | 'schedule' | null>(null);
  const [scheduledAt, setScheduledAt] = useState('');
  const [publishIntent, setPublishIntent] = useState<{ operationId: string; expectedRevision: number } | null>(null);
  const [scheduleIntent, setScheduleIntent] = useState<{ operationId: string; expectedRevision: number; scheduledPublishAt: string } | null>(null);
  const [actionPending, setActionPending] = useState(false);
  const [actionError, setActionError] = useState<unknown>(null);
  const [actionMessage, setActionMessage] = useState('');
  const actionTriggerRef = useRef<HTMLButtonElement | null>(null);
  const actionReturnRef = useRef<HTMLButtonElement | null>(null);
  const documentId = document.documentId;
  const queryClient = useQueryClient();
  const revisionsPair = revisionPair;
  const { data: workingOperation } = useQuery<WorkingOperation | null>({ queryKey: workingOperationKey(documentId), queryFn: skipToken, enabled: false, gcTime: Infinity });
  const workingBlocked = workingOperation?.status === 'pending' || workingOperation?.status === 'unknown';

  useEffect(() => {
    if (action || actionPending) return;
    const trigger = actionTriggerRef.current;
    if (!trigger) return;
    const frame = window.requestAnimationFrame(() => {
      const target = trigger.isConnected && !trigger.disabled ? trigger : actionReturnRef.current;
      if (target?.isConnected && !target.disabled) target.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [action, actionPending]);

  async function confirmAction() {
    if (!selectedVersion || unresolvedWorkingOperation(queryClient, documentId)) return;
    setActionPending(true);
    setActionError(null);
    try {
      if (action === 'publish') {
        const intent = publishIntent ?? { operationId: createOperationId(), expectedRevision: document.revision };
        setPublishIntent(intent);
        const result = await documentApi.publishVersion(documentId, selectedVersion.versionId, intent);
        setActionMessage(`公開しました · ${formatDate(result.publishedAt)}`);
      } else if (action === 'schedule') {
        const scheduledPublishAt = jstDateTimeLocalToUtc(scheduledAt);
        if (!scheduledPublishAt) throw new Error('JSTの公開日時を入力してください。');
        const intent = scheduleIntent ?? {
          operationId: createOperationId(),
          expectedRevision: document.revision,
          scheduledPublishAt,
        };
        setScheduleIntent(intent);
        const result = await documentApi.schedulePublication(documentId, selectedVersion.versionId, intent);
        setActionMessage(`公開を予約しました · ${formatDate(result.scheduledPublishAt)}`);
      }
      setAction(null);
      setPublishIntent(null);
      setScheduleIntent(null);
      await invalidate();
      await queryClient.invalidateQueries({ queryKey: ['document-version', documentId] });
    } catch (error) {
      setActionError(error);
    } finally {
      setActionPending(false);
    }
  }

  function chooseVersionAndReset(versionId: string) {
    updateSearch({ versionId });
    setActionError(null);
    setPublishIntent(null);
    setScheduleIntent(null);
    setActionMessage('');
  }

  function openPublication(method: 'now' | 'scheduled') {
    if (!selectedVersion || unresolvedWorkingOperation(queryClient, documentId)) return;
    setPublicationMethod(method);
    setPublicationConfirmed(false);
    setScheduledAt('');
    setPublishIntent(null);
    setScheduleIntent(null);
    setActionError(null);
    setActionMessage('');
    updateSearch({ workflow: 'publication', versionId: selectedVersion.versionId });
  }

  function requestPublicationConfirmation(event: React.MouseEvent<HTMLButtonElement>) {
    if (unresolvedWorkingOperation(queryClient, documentId) || !publicationConfirmed || (publicationMethod === 'scheduled' && !jstDateTimeLocalToUtc(scheduledAt))) return;
    actionTriggerRef.current = event.currentTarget;
    setAction(publicationMethod === 'now' ? 'publish' : 'schedule');
    setActionError(null);
  }

  return (
    <div className={workflow ? styles.workflowContent : styles.versionLayout}>
      {workflow === 'publication' ? (
        <section className={styles.publicationWorkspace} aria-busy={actionPending}>
          {workingBlocked && <p role="alert">作業版の保存結果を確認するまで、公開・予約公開はできません。</p>}
          <div className={styles.publicationVersions}>
            <div><span>現在</span><strong>{versions.find((version) => version.isCurrent)?.versionNo ? `Version ${versions.find((version) => version.isCurrent)?.versionNo}` : document.currentVersionId ? `Version ${document.displayVersion.versionNo}` : '現行の公開版はありません'}</strong><small>{versions.find((version) => version.isCurrent) ? versionStatusLabel(versions.find((version) => version.isCurrent)!) : '現在の状態'}</small></div>
            <span aria-hidden="true">→</span>
            <div><span>公開対象</span><strong>Version {selectedVersion?.versionNo ?? '未選択'}</strong><small>{selectedVersion ? versionStatusLabel(selectedVersion) : '対象版を選択してください'}</small></div>
          </div>
          <dl className={styles.publicationFacts}>
            <dt>ファイル</dt><dd>{selectedVersion?.fileSummary.primary?.displayName ?? '原本ファイルなし'}</dd>
            <dt>所管部署</dt><dd>{metadataText(selectedVersion?.metadata, ['owning_department', 'department', 'owner']) ?? metadataText(document.metadata, ['owning_department', 'department', 'owner']) ?? '未設定'}</dd>
          </dl>
          <fieldset className={styles.publicationMethods}>
            <legend>公開方法</legend>
            <label><input type="radio" name="publication-method" value="now" checked={publicationMethod === 'now'} disabled={versionDetail?.capabilities.publish.status !== 'available'} onChange={() => { setPublicationMethod('now'); setPublicationConfirmed(false); setPublishIntent(null); setScheduleIntent(null); }} />
              <span><strong>今すぐ公開</strong><small>公開が確定した時点で現行版になります。</small></span>
            </label>
            <label><input type="radio" name="publication-method" value="scheduled" checked={publicationMethod === 'scheduled'} disabled={versionDetail?.capabilities.schedulePublication.status !== 'available'} onChange={() => { setPublicationMethod('scheduled'); setPublicationConfirmed(false); setPublishIntent(null); setScheduleIntent(null); }} />
              <span><strong>日時を指定</strong><small>指定時刻に公開処理を行います。</small></span>
            </label>
          </fieldset>
          {publicationMethod === 'scheduled' && (
            <label className={workspaceStyles.formField}>公開日時（JST / UTC+09:00）
              <input type="datetime-local" value={scheduledAt} onChange={(event) => { setScheduledAt(event.target.value); setScheduleIntent(null); setActionError(null); }} required />
            </label>
          )}
          <label className={styles.publicationConfirm}><input type="checkbox" checked={publicationConfirmed} onChange={(event) => setPublicationConfirmed(event.target.checked)} />公開対象の版とファイルを確認しました。</label>
          {actionMessage && <p className={styles.noticeSuccess} role="status">{actionMessage}</p>}
          {versionDetail?.capabilities.publish.status === 'disabled' && versionDetail.capabilities.schedulePublication.status === 'disabled' && <p className={styles.muted}>公開できません: {availabilityReason(versionDetail.capabilities.publish.reason)}</p>}
          <div className={styles.workflowFooter}>
            <button ref={actionReturnRef} type="button" onClick={() => updateSearch({ workflow: undefined })}>版の一覧へ戻る</button>
            <button className={workspaceStyles.primaryButton} type="button" disabled={workingBlocked || !publicationConfirmed || actionPending || (publicationMethod === 'now' ? versionDetail?.capabilities.publish.status !== 'available' : versionDetail?.capabilities.schedulePublication.status !== 'available' || !jstDateTimeLocalToUtc(scheduledAt))} onClick={requestPublicationConfirmation}>
              {publicationMethod === 'now' ? '公開する' : '公開を予約する'}
            </button>
          </div>
        </section>
      ) : (
        <>
          <section className={styles.contentSection}>
            <div className={styles.sectionHeading}>
              <div><h2>コンテンツ版</h2><p>WORKING版と正式に公開した改訂を分けて表示します。</p></div>
              <CapabilityButton label="新しい版を作成" availability={document.capabilities.createVersion} className={workspaceStyles.primaryButton} onClick={() => updateSearch({ workflow: 'newVersion' })} />
            </div>
            <button type="button" aria-pressed={historyVersions} onClick={onToggleHistory}>{historyVersions ? '通常の版表示に戻る' : '過去版を含めて表示'}</button>
            {historyVersions && <p>過去版の閲覧には現在の履歴閲覧権限が必要です。取下げ対象は一覧から明示的に選択してください。</p>}
            {historyRead && <div>
              {historyRead.canContinue && <button type="button" disabled={historyRead.busy} onClick={historyRead.loadMore}>{historyRead.continuationError ? '過去版の続きを再試行' : '過去版をさらに表示'}</button>}
              <button type="button" disabled={historyRead.busy} onClick={historyRead.restart}>過去版を最初から読み直す</button>
            </div>}
            {versionsLoading && <LoadingState label="コンテンツ版を読み込み中" />}
            {Boolean(versionsError) && <ApiFeedback error={versionsError} onRetry={onRetry} />}
            {!versionsLoading && !versionsError && versions.length === 0 && <p className={styles.muted}>表示できるコンテンツ版はありません。</p>}
            <ul className={styles.versionList}>
              {versions.map((version) => (
                <li key={version.versionId} className={version.versionId === selectedVersion?.versionId ? styles.versionSelected : undefined}>
                  <button className={styles.versionSelect} type="button" aria-pressed={version.versionId === selectedVersion?.versionId} onClick={() => chooseVersionAndReset(version.versionId)}>
                    <strong>{version.lifecycleState === 'working' ? 'WORKING · ' : ''}版 {version.versionNo}</strong>
                    <span>{versionStatusLabel(version)}</span>
                    {version.scheduledPublishAt && <span>予約公開 · {formatDate(version.scheduledPublishAt)}</span>}
                  </button>
                  <small>{version.fileSummary.authoritativeItemCount} ファイル · 更新 {formatDate(version.updatedAt)}</small>
                </li>
              ))}
            </ul>
            {requestedVersionId && !versionsLoading && !versionsError && !selectedVersion && <p>指定したコンテンツ版は一覧にありません。対象を選び直してください。</p>}
            {selectedVersion && (
              <div className={styles.selectedVersionActions}>
                {Boolean(versionError) && <ApiFeedback error={versionError} onRetry={onVersionRetry} />}
                <h3>選択中: {selectedVersion.lifecycleState === 'working' ? 'WORKING · ' : ''}版 {selectedVersion.versionNo}</h3>
                {versionDetail?.capabilities.publish.status === 'available' && <button type="button" disabled={workingBlocked} onClick={() => openPublication('now')}>公開する</button>}
                {versionDetail?.capabilities.schedulePublication.status === 'available' && <button type="button" disabled={workingBlocked} onClick={() => openPublication('scheduled')}>予約公開する</button>}
                {versionDetail?.capabilities.publish.status === 'disabled' && <p className={styles.muted}>公開できません: {availabilityReason(versionDetail.capabilities.publish.reason)}</p>}
                {versionDetail?.capabilities.download.status === 'available' && <p>原本ファイルは「概要」タブからダウンロードできます。</p>}
              </div>
            )}
            {actionMessage && <p className={styles.noticeSuccess} role="status">{actionMessage}</p>}
            {Boolean(actionError) && <ApiFeedback error={actionError} onRetry={() => void confirmAction()} />}
          </section>

          <section className={styles.contentSection}>
            <div className={styles.sectionHeading}>
              <div><h2>正式改訂</h2><p>公開時に確定した改訂番号とメタデータ履歴です。</p></div>
            </div>
            {revisionRead.ready && revisions.length === 0 && <p className={styles.muted}>正式改訂はありません。WORKING版は上の版一覧に表示されます。</p>}
            <DocumentRevisionReadControls read={revisionRead} />
            <ol className={styles.revisionTimeline} aria-label="正式改訂一覧">
              {revisions.map((revision) => (
                <li key={revision.revisionId}>
                  <strong>{revision.label}</strong>
                  <button type="button" onClick={() => setSelectedRevisionId(revision.revisionId)}>改訂 {revision.label} の詳細</button>
                  <span>{revision.sourceKind === 'metadataRevision' ? 'メタデータ改訂' : revision.sourceKind === 'withdrawFallback' ? '取下げ後の復帰' : '公開改訂'}</span>
                  <time dateTime={revision.createdAt}>{formatDate(revision.createdAt)}</time>
                  {revision.metadataSnapshotStatus === 'unavailableLegacy' && <span className={styles.statusWarning}>過去のメタデータは確認できません</span>}
                </li>
              ))}
            </ol>
            {selectedRevisionId && revisionRead.ready && revisions.some(revision => revision.revisionId === selectedRevisionId) && <DocumentRevisionDetailPanel key={`${documentId}:${selectedRevisionId}`} documentId={documentId} revisionId={selectedRevisionId} expectedVersionId={revisions.find(revision => revision.revisionId === selectedRevisionId)?.documentVersionId} onClose={() => setSelectedRevisionId(null)} />}
            {(revisions.length >= 2 || revisionsPair.unresolved) && (
              <div className={styles.comparisonChooser}>
                <p>比較する正式改訂</p>
                <RevisionPairInputs revisions={revisions} pair={revisionsPair} updateSearch={updateSearch} />
                <RevisionPairNotice pair={revisionsPair} />
                <p className={styles.muted}>正式改訂番号とWORKINGコンテンツ版は別の履歴です。</p>
              </div>
            )}
          </section>
        </>
      )}

      {action && (
        <Modal
          className={styles.dialogScrim}
          isOpen
          isKeyboardDismissDisabled={actionPending}
          isDismissable={false}
          onOpenChange={(isOpen) => {
            if (!isOpen && !actionPending) {
              setAction(null);
              setActionError(null);
            }
          }}
        >
          <Dialog className={styles.confirmDialog} aria-labelledby="publication-dialog-title">
            <Heading slot="title" id="publication-dialog-title">{action === 'publish' ? '公開を確認' : '予約公開を確認'}</Heading>
            <p>{action === 'publish' ? `版 ${selectedVersion?.versionNo} を今すぐ公開します。` : `版 ${selectedVersion?.versionNo} の公開日時を指定します。`}</p>
            {action === 'schedule' && <p>公開日時: {formatJstDateTime(scheduledAt)}</p>}
            {Boolean(actionError) && <ApiFeedback error={actionError} onRetry={() => void confirmAction()} />}
            <div className={styles.actionRow}>
              <button type="button" disabled={workingBlocked || actionPending || (action === 'schedule' && !jstDateTimeLocalToUtc(scheduledAt))} onClick={() => void confirmAction()}>{actionPending ? '処理中…' : actionError ? '同じ内容で再試行' : '確定する'}</button>
              <button type="button" autoFocus disabled={actionPending} onClick={() => { setAction(null); setActionError(null); }}>キャンセル</button>
            </div>
          </Dialog>
        </Modal>
      )}
    </div>
  );
}

function CompareTab({ documentId, purpose, revisions, pair, revisionRead, comparisonRead, updateSearch }: {
  documentId: string;
  purpose: 'published' | 'authoring';
  revisions: DocumentRevisionSummary[];
  pair: ReturnType<typeof chooseRevisionPair>;
  revisionRead: DocumentRevisionsRead;
  comparisonRead?: DocumentComparisonRead;
  updateSearch: (patch: Partial<DetailSearch>) => void;
}) {
  return (
    <div className={styles.compareLayout}>
      <div className={styles.sectionHeading}>
        <div><h2>正式改訂の比較</h2><p>比較範囲と判定はDocument Diffの応答をそのまま表示します。</p></div>
      </div>
      <DocumentRevisionReadControls read={revisionRead} />
      {revisionRead.ready && revisions.length < 2 && !pair.unresolved && <p className={styles.muted}>比較には2件以上の正式改訂が必要です。WORKING版は正式改訂と別に扱います。</p>}
      {(revisions.length >= 2 || pair.unresolved) && !revisionRead.error && (
        <div className={styles.compareInputs}>
          <RevisionPairInputs revisions={revisions} pair={pair} updateSearch={updateSearch} compare />
        </div>
      )}
      {!revisionRead.error && <RevisionPairNotice pair={pair} />}
      {comparisonRead && <DocumentComparisonReadControls read={comparisonRead} />}
      {comparisonRead?.comparison && <ComparisonResult documentId={documentId} purpose={purpose} comparison={comparisonRead.comparison} isReadable={comparisonRead.readableNow} watchReadLoss={comparisonRead.watchReadLoss} />}
    </div>
  );
}

function ComparisonResult({ documentId, purpose, comparison, isReadable, watchReadLoss }: { watchReadLoss: (onLoss: () => void) => () => void; isReadable: () => boolean; documentId: string; purpose: 'published' | 'authoring'; comparison: RevisionComparisonResponse }) {
  return (
    <div className={styles.comparisonResult}>
      <dl className={styles.resultFacts}>
        <dt>基準</dt><dd>{revisionLabel(comparison.baseRevision)}</dd>
        <dt>対象</dt><dd>{revisionLabel(comparison.targetRevision)}</dd>
        <dt>本文</dt><dd>{comparison.contentComparisonStatus === 'sameAuthoritativeVersion' ? '同じコンテンツ版のため本文比較なし' : verdictLabel(comparison.verdict ?? 'unknown')}</dd>
        <dt>比較範囲</dt><dd>{comparison.contentComparisonStatus === 'sameAuthoritativeVersion' ? '対象外' : coverageLabel(comparison.coverage ?? 'none')}</dd>
        <dt>メタデータ</dt><dd>{metadataStatusLabel(comparison.metadataComparisonStatus)}</dd>
      </dl>
      {comparison.metadataComparisonStatus === 'unavailableLegacy' && <p className={styles.statusWarning}>過去のメタデータを確認できないため、同一または差分とは判定していません。</p>}
      {comparison.metadataChanges.length > 0 && (
        <section className={styles.diffSection}>
          <h3>メタデータの変更</h3>
          <ul className={styles.diffList}>{comparison.metadataChanges.map((change, index) => <li key={`${change.path}:${index}`}><strong>{change.path}</strong><span>{formatValue(change.baseValue)} → {formatValue(change.targetValue)}</span></li>)}</ul>
        </section>
      )}
      {comparison.contentComparisonStatus === 'differentAuthoritativeVersions' && (
        <section className={styles.diffSection}>
          <h3>本文の変更</h3>
          {comparison.displayItems.length === 0 && <p className={styles.muted}>{comparison.nextCursor
            ? 'このページには表示できる差分がありません。続きの比較結果を確認してください。'
            : '表示できる差分はありません。判定は上記の比較範囲を確認してください。'}</p>}
          <ol className={styles.diffItems}>
            {comparison.displayItems.map((item) => (
              <li key={`${item.changeIndex}:${item.facet}`}>
                <h4>{item.facet} · {operationLabel(item.operation)}{item.relocation ? ` · ${item.relocation}` : ''}</h4>
                <div className={styles.diffColumns}>
                  <FragmentView label="基準" fragment={item.base} locator={item.baseLocator} />
                  <FragmentView label="対象" fragment={item.target} locator={item.targetLocator} />
                </div>
              </li>
            ))}
          </ol>
        </section>
      )}
      {comparison.unverifiedRegions.length > 0 && (
        <section className={styles.unverifiedSection} aria-labelledby="unverified-heading">
          <h3 id="unverified-heading">未比較範囲 · 取得済み{comparison.unverifiedRegions.length}件</h3>
          <p>この範囲の意味は比較できていません。原本をダウンロードして目視で確認してください。</p>
          <ul>
            {comparison.unverifiedRegions.map((region, index) => (
              <li key={`unverified-${index}`}>
                <span>{unverifiedReason(region.reason)}</span>
                {region.navigationHint && <span>{region.navigationHint}</span>}
                {region.base && <DownloadSourceButton isReadable={isReadable} watchReadLoss={watchReadLoss} documentId={documentId} purpose={purpose} versionId={region.base.versionId} evidence={region.base} label="基準原本を確認" />}
                {region.target && <DownloadSourceButton isReadable={isReadable} watchReadLoss={watchReadLoss} documentId={documentId} purpose={purpose} versionId={region.target.versionId} evidence={region.target} label="対象原本を確認" />}
              </li>
            ))}
          </ul>
        </section>
      )}
      {comparison.contentComparisonStatus === 'differentAuthoritativeVersions' && comparison.coverage !== 'full' && comparison.unverifiedRegions.length === 0 && (
        <section className={styles.unverifiedSection}>
          <h3>未比較範囲</h3>
          <p>比較範囲が完全ではありません。両方の原本をダウンロードして確認してください。</p>
          <div className={styles.actionRow}>
            <OriginalVersionDownload documentId={documentId} purpose={purpose} versionId={comparison.baseRevision.documentVersionId} label="基準原本を確認" />
            <OriginalVersionDownload documentId={documentId} purpose={purpose} versionId={comparison.targetRevision.documentVersionId} label="対象原本を確認" />
          </div>
        </section>
      )}
    </div>
  );
}

function DownloadSourceButton({ documentId, purpose, versionId, evidence, label, isReadable, watchReadLoss }: {
  isReadable: () => boolean;
  watchReadLoss: (onLoss: () => void) => () => void;
  documentId: string;
  purpose: 'published' | 'authoring';
  versionId: string;
  evidence: { contentItemId: string; representationId: string };
  label: string;
}) {
  const mounted = useRef(true); const active = useRef<AbortController | undefined>(undefined);
  const identity = JSON.stringify([documentId, purpose, versionId, evidence.contentItemId, evidence.representationId]);
  const liveIdentity = useRef(identity); liveIdentity.current = identity;
  const readable = useRef(isReadable); readable.current = isReadable;
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; active.current?.abort(); }; }, []);
  useEffect(() => () => { active.current?.abort(); }, [identity]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  async function download() {
    if (!readable.current() || active.current && !active.current.signal.aborted) return;
    const controller = new AbortController(); active.current = controller;
    const stopWatching = watchReadLoss(() => controller.abort());
    controller.signal.addEventListener('abort', stopWatching, { once: true });
    const current = () => mounted.current && liveIdentity.current === identity && active.current === controller && !controller.signal.aborted && readable.current();
    setPending(true);
    setError(null);
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, contentItemId: evidence.contentItemId, representationId: evidence.representationId, purpose }, { signal: controller.signal });
      if (!current()) { controller.abort(); return; }
      saveBlob(blob, `原本-${versionId}`);
    } catch (caught) {
      if (current()) setError(caught);
    } finally {
      stopWatching(); controller.signal.removeEventListener('abort', stopWatching);
      if (active.current === controller) { active.current = undefined; if (mounted.current) setPending(false); }
    }
  }
  return <span className={styles.originalAction}><button type="button" disabled={pending} onClick={() => void download()}>{pending ? '取得中…' : label}</button>{Boolean(error) && <ApiFeedback error={error} />}</span>;
}


function DownloadButton({ documentId, versionId, contentItemId, representationId, purpose, filename, label = 'ダウンロード' }: {
  documentId: string; versionId: string; contentItemId: string; representationId: string; purpose: 'published' | 'authoring' | 'history'; filename: string; label?: string;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  async function download() {
    setPending(true);
    setError(null);
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, contentItemId, representationId, purpose });
      saveBlob(blob, filename);
    } catch (caught) {
      setError(caught);
    } finally {
      setPending(false);
    }
  }
  return <span className={styles.downloadAction}><button type="button" disabled={pending} onClick={() => void download()}>{pending ? '取得中…' : label}</button>{Boolean(error) && <ApiFeedback error={error} />}</span>;
}

function chooseVersion(document: DocumentDetail | undefined, versions: Version[], selectedId?: string): Version | undefined {
  if (selectedId) {
    return versions.find((version) => version.versionId === selectedId);
  }
  const working = versions.find((version) => version.lifecycleState === 'working');
  if (working) return working;
  if (document?.currentVersionId) {
    const current = versions.find((version) => version.versionId === document.currentVersionId);
    if (current) return current;
  }
  return versions[0];
}

function chooseRevisionPair(revisions: DocumentRevisionSummary[], baseId?: string, targetId?: string) {
  const target = targetId === undefined ? revisions[0] : revisions.find((revision) => revision.revisionId === targetId);
  const base = baseId === undefined ? revisions.find((revision) => revision.revisionId !== target?.revisionId) : revisions.find((revision) => revision.revisionId === baseId);
  return { base, target, baseId: baseId ?? base?.revisionId, targetId: targetId ?? target?.revisionId,
    unresolved: Boolean(baseId !== undefined && !base || targetId !== undefined && !target) };
}

function RevisionPairInputs({ revisions, pair, updateSearch, compare = false }: {
  revisions: DocumentRevisionSummary[]; pair: ReturnType<typeof chooseRevisionPair>;
  updateSearch: (patch: Partial<DetailSearch>) => void; compare?: boolean;
}) {
  return <>
    <label>{compare ? '基準改訂' : '基準'}
      <select value={pair.baseId ?? ''} onChange={(event) => updateSearch({ baseRevisionId: event.target.value || undefined })}>
        {pair.baseId && !pair.base && <option value={pair.baseId}>未取得の基準改訂</option>}
        {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
      </select>
    </label>
    {compare && <span aria-hidden="true">→</span>}
    <label>{compare ? '比較対象' : '対象'}
      <select value={pair.targetId ?? ''} onChange={(event) => updateSearch({ targetRevisionId: event.target.value || undefined })}>
        {pair.targetId && !pair.target && <option value={pair.targetId}>未取得の対象改訂</option>}
        {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
      </select>
    </label>
  </>;
}

function RevisionPairNotice({ pair }: { pair: ReturnType<typeof chooseRevisionPair> }) {
  return pair.unresolved ? <p className={styles.muted}>未取得の選択があります。正式改訂の続きを表示するか、取得済みの改訂を選び直してください。指定した比較対象は保持しています。</p> : null;
}

function handleTabKeyDown(event: React.KeyboardEvent<HTMLDivElement>, visibleTabs: Array<{ id: DocumentDetailTab; label: string }>, updateSearch: (patch: Partial<DetailSearch>) => void) {
  if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft' && event.key !== 'Home' && event.key !== 'End') return;
  const current = visibleTabs.findIndex((tab) => document.activeElement?.id === `tab-${tab.id}`);
  if (current < 0) return;
  event.preventDefault();
  const targetIndex = event.key === 'Home' ? 0 : event.key === 'End' ? visibleTabs.length - 1 : (current + (event.key === 'ArrowRight' ? 1 : -1) + visibleTabs.length) % visibleTabs.length;
  const target = visibleTabs[targetIndex];
  if (!target) return;
  updateSearch({ tab: target.id });
  requestAnimationFrame(() => document.getElementById(`tab-${target.id}`)?.focus());
}


function revisionLabel(revision: { major: number; minor: number }) {
  return `${revision.major}.${revision.minor}`;
}

function verdictLabel(verdict: 'same' | 'different' | 'unknown') {
  return verdict === 'same' ? '同一' : verdict === 'different' ? '差分あり' : '不明';
}

function coverageLabel(coverage: 'full' | 'partial' | 'none') {
  return coverage === 'full' ? '全範囲' : coverage === 'partial' ? '一部のみ' : '比較できていません';
}

function metadataStatusLabel(status: 'same' | 'different' | 'unavailableLegacy') {
  return status === 'same' ? '変更なし' : status === 'different' ? '変更あり' : '過去データを確認できません';
}


function formatDate(value: string) {
  return formatDateTime(value, 'Asia/Tokyo');
}

function formatJstDateTime(value: string) {
  const instant = jstDateTimeLocalToUtc(value);
  return instant ? formatDate(instant) : '日時を確認してください';
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}

function metadataText(metadata: unknown, keys: string[]): string | null {
  if (typeof metadata !== 'object' || metadata === null || Array.isArray(metadata)) return null;
  const record = metadata as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.trim()) return value.trim();
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  }
  return null;
}

function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
