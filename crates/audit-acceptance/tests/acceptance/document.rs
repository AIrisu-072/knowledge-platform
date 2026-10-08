//! The Document side: the production application services over the
//! production PostgreSQL repository and file-system storage, with
//! in-process substitutes only where production needs worker binaries
//! (semantic inspection, diff) or an identity provider.
//!
//! Every audit row comes from a production producer: nothing here writes
//! `public.audit_outbox_events`.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use document_application::document_diff::{
    DiffExecutionError, DiffExecutor, DiffRequest, DocumentDiffService,
};
use document_application::{
    AccessPolicyService, ApplicationError, AuthorizedDocumentService, BootstrapRootPolicy, Clock,
    ContentReader, CreateDocumentCommand, CreateVersionCommand, CurrentReadStateService,
    DocumentHistoryService, DocumentManagementService, DocumentRevisionPageQuery,
    DocumentRevisionReadService, DocumentVersionService, DueExecutionOutcome,
    EndDocumentPublicationCommand, EnsureSemanticInspection, FolderService, IdGenerator,
    IdentityContextResolver, IdentityResolutionError, InspectionExecutionError, InvocationKind,
    ManagementCommand, ManagementOperationId, MarkVersionRead, PublicationEndOperationId,
    PublicationScheduleRepository, PublishDocumentCommand, PublishOperationId, ReadStateMutation,
    ReadStateMutationKind, ReadStateOperationId, ReadStateService, RevisionComparisonService,
    SchedulePublishCommand, SemanticInspectionExecutor, VerifiedActorContext,
    VersionFileAccessService, VersionFileRequest, VersionOperationId, VersionPurpose,
    VersionRequest, VersioningItemInput, VersioningPreflight, WithdrawVersionCommand,
};
use document_application::{
    CancelScheduleCommand, PreparedManifest, RebaseWorkingVersionCommand,
    UpdateWorkingVersionCommand,
};
use document_diff_core::{DiffCoverage, DiffProfileVersion, WorkerDiffRequest, WorkerDiffResponse};
use document_domain::{
    Action, DocumentId, DocumentVersionId, FileId, FolderId, LogicalPath, MediaType, Metadata,
    PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget, PrincipalRef, Title,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use document_semantic_inspection_core::{WorkerRequest, WorkerResponse};
use document_storage_fs::FileSystemStorage;
use serde_json::json;
use sqlx::PgPool;
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

/// Synthetic identities (no real person, organization or IdP).
pub const IDP: &str = "test-idp";
pub const EDITOR: &str = "editor";
pub const READER: &str = "reader";
/// The group through which the reader holds Read.
pub const READERS_GROUP: &str = "synthetic-readers";
/// A group that only ever appears in an access-control list. Its name must
/// never reach the Store (no ACL list in the audit envelopes).
pub const ACL_ONLY_GROUP: &str = "acl-only-group-5e7d0c";
/// Original filenames of a created document and of a new version: kept by
/// Document (`content_representations`), never in the Store.
pub const ORIGINAL_FILENAME: &str = "FILEMARK-original-5c2e.txt";
pub const VERSION_FILENAME: &str = "FILEMARK-version-8d41.txt";

pub fn editor() -> PrincipalRef {
    PrincipalRef::new(IDP, EDITOR).expect("principal")
}

pub fn reader() -> PrincipalRef {
    PrincipalRef::new(IDP, READER).expect("principal")
}

fn subject(kind: PolicySubjectKind, id: &str) -> PolicySubject {
    PolicySubject::new(kind, IDP, id).expect("subject")
}

/// What a trusted identity adapter verified for one of the synthetic
/// principals (human, interactive).
pub fn context(principal: &str) -> VerifiedActorContext {
    let mut subjects = vec![subject(PolicySubjectKind::Principal, principal)];
    if principal == READER {
        subjects.push(subject(PolicySubjectKind::Group, READERS_GROUP));
    }
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new(IDP, principal).expect("principal"),
        subjects,
        OffsetDateTime::now_utc() + time::Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .expect("verified context")
}

pub fn editor_ctx() -> VerifiedActorContext {
    context(EDITOR)
}

pub fn reader_ctx() -> VerifiedActorContext {
    context(READER)
}

/// The editor holds every action; the readers' group only Read.
pub fn root_grants() -> Vec<PolicyGrant> {
    vec![
        PolicyGrant::new(
            subject(PolicySubjectKind::Principal, EDITOR),
            [
                Action::Read,
                Action::ReadHistory,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ],
        )
        .expect("grant"),
        PolicyGrant::new(
            subject(PolicySubjectKind::Group, READERS_GROUP),
            [Action::Read],
        )
        .expect("grant"),
    ]
}

/// The identity directory of the scheduler: resolves the original
/// requester again at execution time (the production resolver of the PoC
/// runtime only knows the `poc` identities).
pub struct Directory;

impl IdentityContextResolver for Directory {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        match (principal.identity_provider(), principal.principal_id()) {
            (IDP, EDITOR) => Ok(editor_ctx()),
            (IDP, READER) => Ok(reader_ctx()),
            _ => Err(IdentityResolutionError::InvalidIdentity),
        }
    }
}

/// The wall clock (microsecond precision, as PostgreSQL stores it): the
/// scheduler compares it with the database clock.
pub struct UtcClock;

impl Clock for UtcClock {
    fn now(&self) -> OffsetDateTime {
        let now = OffsetDateTime::now_utc();
        now.replace_nanosecond(now.nanosecond() / 1_000 * 1_000)
            .expect("microseconds")
    }
}

pub struct V7Ids;

impl IdGenerator for V7Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// In-process semantic inspection (no worker binary, no PDFium): echoes the
/// expected raw hash and size, reports plain text, and fingerprints the
/// content by its first byte, so versions starting with different bytes
/// differ semantically.
pub struct SyntheticInspection;

impl SemanticInspectionExecutor for SyntheticInspection {
    async fn inspect(
        &self,
        request: WorkerRequest,
        mut content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        let mut bytes = Vec::new();
        content
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| InspectionExecutionError::ExtractorUnavailable)?;
        let first = *bytes
            .first()
            .ok_or(InspectionExecutionError::InvalidWorkerResult)?;
        serde_json::from_value(json!({
            "protocol_version": "dsi-worker-v0",
            "inspection_profile_version": "dsi-v0",
            "observed_raw_content_hash": request.expected_raw_content_hash,
            "observed_size_bytes": request.expected_size_bytes,
            "detected_format": "txt",
            "semantic_fingerprint": {"algorithm": "sha256", "digest": vec![first; 32]},
            "semantic_capabilities": [],
            "editorial_provenance": {"tracked_changes": [], "comments": [],
                "document_author_labels": [], "last_modified_by": null,
                "modification_metadata": {}},
            "external_dependencies": [],
            "digital_signature_evidence": [],
            "extractor_provenance": {"worker_build_id": "synthetic-acceptance",
                "adapter_id": "txt", "adapter_version": "1", "parser_libraries": [],
                "native_dependency_identity": []},
            "diagnostics": []
        }))
        .map_err(|_| InspectionExecutionError::InvalidWorkerResult)
    }
}

/// In-process diff (no worker binary): a full-coverage result bound to the
/// request, as `WorkerDiffResponse::validate_against` requires.
pub struct SyntheticDiff;

impl DiffExecutor for SyntheticDiff {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        _base: ContentReader,
        _target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        Ok(WorkerDiffResponse {
            protocol_version: request.protocol_version,
            diff_profile_version: request.diff_profile_version,
            resource_profile_version: request.resource_profile_version,
            base_raw_sha256: request.base_raw_sha256,
            base_size_bytes: request.base_size_bytes,
            target_raw_sha256: request.target_raw_sha256,
            target_size_bytes: request.target_size_bytes,
            format: request.format,
            coverage: DiffCoverage::Full,
            changes: Vec::new(),
            unverified_regions: Vec::new(),
            ancillary_changes: Vec::new(),
            parser_provenance: "synthetic-acceptance".into(),
        })
    }
}

pub type Repo = PostgresDocumentRepository;
pub type Docs =
    AuthorizedDocumentService<V7Ids, UtcClock, FileSystemStorage, SyntheticInspection, Repo>;
pub type Diff = DocumentDiffService<
    Repo,
    FileSystemStorage,
    SyntheticDiff,
    EnsureSemanticInspection<Repo, FileSystemStorage, SyntheticInspection, UtcClock>,
>;

pub fn op() -> Uuid {
    Uuid::now_v7()
}

pub fn management_op() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(op()).expect("v7")
}

/// The Document platform as the server composes it: one repository on the
/// Document runtime pool, the file-system storage, the services.
pub struct Platform {
    pub pool: PgPool,
    pub storage_root: TempDir,
    pub storage: Arc<FileSystemStorage>,
    pub repo: Arc<Repo>,
    pub docs: Docs,
    pub diff: Arc<Diff>,
}

impl Platform {
    pub fn new(pool: PgPool) -> Self {
        let storage_root = TempDir::new().expect("storage root");
        let storage = Arc::new(FileSystemStorage::new(storage_root.path()));
        let repo = Arc::new(PostgresDocumentRepository::new(pool.clone()));
        let inspection = Arc::new(SyntheticInspection);
        let docs = AuthorizedDocumentService::new(
            Arc::new(V7Ids),
            Arc::new(UtcClock),
            storage.clone(),
            inspection.clone(),
            repo.clone(),
        );
        let diff = Arc::new(DocumentDiffService::new(
            repo.clone(),
            storage.clone(),
            Arc::new(SyntheticDiff),
            Arc::new(EnsureSemanticInspection::new(
                repo.clone(),
                storage.clone(),
                inspection,
                Arc::new(UtcClock),
            )),
        ));
        Self {
            pool,
            storage_root,
            storage,
            repo,
            docs,
            diff,
        }
    }

    pub fn root() -> FolderId {
        FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID)
    }

    /// The one-time root policy (`document-server` bootstrap): a repository
    /// that trusts the editor as its bootstrap actor.
    pub async fn bootstrap(&self) {
        PostgresDocumentRepository::new_with_bootstrap_actor(self.pool.clone(), editor())
            .initialize_root_policy(&editor_ctx(), root_grants())
            .await
            .expect("root policy bootstrap");
    }

    pub async fn document_revision(&self, document: DocumentId) -> i64 {
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
            .bind(document.as_uuid())
            .fetch_one(&self.pool)
            .await
            .expect("document revision")
    }

    pub async fn folder_revision(&self, folder: FolderId) -> i64 {
        sqlx::query_scalar("SELECT revision FROM folders WHERE folder_id = $1")
            .bind(folder.as_uuid())
            .fetch_one(&self.pool)
            .await
            .expect("folder revision")
    }

    /// A new document with one plain-text original (v1, WORKING).
    pub async fn create_document(
        &self,
        folder: FolderId,
        title: &str,
        content: &[u8],
    ) -> Result<(DocumentId, DocumentVersionId), ApplicationError> {
        let created = self
            .docs
            .create_document(
                &editor_ctx(),
                CreateDocumentCommand {
                    folder_id: folder,
                    title: title.to_owned(),
                    document_metadata: Metadata::default(),
                    version_metadata: Metadata::default(),
                    principal: editor(),
                    original_filename: ORIGINAL_FILENAME.into(),
                    media_type: MediaType::new("text/plain").expect("media type"),
                    content: Box::pin(Cursor::new(content.to_vec())),
                },
            )
            .await?;
        Ok((created.document_id(), created.document_version_id()))
    }

    pub async fn publish(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        self.docs
            .publish_document(
                &editor_ctx(),
                PublishDocumentCommand::new(
                    PublishOperationId::try_from_uuid(op()).expect("v7"),
                    document,
                    version,
                    revision,
                    editor(),
                )?,
            )
            .await
            .map(|_| ())
    }

    pub async fn mark_read(
        &self,
        ctx: &VerifiedActorContext,
        document: DocumentId,
        version: DocumentVersionId,
    ) -> Result<bool, ApplicationError> {
        ReadStateService::new(self.repo.clone())
            .mark_version_read(
                ctx,
                MarkVersionRead {
                    document_id: document,
                    document_version_id: version,
                },
            )
            .await
            .map(|result| result.inserted)
    }

    /// The current read-state VIEW / RESET transitions (detail display and
    /// "mark unread").
    pub async fn read_state(
        &self,
        ctx: &VerifiedActorContext,
        document: DocumentId,
        version: DocumentVersionId,
        expected: i64,
        kind: ReadStateMutationKind,
    ) -> Result<bool, ApplicationError> {
        CurrentReadStateService::new(self.repo.clone())
            .mutate_read_state(
                ctx,
                ReadStateMutation {
                    operation_id: ReadStateOperationId::try_from_uuid(op()).expect("v7"),
                    document_id: document,
                    document_version_id: version,
                    expected_read_state_revision: expected,
                    kind,
                },
            )
            .await
            .map(|result| result.changed)
    }

    /// Opens the authoritative original of a published version, as the
    /// download endpoint does, and returns its bytes.
    pub async fn open_original(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
        correlation: Uuid,
    ) -> Vec<u8> {
        let ctx = editor_ctx();
        let request = VersionRequest {
            document_id: document,
            document_version_id: version,
            purpose: VersionPurpose::Published,
        };
        let files = DocumentHistoryService::new(self.repo.clone())
            .list_version_files(&ctx, request)
            .await
            .expect("version files");
        let original = files
            .iter()
            .find(|file| file.role == "AUTHORITATIVE")
            .expect("authoritative original");
        let mut opened = VersionFileAccessService::new(self.repo.clone(), self.storage.clone())
            .open_version_file(
                &ctx,
                VersionFileRequest {
                    version: request,
                    content_item_id: original.content_item_id,
                    representation_id: original.representation_id,
                    correlation_id: Some(correlation),
                },
            )
            .await
            .expect("open the original");
        let mut bytes = Vec::new();
        opened
            .content
            .read_to_end(&mut bytes)
            .await
            .expect("read the original");
        bytes
    }

    pub async fn update_metadata(
        &self,
        document: DocumentId,
        category: &str,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        DocumentManagementService::new(self.repo.clone())
            .update_document_metadata(
                &editor_ctx(),
                ManagementCommand::UpdateDocumentMetadata {
                    operation_id: management_op(),
                    document_id: document,
                    expected_document_revision: revision,
                    set: BTreeMap::from([("category".to_owned(), json!(category))]),
                    unset: BTreeSet::new(),
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn create_folder(
        &self,
        ctx: &VerifiedActorContext,
        folder: FolderId,
        parent: FolderId,
        name: &str,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.folder_revision(parent).await;
        FolderService::new(self.repo.clone())
            .create_folder(
                ctx,
                ManagementCommand::CreateFolder {
                    operation_id: management_op(),
                    folder_id: folder,
                    parent_folder_id: parent,
                    expected_parent_revision: revision,
                    name: name.to_owned(),
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn rename_folder(
        &self,
        folder: FolderId,
        name: &str,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.folder_revision(folder).await;
        FolderService::new(self.repo.clone())
            .rename_folder(
                &editor_ctx(),
                ManagementCommand::RenameFolder {
                    operation_id: management_op(),
                    folder_id: folder,
                    expected_folder_revision: revision,
                    name: name.to_owned(),
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn move_folder(
        &self,
        folder: FolderId,
        from: FolderId,
        to: FolderId,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.folder_revision(folder).await;
        FolderService::new(self.repo.clone())
            .move_folder(
                &editor_ctx(),
                ManagementCommand::MoveFolder {
                    operation_id: management_op(),
                    folder_id: folder,
                    from_parent_id: from,
                    to_parent_id: to,
                    expected_folder_revision: revision,
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn move_document(
        &self,
        document: DocumentId,
        from: FolderId,
        to: FolderId,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        DocumentManagementService::new(self.repo.clone())
            .move_document(
                &editor_ctx(),
                ManagementCommand::MoveDocument {
                    operation_id: management_op(),
                    document_id: document,
                    from_folder_id: from,
                    to_folder_id: to,
                    expected_document_revision: revision,
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    /// An explicit folder policy: the editor keeps every action, the
    /// readers keep Read, and an ACL-only group is added (its name must not
    /// reach the Store).
    pub async fn set_folder_policy(
        &self,
        folder: FolderId,
        expected_policy_revision: i64,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let mut grants = root_grants();
        grants.push(
            PolicyGrant::new(
                subject(PolicySubjectKind::Group, ACL_ONLY_GROUP),
                [Action::Read],
            )
            .expect("grant"),
        );
        AccessPolicyService::new(self.repo.clone())
            .set_access_policy(
                &editor_ctx(),
                ManagementCommand::SetAccessPolicy {
                    operation_id: management_op(),
                    target: PolicyTarget::Folder(folder),
                    expected_policy_revision,
                    mode: PolicyMode::Explicit(grants),
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    /// The manifest of one plain-text original, prepared the way the
    /// versioning endpoint does (preflight over a repository scoped to the
    /// verified actor).
    async fn prepare(
        &self,
        ctx: &VerifiedActorContext,
        title: &str,
        content: &[u8],
    ) -> Result<PreparedManifest, ApplicationError> {
        VersioningPreflight::new(
            Arc::new(self.repo.with_verified_actor(ctx.clone())),
            self.storage.clone(),
            Arc::new(SyntheticInspection),
            Arc::new(UtcClock),
        )
        .prepare_dsi_v0(
            Title::new(title)?,
            vec![VersioningItemInput::new(
                LogicalPath::new("primary")?,
                0,
                FileId::from_uuid(Uuid::now_v7()),
                MediaType::new("text/plain")?,
                VERSION_FILENAME,
                Box::pin(Cursor::new(content.to_vec())),
            )],
        )
        .await
    }

    /// A new WORKING version, prepared the way the versioning endpoint does
    /// (preflight over a repository scoped to the verified actor).
    pub async fn create_version(
        &self,
        document: DocumentId,
        title: &str,
        content: &[u8],
    ) -> Result<DocumentVersionId, ApplicationError> {
        let ctx = editor_ctx();
        let prepared = self.prepare(&ctx, title, content).await?;
        let target = DocumentVersionId::from_uuid(Uuid::now_v7());
        let revision = self.document_revision(document).await;
        self.docs
            .create_version(
                &ctx,
                CreateVersionCommand::new(
                    VersionOperationId::try_from_uuid(op())?,
                    document,
                    target,
                    revision,
                    editor(),
                )?,
                prepared,
            )
            .await?;
        Ok(target)
    }

    pub async fn schedule_publish(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
        due: OffsetDateTime,
    ) -> Result<PublishOperationId, ApplicationError> {
        let revision = self.document_revision(document).await;
        let operation = PublishOperationId::try_from_uuid(op())?;
        self.docs
            .schedule_publish(
                &editor_ctx(),
                SchedulePublishCommand::new(operation, document, version, revision, editor(), due)?,
            )
            .await?;
        Ok(operation)
    }

    /// The publication scheduler's poll loop (`DueScheduler::poll_once`,
    /// which needs the worker binary to be constructed) until `target` has
    /// been executed: database clock, due list, and the authorized due
    /// execution with the production scheduler attribution `service/scheduler`.
    pub async fn run_scheduler_until(
        &self,
        targets: &[PublishOperationId],
    ) -> BTreeMap<Uuid, DueExecutionOutcome> {
        let scheduler = DocumentVersionService::new(
            Arc::new(V7Ids),
            Arc::new(UtcClock),
            self.storage.clone(),
            Arc::new(SyntheticInspection),
            self.repo.clone(),
        );
        let executor = document_publication_scheduler::scheduler_executor();
        assert_eq!(
            (executor.identity_provider(), executor.principal_id()),
            ("service", "scheduler")
        );
        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(30);
        let mut outcomes = BTreeMap::new();
        loop {
            let now = self.repo.database_now().await.expect("database clock");
            let due = self.repo.list_due(now, 100).await.expect("due schedules");
            for id in due {
                let outcome = scheduler
                    .execute_due_authorized(id, &Directory, &executor)
                    .await
                    .expect("due execution");
                outcomes.insert(id.as_uuid(), outcome);
            }
            if targets
                .iter()
                .all(|target| outcomes.contains_key(&target.as_uuid()))
            {
                return outcomes;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the schedules did not become due: {outcomes:?}"
            );
            tokio::time::sleep(StdDuration::from_millis(200)).await;
        }
    }

    pub async fn cancel_schedule(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
        schedule: PublishOperationId,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        self.docs
            .cancel_schedule(
                &editor_ctx(),
                CancelScheduleCommand::new(
                    VersionOperationId::try_from_uuid(op())?,
                    schedule,
                    document,
                    version,
                    revision,
                    editor(),
                )?,
            )
            .await
            .map(|_| ())
    }

    /// Replaces the content of a WORKING version (versioning endpoint PUT).
    pub async fn update_working(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
        title: &str,
        content: &[u8],
    ) -> Result<(), ApplicationError> {
        let ctx = editor_ctx();
        let prepared = self.prepare(&ctx, title, content).await?;
        let revision = self.document_revision(document).await;
        self.docs
            .update_working_version(
                &ctx,
                UpdateWorkingVersionCommand::new(
                    VersionOperationId::try_from_uuid(op())?,
                    document,
                    version,
                    revision,
                    editor(),
                )?,
                prepared,
            )
            .await
            .map(|_| ())
    }

    /// Moves a WORKING version onto the current published version.
    pub async fn rebase(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        self.docs
            .rebase_working_version(
                &editor_ctx(),
                RebaseWorkingVersionCommand::new(
                    VersionOperationId::try_from_uuid(op())?,
                    document,
                    version,
                    revision,
                    editor(),
                )?,
            )
            .await
            .map(|_| ())
    }

    /// A document policy that withdraws the editor's Publish (every other
    /// action stays).
    pub async fn revoke_publish(
        &self,
        document: DocumentId,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let grants = vec![
            PolicyGrant::new(
                subject(PolicySubjectKind::Principal, EDITOR),
                [
                    Action::Read,
                    Action::ReadHistory,
                    Action::Write,
                    Action::Administer,
                ],
            )
            .expect("grant"),
            PolicyGrant::new(
                subject(PolicySubjectKind::Group, READERS_GROUP),
                [Action::Read],
            )
            .expect("grant"),
        ];
        AccessPolicyService::new(self.repo.clone())
            .set_access_policy(
                &editor_ctx(),
                ManagementCommand::SetAccessPolicy {
                    operation_id: management_op(),
                    target: PolicyTarget::Document(document),
                    expected_policy_revision: 0,
                    mode: PolicyMode::Explicit(grants),
                    reason: reason.to_owned(),
                },
            )
            .await
            .map(|_| ())
    }

    pub async fn compare_versions(
        &self,
        document: DocumentId,
        base: DocumentVersionId,
        target: DocumentVersionId,
    ) -> bool {
        self.diff
            .compare(
                &editor_ctx(),
                DiffRequest {
                    document_id: document,
                    base_version_id: base,
                    target_version_id: target,
                    profile: DiffProfileVersion::V0,
                },
            )
            .await
            .expect("diff")
            .cache_hit
    }

    /// The revision id of `major.minor`.
    pub async fn revision_id(&self, document: DocumentId, major: i64, minor: i64) -> Uuid {
        DocumentRevisionReadService::new(self.repo.clone())
            .list_document_revisions(
                &editor_ctx(),
                DocumentRevisionPageQuery {
                    document_id: document,
                    page_size: Some(100),
                    cursor: None,
                },
            )
            .await
            .expect("revisions")
            .items
            .iter()
            .find(|revision| revision.major_no == major && revision.minor_no == minor)
            .unwrap_or_else(|| panic!("revision {major}.{minor}"))
            .revision_id
    }

    /// Compares two revisions; `true` when the content was compared too.
    pub async fn compare_revisions(&self, document: DocumentId, base: Uuid, target: Uuid) -> bool {
        let comparison = RevisionComparisonService::new(self.repo.clone(), self.diff.clone())
            .compare(&editor_ctx(), document, base, target)
            .await
            .expect("revision comparison");
        comparison.content_audit_event_id.is_some()
    }

    pub async fn withdraw(
        &self,
        document: DocumentId,
        version: DocumentVersionId,
        reason: &str,
    ) -> Result<Option<DocumentVersionId>, ApplicationError> {
        let revision = self.document_revision(document).await;
        self.docs
            .withdraw_version(
                &editor_ctx(),
                WithdrawVersionCommand::new(
                    VersionOperationId::try_from_uuid(op())?,
                    document,
                    version,
                    revision,
                    editor(),
                    reason,
                )?,
            )
            .await
            .map(|result| result.resulting_current_version_id)
    }

    pub async fn end_publication(
        &self,
        document: DocumentId,
        current: DocumentVersionId,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        let revision = self.document_revision(document).await;
        self.docs
            .end_document_publication(
                &editor_ctx(),
                EndDocumentPublicationCommand::new(
                    PublicationEndOperationId::try_from_uuid(op())?,
                    document,
                    revision,
                    current,
                    editor(),
                    reason.to_owned(),
                )?,
            )
            .await
            .map(|_| ())
    }
}

/// A small, complete Document life cycle used where the scenario itself is
/// not under test (outage, crash, restore): create, publish, read confirm,
/// metadata change with a reason, withdrawal-free. Returns the document.
pub async fn small_lifecycle(platform: &Platform, label: &str) -> DocumentId {
    let (document, version) = platform
        .create_document(
            Platform::root(),
            &format!("Synthetic {label}"),
            format!("A synthetic body {label}\n").as_bytes(),
        )
        .await
        .expect("create");
    platform.publish(document, version).await.expect("publish");
    assert!(
        platform
            .mark_read(&editor_ctx(), document, version)
            .await
            .expect("read")
    );
    platform
        .update_metadata(document, label, &format!("synthetic reason {label}"))
        .await
        .expect("metadata");
    document
}
