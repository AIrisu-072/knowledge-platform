use std::pin::Pin;

use document_domain::{
    ContentHash, Document, DocumentId, DocumentVersion, DocumentVersionId, FileId, FileObject,
    FileSize, LogicalPath, MediaType, PrincipalRef, StorageKey, StoredFileDescriptor, VersionFile,
};
use document_semantic_inspection_core::{InspectionProfileVersion, WorkerRequest, WorkerResponse};
use time::OffsetDateTime;
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::{
    AuditEventRecord, DomainEventRecord, InspectionExecutionError, RepositoryError,
    SemanticInspectionRecord, StorageError,
    command::{PublishDocumentCommand, PublishDocumentResult, PublishOperationId},
    publication_end::{
        EndDocumentPublicationResult, EndPublicationCandidate, EndPublicationOperationRecord,
        EndPublicationRecord, PublicationEndOperationId,
    },
    schedule::{
        CancelOperationRecord, CancelScheduleRecord, CancelScheduleResult, DueTerminalRecord,
        ScheduleOperationRecord, SchedulePublishRecord, SchedulePublishResult,
    },
    versioning_command::{
        VersionMutationRecord, VersionOperationId, VersionOperationRecord, VersionOperationResult,
        WithdrawOperationRecord, WithdrawVersionRecord, WithdrawVersionResult,
    },
};

pub type ContentReader = Pin<Box<dyn AsyncRead + Send + Unpin>>;

pub trait IdGenerator: Send + Sync {
    fn next_uuid_v7(&self) -> Uuid;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

pub struct StoreFileRequest {
    file_id: FileId,
    content: ContentReader,
    media_type: MediaType,
}

impl StoreFileRequest {
    pub fn new(file_id: FileId, content: ContentReader, media_type: MediaType) -> Self {
        Self {
            file_id,
            content,
            media_type,
        }
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    pub fn into_parts(self) -> (FileId, ContentReader, MediaType) {
        (self.file_id, self.content, self.media_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    storage_key: StorageKey,
    content_hash: ContentHash,
    size_bytes: FileSize,
    media_type: MediaType,
}

impl StoredFile {
    pub fn new(
        storage_key: StorageKey,
        content_hash: ContentHash,
        size_bytes: FileSize,
        media_type: MediaType,
    ) -> Self {
        Self {
            storage_key,
            content_hash,
            size_bytes,
            media_type,
        }
    }

    pub fn storage_key(&self) -> &StorageKey {
        &self.storage_key
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    pub const fn size_bytes(&self) -> FileSize {
        self.size_bytes
    }

    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    pub fn into_descriptor(self) -> StoredFileDescriptor {
        StoredFileDescriptor::new(
            self.storage_key,
            self.content_hash,
            self.size_bytes,
            self.media_type,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageObjectKind {
    Staging,
    Final,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageObjectInfo {
    relative_key: String,
    kind: StorageObjectKind,
    file_id: Option<FileId>,
    modified_at: OffsetDateTime,
}

impl StorageObjectInfo {
    pub fn new(
        relative_key: impl Into<String>,
        kind: StorageObjectKind,
        file_id: Option<FileId>,
        modified_at: OffsetDateTime,
    ) -> Self {
        Self {
            relative_key: relative_key.into(),
            kind,
            file_id,
            modified_at,
        }
    }

    pub fn relative_key(&self) -> &str {
        &self.relative_key
    }

    pub const fn kind(&self) -> StorageObjectKind {
        self.kind
    }

    pub const fn file_id(&self) -> Option<FileId> {
        self.file_id
    }

    pub const fn modified_at(&self) -> OffsetDateTime {
        self.modified_at
    }
}

#[allow(async_fn_in_trait)]
pub trait FileStorage: Send + Sync {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError>;
    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError>;
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError>;
}

#[allow(async_fn_in_trait)]
pub trait SemanticInspectionRepository: Send + Sync {
    async fn get_file_object(&self, file_id: FileId)
    -> Result<Option<FileObject>, RepositoryError>;
    async fn get_semantic_inspection(
        &self,
        file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> Result<Option<SemanticInspectionRecord>, RepositoryError>;
    async fn insert_or_converge_semantic_inspection(
        &self,
        record: SemanticInspectionRecord,
    ) -> Result<SemanticInspectionRecord, RepositoryError>;
}

#[allow(async_fn_in_trait)]
pub trait VersioningRepository: Send + Sync {
    /// Register an immutable FileObject before a Version can reference it.
    /// Replaying the same FileId is valid only for the same raw binding.
    async fn register_file_object(&self, file: FileObject) -> Result<(), RepositoryError>;

    async fn get_version_operation(
        &self,
        _operation_id: VersionOperationId,
    ) -> Result<Option<VersionOperationRecord>, RepositoryError> {
        Err(RepositoryError::Internal(
            "version mutation repository unavailable".to_owned(),
        ))
    }

    async fn get_version_snapshot(
        &self,
        _document_id: DocumentId,
        _version_id: DocumentVersionId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Err(RepositoryError::Internal(
            "version mutation repository unavailable".to_owned(),
        ))
    }

    async fn create_version(
        &self,
        _record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        Err(RepositoryError::Internal(
            "version mutation repository unavailable".to_owned(),
        ))
    }

    async fn update_working(
        &self,
        _record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        Err(RepositoryError::Internal(
            "version mutation repository unavailable".to_owned(),
        ))
    }

    async fn rebase_working(
        &self,
        _record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        Err(RepositoryError::Internal(
            "version mutation repository unavailable".to_owned(),
        ))
    }

    async fn get_withdraw_operation(
        &self,
        _operation_id: VersionOperationId,
    ) -> Result<Option<WithdrawOperationRecord>, RepositoryError> {
        Err(RepositoryError::Internal(
            "withdrawal repository unavailable".to_owned(),
        ))
    }

    async fn withdraw_version(
        &self,
        _record: WithdrawVersionRecord,
    ) -> Result<WithdrawVersionResult, RepositoryError> {
        Err(RepositoryError::Internal(
            "withdrawal repository unavailable".to_owned(),
        ))
    }
}

#[allow(async_fn_in_trait)]
pub trait PublicationScheduleRepository: Send + Sync {
    async fn get_schedule(
        &self,
        id: crate::PublishOperationId,
    ) -> Result<Option<ScheduleOperationRecord>, RepositoryError>;
    async fn reserve(
        &self,
        record: SchedulePublishRecord,
    ) -> Result<SchedulePublishResult, RepositoryError>;
    async fn get_cancel_operation(
        &self,
        id: VersionOperationId,
    ) -> Result<Option<CancelOperationRecord>, RepositoryError>;
    async fn cancel(
        &self,
        record: CancelScheduleRecord,
    ) -> Result<CancelScheduleResult, RepositoryError>;

    async fn database_now(&self) -> Result<OffsetDateTime, RepositoryError>;
    async fn list_due(
        &self,
        database_now: OffsetDateTime,
        limit: i64,
    ) -> Result<Vec<crate::PublishOperationId>, RepositoryError>;
    async fn is_due(&self, id: crate::PublishOperationId) -> Result<bool, RepositoryError>;
    async fn record_retry(
        &self,
        id: crate::PublishOperationId,
    ) -> Result<OffsetDateTime, RepositoryError>;
    async fn terminalize(&self, record: DueTerminalRecord) -> Result<(), RepositoryError>;
}

#[allow(async_fn_in_trait)]
pub trait PublicationEndRepository: Send + Sync {
    async fn get_end_operation(
        &self,
        operation_id: PublicationEndOperationId,
    ) -> Result<Option<EndPublicationOperationRecord>, RepositoryError>;

    async fn get_end_candidate(
        &self,
        document_id: DocumentId,
    ) -> Result<Option<EndPublicationCandidate>, RepositoryError>;

    async fn end_document_publication(
        &self,
        record: EndPublicationRecord,
    ) -> Result<EndDocumentPublicationResult, RepositoryError>;
}

#[allow(async_fn_in_trait)]
pub trait SemanticInspectionExecutor: Send + Sync {
    async fn inspect(
        &self,
        request: WorkerRequest,
        content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeContentItem {
    logical_path: LogicalPath,
    ordinal: u32,
    file: FileObject,
    original_filename: String,
}

impl AuthoritativeContentItem {
    pub fn new(
        logical_path: LogicalPath,
        ordinal: u32,
        file: FileObject,
        original_filename: impl Into<String>,
    ) -> Self {
        Self {
            logical_path,
            ordinal,
            file,
            original_filename: original_filename.into(),
        }
    }

    pub fn logical_path(&self) -> &LogicalPath {
        &self.logical_path
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub const fn file(&self) -> &FileObject {
        &self.file
    }

    pub fn original_filename(&self) -> &str {
        &self.original_filename
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeDocument {
    document: Document,
    version: DocumentVersion,
    file: FileObject,
    version_file: VersionFile,
    content_items: Vec<AuthoritativeContentItem>,
    requires_content_classification: bool,
}

impl AuthoritativeDocument {
    pub fn from_initial(initial: document_domain::InitialDocument) -> Self {
        let (document, version, file, version_file) = initial.into_parts();
        Self {
            content_items: vec![AuthoritativeContentItem::new(
                LogicalPath::new("primary").expect("constant path is valid"),
                0,
                file.clone(),
                version_file.original_filename(),
            )],
            document,
            version,
            file,
            version_file,
            requires_content_classification: false,
        }
    }

    pub fn from_parts(
        document: Document,
        version: DocumentVersion,
        file: FileObject,
        version_file: VersionFile,
    ) -> Self {
        Self {
            content_items: vec![AuthoritativeContentItem::new(
                LogicalPath::new("primary").expect("constant path is valid"),
                0,
                file.clone(),
                version_file.original_filename(),
            )],
            document,
            version,
            file,
            version_file,
            requires_content_classification: false,
        }
    }

    pub fn from_parts_with_items(
        document: Document,
        version: DocumentVersion,
        file: FileObject,
        version_file: VersionFile,
        content_items: Vec<AuthoritativeContentItem>,
        requires_content_classification: bool,
    ) -> Self {
        Self {
            document,
            version,
            file,
            version_file,
            content_items,
            requires_content_classification,
        }
    }

    pub const fn document(&self) -> &Document {
        &self.document
    }

    pub const fn version(&self) -> &DocumentVersion {
        &self.version
    }

    pub const fn file(&self) -> &FileObject {
        &self.file
    }

    pub const fn version_file(&self) -> &VersionFile {
        &self.version_file
    }

    pub fn content_items(&self) -> &[AuthoritativeContentItem] {
        &self.content_items
    }

    pub const fn requires_content_classification(&self) -> bool {
        self.requires_content_classification
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateInitialDocumentRecord {
    authoritative: AuthoritativeDocument,
    domain_events: Vec<DomainEventRecord>,
    audit_events: Vec<AuditEventRecord>,
}

impl CreateInitialDocumentRecord {
    pub fn new(
        authoritative: AuthoritativeDocument,
        domain_events: Vec<DomainEventRecord>,
        audit_events: Vec<AuditEventRecord>,
    ) -> Self {
        Self {
            authoritative,
            domain_events,
            audit_events,
        }
    }

    pub const fn authoritative(&self) -> &AuthoritativeDocument {
        &self.authoritative
    }

    pub fn domain_events(&self) -> &[DomainEventRecord] {
        &self.domain_events
    }

    pub fn audit_events(&self) -> &[AuditEventRecord] {
        &self.audit_events
    }

    pub fn into_parts(
        self,
    ) -> (
        AuthoritativeDocument,
        Vec<DomainEventRecord>,
        Vec<AuditEventRecord>,
    ) {
        (self.authoritative, self.domain_events, self.audit_events)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCommandIdentity {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
}

impl PublishCommandIdentity {
    pub fn from_command(command: &PublishDocumentCommand) -> Self {
        Self {
            publish_operation_id: command.publish_operation_id(),
            document_id: command.document_id(),
            target_document_version_id: command.target_document_version_id(),
            expected_document_revision: command.expected_document_revision(),
            principal: command.principal().clone(),
        }
    }

    pub fn from_persisted(
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        target_document_version_id: DocumentVersionId,
        expected_document_revision: i64,
        principal: PrincipalRef,
    ) -> Self {
        Self {
            publish_operation_id,
            document_id,
            target_document_version_id,
            expected_document_revision,
            principal,
        }
    }

    pub const fn publish_operation_id(&self) -> PublishOperationId {
        self.publish_operation_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn target_document_version_id(&self) -> DocumentVersionId {
        self.target_document_version_id
    }

    pub const fn expected_document_revision(&self) -> i64 {
        self.expected_document_revision
    }

    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOperationRecord {
    identity: PublishCommandIdentity,
    result: PublishDocumentResult,
}

impl PublishOperationRecord {
    pub fn new(identity: PublishCommandIdentity, result: PublishDocumentResult) -> Self {
        Self { identity, result }
    }

    pub fn matches_identity(&self, identity: &PublishCommandIdentity) -> bool {
        self.identity.eq(identity)
    }

    pub const fn identity(&self) -> &PublishCommandIdentity {
        &self.identity
    }

    pub const fn result(&self) -> &PublishDocumentResult {
        &self.result
    }

    pub fn into_parts(self) -> (PublishCommandIdentity, PublishDocumentResult) {
        (self.identity, self.result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCandidate {
    document: Document,
    version: DocumentVersion,
    file: FileObject,
    version_file: VersionFile,
}

impl PublishCandidate {
    pub fn new(
        document: Document,
        version: DocumentVersion,
        file: FileObject,
        version_file: VersionFile,
    ) -> Self {
        Self {
            document,
            version,
            file,
            version_file,
        }
    }

    pub const fn document(&self) -> &Document {
        &self.document
    }

    pub const fn version(&self) -> &DocumentVersion {
        &self.version
    }

    pub const fn file(&self) -> &FileObject {
        &self.file
    }

    pub const fn version_file(&self) -> &VersionFile {
        &self.version_file
    }

    pub fn into_parts(self) -> (Document, DocumentVersion, FileObject, VersionFile) {
        (self.document, self.version, self.file, self.version_file)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishInitialVersionRecord {
    operation: PublishOperationRecord,
    domain_event: DomainEventRecord,
    audit_event: AuditEventRecord,
    scheduled_due: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishVersionRecord {
    operation: PublishOperationRecord,
    domain_event: DomainEventRecord,
    audit_event: AuditEventRecord,
    base_version_id: DocumentVersionId,
    base_manifest_digest: [u8; 32],
    target_manifest_digest: [u8; 32],
    scheduled_due: bool,
}

impl PublishVersionRecord {
    pub fn new(
        operation: PublishOperationRecord,
        domain_event: DomainEventRecord,
        audit_event: AuditEventRecord,
        base_version_id: DocumentVersionId,
        base_manifest_digest: [u8; 32],
        target_manifest_digest: [u8; 32],
    ) -> Self {
        Self {
            operation,
            domain_event,
            audit_event,
            base_version_id,
            base_manifest_digest,
            target_manifest_digest,
            scheduled_due: false,
        }
    }

    pub fn for_due(mut self) -> Self {
        self.scheduled_due = true;
        self
    }
    pub const fn scheduled_due(&self) -> bool {
        self.scheduled_due
    }
    pub fn into_parts(
        self,
    ) -> (
        PublishOperationRecord,
        DomainEventRecord,
        AuditEventRecord,
        DocumentVersionId,
        [u8; 32],
        [u8; 32],
    ) {
        (
            self.operation,
            self.domain_event,
            self.audit_event,
            self.base_version_id,
            self.base_manifest_digest,
            self.target_manifest_digest,
        )
    }
}

impl PublishInitialVersionRecord {
    pub fn new(
        operation: PublishOperationRecord,
        domain_event: DomainEventRecord,
        audit_event: AuditEventRecord,
    ) -> Self {
        Self {
            operation,
            domain_event,
            audit_event,
            scheduled_due: false,
        }
    }

    pub fn for_due(mut self) -> Self {
        self.scheduled_due = true;
        self
    }
    pub const fn scheduled_due(&self) -> bool {
        self.scheduled_due
    }

    pub const fn operation(&self) -> &PublishOperationRecord {
        &self.operation
    }

    pub const fn domain_event(&self) -> &DomainEventRecord {
        &self.domain_event
    }

    pub const fn audit_event(&self) -> &AuditEventRecord {
        &self.audit_event
    }

    pub fn into_parts(self) -> (PublishOperationRecord, DomainEventRecord, AuditEventRecord) {
        (self.operation, self.domain_event, self.audit_event)
    }
}

#[allow(async_fn_in_trait)]
pub trait DocumentPublishRepository: Send + Sync {
    async fn get_publish_operation(
        &self,
        operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError>;

    async fn get_publish_candidate(
        &self,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError>;

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError>;

    async fn publish_next_version(
        &self,
        _record: PublishVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        Err(RepositoryError::Internal(
            "replacement publish repository unavailable".to_owned(),
        ))
    }
}

#[allow(async_fn_in_trait)]
pub trait DocumentRepository: Send + Sync {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError>;

    async fn get_authoritative_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError>;

    async fn get_authoring_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError>;

    async fn get_current_published_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError>;

    async fn is_current_published_version(
        &self,
        document_id: DocumentId,
        version_id: DocumentVersionId,
    ) -> Result<bool, RepositoryError>;

    async fn list_current_published_versions(
        &self,
        after: Option<DocumentId>,
        limit: i64,
    ) -> Result<Vec<CurrentPublishedVersionRef>, RepositoryError>;

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError>;

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentPublishedVersionRef {
    document_id: DocumentId,
    current_version_id: DocumentVersionId,
    document_revision: i64,
}

impl CurrentPublishedVersionRef {
    pub fn new(
        document_id: DocumentId,
        current_version_id: DocumentVersionId,
        document_revision: i64,
    ) -> Self {
        Self {
            document_id,
            current_version_id,
            document_revision,
        }
    }

    pub const fn document_id(self) -> DocumentId {
        self.document_id
    }

    pub const fn current_version_id(self) -> DocumentVersionId {
        self.current_version_id
    }

    pub const fn document_revision(self) -> i64 {
        self.document_revision
    }
}
