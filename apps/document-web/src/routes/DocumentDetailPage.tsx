import { Fragment, useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate, useParams, useSearch } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import {
  documentApi,
  type AccessPolicyRead,
  type CommandsPolicyExplicit,
  type CommandsPolicyInherit,
  type DocumentDetail,
  type DocumentRevisionSummary,
  type DisplayFragment,
  type FileList,
  type History,
  type PolicyGrantInput,
  type RevisionComparisonResponse,
  type SourceLocator,
  type Version,
  type VersionDetail,
} from '../application/document-workspace';
import { ApiFeedback, LoadingState } from '../components/shared/ApiFeedback';
import { OriginalVersionDownload } from '../components/shared/OriginalVersionDownload';
import { AppShell } from '../components/app-shell/AppShell';
import { createOperationId } from '../application/operation-id';
import { documentStatusLabel, versionStatusLabel } from '../view-model/document-status';
import { jstDateTimeLocalToUtc } from '../application/schedule-time';
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
  const navigate = useNavigate({ from: '/documents/$documentId' });
  const queryClient = useQueryClient();
  const [publicationMethod, setPublicationMethod] = useState<'now' | 'scheduled'>('now');
  const detailQuery = useQuery({
    queryKey: ['document', documentId, search.view],
    queryFn: () => documentApi.getDocument(documentId, search.view),
  });
  const document = detailQuery.data;
  const canManageAccess = document?.capabilities.manageAccess.status === 'available';
  const visibleTabs = tabs.filter((tab) => tab.id !== 'access' || canManageAccess);
  const activeTab = visibleTabs.some((tab) => tab.id === search.tab) ? search.tab : 'overview';
  const versionsQuery = useQuery({
    queryKey: ['document-versions', documentId, search.view],
    queryFn: () => documentApi.listDocumentVersions(documentId, search.view),
    enabled: Boolean(document && (activeTab === 'versions' || activeTab === 'compare')),
  });
  const revisionsQuery = useQuery({
    queryKey: ['document-revisions', documentId],
    queryFn: () => documentApi.listDocumentRevisions(documentId),
    enabled: Boolean(document && (activeTab === 'versions' || activeTab === 'compare')),
  });
  const historyQuery = useQuery({
    queryKey: ['document-history', documentId],
    queryFn: () => documentApi.getDocumentHistory(documentId),
    enabled: Boolean(document && activeTab === 'history'),
  });
  const accessQuery = useQuery({
    queryKey: ['document-access-policy', documentId],
    queryFn: () => documentApi.getDocumentAccessPolicy(documentId),
    enabled: Boolean(document && canManageAccess && activeTab === 'access'),
  });
  const versionItems = versionsQuery.data?.items ?? [];
  const selectedVersion = chooseVersion(document, versionItems, search.versionId);
  const currentFileVersionId = document?.currentVersionId ?? document?.displayVersion.versionId;
  const detailVersionId = activeTab === 'overview' ? currentFileVersionId : selectedVersion?.versionId;
  const versionDetailQuery = useQuery({
    queryKey: ['document-version', documentId, detailVersionId, search.view],
    queryFn: () => documentApi.getDocumentVersion(documentId, detailVersionId!, search.view),
    enabled: Boolean(detailVersionId && document && (activeTab === 'versions' || activeTab === 'overview')),
  });
  const filesQuery = useQuery({
    queryKey: ['document-version-files', documentId, currentFileVersionId, search.view],
    queryFn: () => documentApi.listVersionFiles(documentId, currentFileVersionId!, search.view),
    enabled: Boolean(document && currentFileVersionId && activeTab === 'overview'),
  });
  const revisions = revisionsQuery.data?.items ?? [];
  const revisionPair = chooseRevisionPair(revisions, search.baseRevisionId, search.targetRevisionId);
  const comparisonQuery = useQuery({
    queryKey: ['revision-comparison', documentId, revisionPair.base?.revisionId, revisionPair.target?.revisionId],
    queryFn: () => documentApi.compareDocumentRevisions(documentId, {
      baseRevisionId: revisionPair.base!.revisionId,
      targetRevisionId: revisionPair.target!.revisionId,
      projection: 'display',
      pageSize: 50,
    }),
    enabled: Boolean(document && activeTab === 'compare' && revisionPair.base && revisionPair.target && revisionPair.base.revisionId !== revisionPair.target.revisionId),
  });

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
      try {
        const returnUrl = new URL(search.returnTo, window.location.origin);
        if (returnUrl.origin === window.location.origin && returnUrl.pathname === '/documents') {
          listSearch = validateListSearch(Object.fromEntries(returnUrl.searchParams.entries()));
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
      document={document}
      versions={versionItems}
      revisions={revisions}
      selectedVersion={selectedVersion}
      versionDetail={versionDetailQuery.data}
      versionsLoading={versionsQuery.isPending || revisionsQuery.isPending}
      versionsError={versionsQuery.error ?? revisionsQuery.error}
      onRetry={() => { void versionsQuery.refetch(); void revisionsQuery.refetch(); }}
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
          {search.workflow || activeTab === 'compare' ? (
            <header className={styles.workflowHeader}>
              <button type="button" onClick={() => search.workflow ? updateSearch({ workflow: undefined }) : updateSearch({ tab: 'versions' })}>{search.workflow ? '← 版の一覧へ戻る' : '← 版・改訂へ戻る'}</button>
              <div>
                <h1 id={headingId}>{search.workflow === 'newVersion' ? '新しい版を作成' : search.workflow === 'publication' ? '公開・予約公開' : '新旧比較'}</h1>
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
          {document ? (
            <div className={styles.titleLine}>
              <div className={styles.titleWithState}>
                <div className={styles.titleHeading}>
                  <h1 id={headingId}>{document.title}</h1>
                  <span className={styles.titleStatus}>{documentStatusLabel(document)}</span>
                </div>
                <p className={styles.versionContext}>Version {document.displayVersion.versionNo}{document.folderName && <>　·　{document.folderName}</>}</p>
              </div>
              {document.capabilities.createVersion.status === 'available' && <button className={styles.headerAction} type="button" onClick={() => updateSearch({ tab: 'versions', workflow: 'newVersion' })}>新しい版を作成</button>}
            </div>
          ) : <h1 id={headingId}>文書ワークスペース</h1>}
        </header>
      )}

      {detailQuery.isPending && <LoadingState label="文書情報を読み込み中" />}
      {detailQuery.error && <ApiFeedback error={detailQuery.error} onRetry={retryAll} />}
      {document && (
        <>
          {search.workflow || activeTab === 'compare' ? (
            search.workflow ? (
            <section className={styles.workflowPanel} aria-label={search.workflow === 'newVersion' ? '新版作成' : '公開・予約公開'}>
              {activeTab === 'versions' && versionsPanel}
            </section>
            ) : (
              <section className={styles.wideWorkflowPanel} aria-label="新旧比較">
                <CompareTab
                  documentId={documentId}
                  purpose={search.view}
                  revisions={revisions}
                  pair={revisionPair}
                  comparison={comparisonQuery.data}
                  loading={revisionsQuery.isPending || comparisonQuery.isPending}
                  error={revisionsQuery.error ?? comparisonQuery.error}
                  onRetry={() => { void revisionsQuery.refetch(); void comparisonQuery.refetch(); }}
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
                {activeTab === 'overview' && <OverviewTab document={document} filesQuery={filesQuery} />}
                {activeTab === 'versions' && versionsPanel}
                {activeTab === 'history' && <HistoryTab query={historyQuery} />}
                {activeTab === 'access' && canManageAccess && <AccessTab documentId={documentId} documentTitle={document.title} documentFolderId={document.folderId ?? null} folderName={document.folderName ?? null} policy={accessQuery.data} loading={accessQuery.isPending} error={accessQuery.error} onRetry={() => void accessQuery.refetch()} />}
              </section>
            </>
          )}
        </>
      )}
    </AppShell>
  );
}

function OverviewTab({ document, filesQuery }: {
  document: DocumentDetail;
  filesQuery: { data?: FileList; isPending: boolean; error: unknown; refetch: () => Promise<unknown> };
}) {
  const metadata = document.metadata ?? {};
  const mainMetadataKeys = new Set(['department', 'documentType', 'category']);
  const additionalMetadata = Object.entries(metadata).filter(([key]) => !mainMetadataKeys.has(key));
  return (
    <div className={styles.overviewGrid}>
      <section className={styles.overviewSection}>
        <h2>基本情報</h2>
        <dl className={styles.metadataGrid}>
          <dt>状態</dt><dd>{documentStatusLabel(document)}</dd>
          <dt>現行Version</dt><dd>Version {document.displayVersion.versionNo}</dd>
          <dt>正式改訂</dt><dd>{document.displayRevision?.label ?? '未発行'}</dd>
          <dt>{document.displayTimestamp.kind === 'workingUpdatedAt' ? '更新日時' : '公開日時'}</dt><dd>{formatDate(document.displayTimestamp.value)}</dd>
          <dt>更新日時</dt><dd>{formatDate(document.displayVersion.updatedAt)}</dd>
          <dt>フォルダー</dt><dd>{document.folderName ?? 'ルート'}</dd>
          {typeof metadata.department === 'string' && <><dt>所管部署</dt><dd>{metadata.department}</dd></>}
          {typeof metadata.documentType === 'string' && <><dt>文書種別</dt><dd>{metadata.documentType}</dd></>}
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
          <h2>主要メタデータ</h2>
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
  document,
  versions,
  revisions,
  selectedVersion,
  versionDetail,
  versionsLoading,
  versionsError,
  onRetry,
  updateSearch,
  workflow,
  publicationMethod,
  setPublicationMethod,
  invalidate,
}: {
  document: DocumentDetail;
  versions: Version[];
  revisions: DocumentRevisionSummary[];
  selectedVersion?: Version;
  versionDetail?: VersionDetail;
  versionsLoading: boolean;
  versionsError: unknown;
  onRetry: () => void;
  updateSearch: (patch: Partial<DetailSearch>) => void;
  workflow?: VersionWorkflow;
  publicationMethod: 'now' | 'scheduled';
  setPublicationMethod: (method: 'now' | 'scheduled') => void;
  invalidate: () => Promise<void>;
}) {
  const [uploadTitle, setUploadTitle] = useState(document.title);
  const [uploadFile, setUploadFile] = useState<File | null>(null);
  const [uploadIntent, setUploadIntent] = useState<UploadIntent | null>(null);
  const [uploadPending, setUploadPending] = useState(false);
  const [uploadError, setUploadError] = useState<unknown>(null);
  const [uploadSuccess, setUploadSuccess] = useState(false);
  const [uploadValidation, setUploadValidation] = useState('');
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
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
  const revisionsPair = chooseRevisionPair(revisions, undefined, undefined);

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

  async function submitUpload(event?: React.FormEvent<HTMLFormElement>) {
    event?.preventDefault();
    if (!uploadFile || !uploadTitle.trim()) return;
    if (uploadFile.size > 256 * 1024 * 1024) {
      setUploadValidation('1ファイルあたり256 MiB以下のファイルを選択してください。');
      return;
    }
    setUploadValidation('');
    const intent = uploadIntent ?? {
      operationId: createOperationId(),
      targetVersionId: createOperationId(),
      fileId: createOperationId(),
      partId: createOperationId(),
      expectedRevision: document.revision,
      title: uploadTitle.trim(),
      file: uploadFile,
    };
    setUploadIntent(intent);
    setUploadError(null);
    setUploadPending(true);
    setUploadSuccess(false);
    try {
      await documentApi.createVersion(documentId, {
        operationId: intent.operationId,
        targetVersionId: intent.targetVersionId,
        expectedRevision: intent.expectedRevision,
        title: intent.title,
        items: [{
          logicalPath: intent.file.name,
          ordinal: 0,
          fileId: intent.fileId,
          partId: intent.partId,
          mediaType: intent.file.type || 'application/octet-stream',
          originalFilename: intent.file.name,
        }],
      }, new Map([[intent.partId, intent.file]]));
      setUploadSuccess(true);
      setUploadFile(null);
      setUploadIntent(null);
      await invalidate();
    } catch (error) {
      setUploadError(error);
    } finally {
      setUploadPending(false);
    }
  }

  async function confirmAction() {
    if (!selectedVersion) return;
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

  function selectUploadFile(file: File | null) {
    setUploadFile(file);
    setUploadIntent(null);
    setUploadError(null);
    setUploadValidation(file && file.size > 256 * 1024 * 1024 ? '1ファイルあたり256 MiB以下のファイルを選択してください。' : '');
  }

  function openPublication(method: 'now' | 'scheduled') {
    if (!selectedVersion) return;
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
    if (!publicationConfirmed || (publicationMethod === 'scheduled' && !jstDateTimeLocalToUtc(scheduledAt))) return;
    actionTriggerRef.current = event.currentTarget;
    setAction(publicationMethod === 'now' ? 'publish' : 'schedule');
    setActionError(null);
  }

  return (
    <div className={workflow ? styles.workflowContent : styles.versionLayout}>
      {workflow === 'newVersion' ? (
        <form className={styles.newVersionWorkspace} onSubmit={(event) => void submitUpload(event)} aria-busy={uploadPending}>
          <label className={workspaceStyles.formField}>文書名
            <input value={uploadTitle} onChange={(event) => { setUploadTitle(event.target.value); setUploadIntent(null); setUploadError(null); }} required maxLength={500} />
          </label>
          <div className={styles.workflowFacts}>
            <span>基準版</span>
            <strong>{versions.find((version) => version.isCurrent)?.versionNo ? `Version ${versions.find((version) => version.isCurrent)?.versionNo}` : document.displayVersion.lifecycleState === 'PUBLISHED' ? `Version ${document.displayVersion.versionNo}` : '公開済みの基準版はありません'}</strong>
            <span>新しい版</span>
            <strong>作業版として作成</strong>
          </div>
          <section className={styles.fileSelection} aria-labelledby="new-version-file-heading">
            <h2 id="new-version-file-heading">ファイル</h2>
            <label
              className={styles.fileDrop}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => { event.preventDefault(); selectUploadFile(event.dataTransfer.files.item(0)); }}
            >
              <span>ファイルをドロップ</span>
              <span>または選択</span>
              <input
                key={uploadSuccess ? 'upload-cleared' : 'upload-ready'}
                ref={uploadInputRef}
                aria-label="原本ファイル"
                type="file"
                onChange={(event) => selectUploadFile(event.target.files?.item(0) ?? null)}
                required
              />
            </label>
            {uploadFile && <div className={styles.selectedFile}><strong>{uploadFile.name}</strong><span>{formatBytes(uploadFile.size)} · 作成対象</span></div>}
            {uploadValidation && <p className={styles.statusWarning} role="alert">{uploadValidation}</p>}
            <p className={styles.muted}>基準版を保持したまま、新しい作業版を作成します。アップロード中の進捗率は表示しません。</p>
            {Boolean(uploadError) && <ApiFeedback error={uploadError} onRetry={() => void submitUpload()} />}
            {uploadSuccess && <p role="status" className={styles.noticeSuccess}>新しい版を作成しました。</p>}
          </section>
          <div className={styles.workflowFooter}>
            <button type="button" onClick={() => updateSearch({ workflow: undefined })}>版の一覧へ戻る</button>
            <button className={workspaceStyles.primaryButton} type="submit" disabled={uploadPending || !uploadFile || Boolean(uploadValidation)}>
              {uploadPending ? 'アップロード中…' : uploadIntent ? '同じ内容で再試行' : '新しい版を作成'}
            </button>
          </div>
        </form>
      ) : workflow === 'publication' ? (
        <section className={styles.publicationWorkspace} aria-busy={actionPending}>
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
            <button className={workspaceStyles.primaryButton} type="button" disabled={!publicationConfirmed || actionPending || (publicationMethod === 'now' ? versionDetail?.capabilities.publish.status !== 'available' : versionDetail?.capabilities.schedulePublication.status !== 'available' || !jstDateTimeLocalToUtc(scheduledAt))} onClick={requestPublicationConfirmation}>
              {publicationMethod === 'now' ? '公開する' : '公開を予約する'}
            </button>
          </div>
        </section>
      ) : (
        <>
          <section className={styles.contentSection}>
            <div className={styles.sectionHeading}>
              <div><h2>コンテンツ版</h2><p>WORKING版と正式に公開した改訂を分けて表示します。</p></div>
              {document.capabilities.createVersion.status === 'available' && <button className={workspaceStyles.primaryButton} type="button" onClick={() => updateSearch({ workflow: 'newVersion' })}>新しい版を作成</button>}
            </div>
            {versionsLoading && <LoadingState label="版と改訂を読み込み中" />}
            {Boolean(versionsError) && <ApiFeedback error={versionsError} onRetry={onRetry} />}
            {!versionsLoading && versions.length === 0 && <p className={styles.muted}>表示できるコンテンツ版はありません。</p>}
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
            {selectedVersion && (
              <div className={styles.selectedVersionActions}>
                <h3>選択中: {selectedVersion.lifecycleState === 'working' ? 'WORKING · ' : ''}版 {selectedVersion.versionNo}</h3>
                {versionDetail?.capabilities.publish.status === 'available' && <button type="button" onClick={() => openPublication('now')}>公開する</button>}
                {versionDetail?.capabilities.schedulePublication.status === 'available' && <button type="button" onClick={() => openPublication('scheduled')}>予約公開する</button>}
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
            {revisions.length === 0 && !versionsLoading && <p className={styles.muted}>正式改訂はありません。WORKING版は上の版一覧に表示されます。</p>}
            <ol className={styles.revisionTimeline}>
              {revisions.map((revision) => (
                <li key={revision.revisionId}>
                  <strong>{revision.label}</strong>
                  <span>{revision.sourceKind === 'metadataRevision' ? 'メタデータ改訂' : revision.sourceKind === 'withdrawFallback' ? '取下げ後の復帰' : '公開改訂'}</span>
                  <time dateTime={revision.createdAt}>{formatDate(revision.createdAt)}</time>
                  {revision.metadataSnapshotStatus === 'unavailableLegacy' && <span className={styles.statusWarning}>過去のメタデータは確認できません</span>}
                </li>
              ))}
            </ol>
            {revisions.length >= 2 && (
              <div className={styles.comparisonChooser}>
                <p>比較する正式改訂</p>
                <label>基準
                  <select value={revisionsPair.base?.revisionId ?? ''} onChange={(event) => updateSearch({ baseRevisionId: event.target.value || undefined })}>
                    {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
                  </select>
                </label>
                <label>対象
                  <select value={revisionsPair.target?.revisionId ?? ''} onChange={(event) => updateSearch({ targetRevisionId: event.target.value || undefined })}>
                    {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
                  </select>
                </label>
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
              <button type="button" disabled={actionPending || (action === 'schedule' && !jstDateTimeLocalToUtc(scheduledAt))} onClick={() => void confirmAction()}>{actionPending ? '処理中…' : actionError ? '同じ内容で再試行' : '確定する'}</button>
              <button type="button" autoFocus disabled={actionPending} onClick={() => { setAction(null); setActionError(null); }}>キャンセル</button>
            </div>
          </Dialog>
        </Modal>
      )}
    </div>
  );
}

type UploadIntent = {
  operationId: string;
  targetVersionId: string;
  fileId: string;
  partId: string;
  expectedRevision: number;
  title: string;
  file: File;
};

function CompareTab({ documentId, purpose, revisions, pair, comparison, loading, error, onRetry, updateSearch }: {
  documentId: string;
  purpose: 'published' | 'authoring';
  revisions: DocumentRevisionSummary[];
  pair: { base?: DocumentRevisionSummary; target?: DocumentRevisionSummary };
  comparison?: RevisionComparisonResponse;
  loading: boolean;
  error: unknown;
  onRetry: () => void;
  updateSearch: (patch: Partial<DetailSearch>) => void;
}) {
  return (
    <div className={styles.compareLayout}>
      <div className={styles.sectionHeading}>
        <div><h2>正式改訂の比較</h2><p>比較範囲と判定はDocument Diffの応答をそのまま表示します。</p></div>
      </div>
      {revisions.length < 2 && <p className={styles.muted}>比較には2件以上の正式改訂が必要です。WORKING版は正式改訂と別に扱います。</p>}
      {revisions.length >= 2 && (
        <div className={styles.compareInputs}>
          <label>基準改訂
            <select value={pair.base?.revisionId ?? ''} onChange={(event) => updateSearch({ baseRevisionId: event.target.value || undefined })}>
              {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
            </select>
          </label>
          <span aria-hidden="true">→</span>
          <label>比較対象
            <select value={pair.target?.revisionId ?? ''} onChange={(event) => updateSearch({ targetRevisionId: event.target.value || undefined })}>
              {revisions.map((revision) => <option key={revision.revisionId} value={revision.revisionId}>{revision.label}</option>)}
            </select>
          </label>
        </div>
      )}
      {loading && <LoadingState label="比較結果を取得中" />}
      {Boolean(error) && <ApiFeedback error={error} onRetry={onRetry} />}
      {comparison && <ComparisonResult documentId={documentId} purpose={purpose} comparison={comparison} />}
    </div>
  );
}

function ComparisonResult({ documentId, purpose, comparison }: { documentId: string; purpose: 'published' | 'authoring'; comparison: RevisionComparisonResponse }) {
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
          {comparison.displayItems.length === 0 && <p className={styles.muted}>表示できる差分はありません。判定は上記の比較範囲を確認してください。</p>}
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
          <h3 id="unverified-heading">未比較範囲 · {comparison.unverifiedRegions.length}</h3>
          <p>この範囲の意味は比較できていません。原本をダウンロードして目視で確認してください。</p>
          <ul>
            {comparison.unverifiedRegions.map((region, index) => (
              <li key={`unverified-${index}`}>
                <span>{unverifiedReason(region.reason)}</span>
                {region.navigationHint && <span>{region.navigationHint}</span>}
                {region.base && <DownloadSourceButton documentId={documentId} purpose={purpose} versionId={region.base.versionId} evidence={region.base} label="基準原本を確認" />}
                {region.target && <DownloadSourceButton documentId={documentId} purpose={purpose} versionId={region.target.versionId} evidence={region.target} label="対象原本を確認" />}
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

function FragmentView({ label, fragment, locator }: { label: string; fragment: DisplayFragment | null; locator?: SourceLocator | null }) {
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

function DownloadSourceButton({ documentId, purpose, versionId, evidence, label }: {
  documentId: string;
  purpose: 'published' | 'authoring';
  versionId: string;
  evidence: { contentItemId: string; representationId: string };
  label: string;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  async function download() {
    setPending(true);
    setError(null);
    try {
      const blob = await documentApi.downloadVersionFile({ documentId, versionId, contentItemId: evidence.contentItemId, representationId: evidence.representationId, purpose });
      saveBlob(blob, `原本-${versionId}`);
    } catch (caught) {
      setError(caught);
    } finally {
      setPending(false);
    }
  }
  return <span className={styles.originalAction}><button type="button" disabled={pending} onClick={() => void download()}>{pending ? '取得中…' : label}</button>{Boolean(error) && <ApiFeedback error={error} />}</span>;
}

function HistoryTab({ query }: { query: { data?: History; isPending: boolean; error: unknown; refetch: () => Promise<unknown> } }) {
  return (
    <section>
      <div className={styles.sectionHeading}><div><h2>変更履歴</h2><p>記録された操作と由来を表示します。</p></div></div>
      {query.isPending && <LoadingState label="履歴を読み込み中" />}
      {Boolean(query.error) && <ApiFeedback error={query.error} onRetry={() => void query.refetch()} />}
      {query.data?.items.length === 0 && <p className={styles.muted}>表示できる履歴はありません。</p>}
      <ol className={styles.historyList}>
        {query.data?.items.map((entry, index) => (
          <li key={`${entry.sourceKind}:${entry.sourceKey}:${index}`}>
            <div className={styles.historyTitle}><strong>{entry.actionCode}</strong><span>{entry.provenanceQuality === 'operationLedger' ? '操作記録' : entry.provenanceQuality === 'versionFallback' ? '版からの履歴' : '由来不明の履歴'}</span></div>
            <time>{entry.occurredAt ? formatDate(entry.occurredAt) : '日時不明'}</time>
            <p>{entry.actor?.presentation.displayName ?? entry.actor?.principalId ?? '実行者不明'}{entry.actor?.presentation.resolution === 'notFound' ? ' · ディレクトリに存在しません' : entry.actor?.presentation.resolution === 'unavailable' ? ' · 表示情報を取得できません' : ''}</p>
          </li>
        ))}
      </ol>
    </section>
  );
}

const policyActions = ['read', 'readHistory', 'write', 'publish', 'administer'] as const;
const policyActionLabels: Record<typeof policyActions[number], string> = {
  read: '閲覧',
  readHistory: '履歴閲覧',
  write: '編集',
  publish: '公開',
  administer: 'アクセス管理',
};

function EffectiveGrantTable({ grants }: { grants: AccessPolicyRead['effectiveGrants'] }) {
  return (
    <table className={styles.grantMatrix}>
      <thead><tr><th scope="col">対象</th>{policyActions.map((action) => <th key={action} scope="col">{policyActionLabels[action]}</th>)}</tr></thead>
      <tbody>
        {grants.map((grant) => {
          const label = grant.presentation.displayName ?? grant.subjectId;
          return (
            <tr key={`${grant.identityProvider}:${grant.subjectKind}:${grant.subjectId}`}>
              <th scope="row"><strong>{label}</strong><small>{grant.subjectKind} · {grant.identityProvider}</small></th>
              {policyActions.map((action) => <td key={action}>{grant.actions.includes(action) ? '許可' : '—'}</td>)}
            </tr>
          );
        })}
        {grants.length === 0 && <tr><td colSpan={policyActions.length + 1}>有効な主体はありません。</td></tr>}
      </tbody>
    </table>
  );
}

function AccessTab({ documentId, documentTitle, documentFolderId, folderName, policy, loading, error, onRetry }: { documentId: string; documentTitle: string; documentFolderId: string | null; folderName: string | null; policy?: AccessPolicyRead; loading: boolean; error: unknown; onRetry: () => void }) {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<'inherit' | 'explicit'>('inherit');
  const [draft, setDraft] = useState<PolicyGrantInput[]>([]);
  const [reason, setReason] = useState('');
  const [operationId, setOperationId] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [mutationError, setMutationError] = useState<unknown>(null);
  const [success, setSuccess] = useState(false);

  useEffect(() => {
    if (!policy) return;
    setMode(policy.bindingMode);
    setDraft(policy.effectiveGrants.map((grant) => ({
      subjectKind: grant.subjectKind,
      identityProvider: grant.identityProvider,
      subjectId: grant.subjectId,
      actions: [...grant.actions],
    })));
    setOperationId(null);
  }, [policy]);

  function changeDraft(update: (current: PolicyGrantInput[]) => PolicyGrantInput[]) {
    setDraft(update);
    setOperationId(null);
    setMutationError(null);
    setSuccess(false);
  }

  function toggleAction(index: number, action: typeof policyActions[number], checked: boolean) {
    changeDraft((current) => current.map((grant, grantIndex) => {
      if (grantIndex !== index) return grant;
      const actions = new Set(grant.actions);
      if (checked) actions.add(action);
      else actions.delete(action);
      return { ...grant, actions: [...actions] };
    }).filter((grant) => grant.actions.length > 0));
  }

  async function save(event?: React.FormEvent<HTMLFormElement>) {
    event?.preventDefault();
    if (!policy || !reason.trim()) return;
    const id = operationId ?? createOperationId();
    setOperationId(id);
    setPending(true);
    setMutationError(null);
    try {
      const body: CommandsPolicyExplicit | CommandsPolicyInherit = mode === 'inherit'
        ? { operationId: id, expectedPolicyRevision: policy.policyRevision, reason: reason.trim(), mode: 'inherit' }
        : { operationId: id, expectedPolicyRevision: policy.policyRevision, reason: reason.trim(), mode: 'explicit', grants: draft };
      await documentApi.setDocumentAccessPolicy(documentId, body);
      setSuccess(true);
      setOperationId(null);
      setReason('');
      await queryClient.invalidateQueries({ queryKey: ['document-access-policy', documentId] });
    } catch (caught) {
      setMutationError(caught);
    } finally {
      setPending(false);
    }
  }

  return (
    <section>
      <div className={styles.sectionHeading}><div><h2>アクセス設定</h2><p>既存の主体に付与された権限を表示します。</p></div></div>
      {loading && <LoadingState label="アクセス設定を読み込み中" />}
      {Boolean(error) && <ApiFeedback error={error} onRetry={onRetry} />}
      {policy && (
        <>
          <dl className={styles.resultFacts}>
            <dt>設定方式</dt><dd>{policy.bindingMode === 'inherit' ? '上位フォルダーから継承' : 'この文書に明示'}</dd>
            <dt>適用元</dt><dd>{policy.effectiveSource.kind === 'folder' ? (policy.effectiveSource.id === documentFolderId && folderName ? folderName : '上位フォルダー') : documentTitle}</dd>
          </dl>
          <fieldset className={styles.policyModes} disabled={pending}>
            <legend>設定方法</legend>
            <label><input type="radio" name="policy-mode" value="inherit" checked={mode === 'inherit'} onChange={() => { setMode('inherit'); setOperationId(null); setMutationError(null); setSuccess(false); }} />親フォルダーのアクセス権を継承</label>
            <p>{policy.effectiveSource.kind === 'folder' ? (policy.effectiveSource.id === documentFolderId && folderName ? `${folderName} から継承しています。` : '上位フォルダーから継承しています。') : '文書固有の設定です。'}</p>
            <label><input type="radio" name="policy-mode" value="explicit" checked={mode === 'explicit'} onChange={() => { setMode('explicit'); setOperationId(null); setMutationError(null); setSuccess(false); }} />この文書だけに個別設定</label>
            <p>対象と閲覧・編集・履歴・公開・管理の権限を設定します。</p>
          </fieldset>
          <section className={styles.effectivePolicy} aria-labelledby="effective-policy-heading">
            <h3 id="effective-policy-heading">現在有効なアクセス権</h3>
            <EffectiveGrantTable grants={policy.effectiveGrants} />
          </section>
          {policy.bindingMode === 'inherit' && mode === 'inherit' && (
            <div className={styles.inheritedNotice}>
              <p>継承された設定は読み取り専用です。個別設定へ切り替えると、表示中の有効権限を変更案として編集できます。</p>
            </div>
          )}
          {mode === 'explicit' && (
            <form className={styles.policyForm} onSubmit={(event) => void save(event)}>
              <p className={styles.muted}>新しいユーザーやグループは追加できません。既存の有効権限だけを編集します。</p>
              {draft.length === 0 && <p className={styles.statusWarning}>編集できる主体がありません。Identity directoryから主体を選べるAPIが用意されるまで、明示設定は保存できません。</p>}
              <ul className={styles.grantList}>
                {draft.map((grant, index) => {
                  const resolved = policy.effectiveGrants.find((candidate) => candidate.subjectId === grant.subjectId && candidate.identityProvider === grant.identityProvider);
                  const presentation = resolved?.presentation;
                  return (
                    <li key={`${grant.identityProvider}:${grant.subjectKind}:${grant.subjectId}`}>
                      <div className={styles.grantHeading}>
                        <strong>{presentation?.displayName ?? grant.subjectId}</strong>
                        <span>{grant.subjectKind} · {grant.identityProvider}</span>
                      </div>
                      {presentation?.secondaryText && <small>{presentation.secondaryText}</small>}
                      {presentation?.resolution === 'notFound' && <small>ディレクトリに存在しません</small>}
                      {presentation?.resolution === 'unavailable' && <small>表示名を取得できません</small>}
                      <div className={styles.actionChecks}>
                        {policyActions.map((action) => <label key={action}><input type="checkbox" checked={grant.actions.includes(action)} onChange={(event) => toggleAction(index, action, event.target.checked)} />{policyActionLabels[action]}</label>)}
                      </div>
                    </li>
                  );
                })}
              </ul>
              <label className={workspaceStyles.formField}>変更理由
                <textarea value={reason} onChange={(event) => { setReason(event.target.value); setOperationId(null); }} required minLength={1} rows={3} />
              </label>
              {Boolean(mutationError) && <ApiFeedback error={mutationError} onRetry={() => void save()} />}
              {success && <p role="status" className={styles.noticeSuccess}>アクセス設定を保存しました。</p>}
              <div className={styles.actionRow}>
                <button type="submit" disabled={pending || !reason.trim() || draft.length === 0}>{pending ? '保存中…' : mutationError ? '同じ内容で再試行' : 'アクセス設定を保存'}</button>
                <button type="button" onClick={() => { setMode(policy.bindingMode); setDraft(policy.effectiveGrants.map((grant) => ({ subjectKind: grant.subjectKind, identityProvider: grant.identityProvider, subjectId: grant.subjectId, actions: [...grant.actions] }))); setOperationId(null); setMutationError(null); }}>変更を破棄</button>
              </div>
            </form>
          )}
          {mode === 'inherit' && policy.bindingMode === 'explicit' && (
            <form className={styles.policyForm} onSubmit={(event) => void save(event)}>
              <p>保存すると、この文書の明示設定を解除し、上位フォルダーのアクセス設定を適用します。</p>
              <label className={workspaceStyles.formField}>変更理由
                <textarea value={reason} onChange={(event) => { setReason(event.target.value); setOperationId(null); }} required minLength={1} rows={3} />
              </label>
              {Boolean(mutationError) && <ApiFeedback error={mutationError} onRetry={() => void save()} />}
              {success && <p role="status" className={styles.noticeSuccess}>アクセス設定を保存しました。</p>}
              <div className={styles.actionRow}>
                <button type="submit" disabled={pending || !reason.trim()}>{pending ? '保存中…' : mutationError ? '同じ内容で再試行' : 'アクセス設定を保存'}</button>
                <button type="button" onClick={() => { setMode('explicit'); setOperationId(null); setMutationError(null); }}>キャンセル</button>
              </div>
            </form>
          )}
        </>
      )}
    </section>
  );
}

function CapabilityButton({ label, availability, onClick }: { label: string; availability: DocumentDetail['capabilities']['createVersion']; onClick: () => void }) {
  if (availability.status === 'available') return <button type="button" onClick={onClick}>{label}</button>;
  return <p className={styles.muted}>{label}: {availabilityReason(availability.reason)}</p>;
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
    const selected = versions.find((version) => version.versionId === selectedId);
    if (selected) return selected;
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
  const target = revisions.find((revision) => revision.revisionId === targetId) ?? revisions[0];
  const base = revisions.find((revision) => revision.revisionId === baseId) ?? revisions.find((revision) => revision.revisionId !== target?.revisionId) ?? revisions[1];
  return { base, target };
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

function availabilityReason(reason: string): string {
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

function operationLabel(operation: 'added' | 'removed' | 'modified' | null) {
  return operation === 'added' ? '追加' : operation === 'removed' ? '削除' : operation === 'modified' ? '変更' : '差分';
}

function unverifiedReason(reason: string) {
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

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium', timeStyle: 'short', timeZone: 'Asia/Tokyo' }).format(date);
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
