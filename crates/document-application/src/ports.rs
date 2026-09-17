use std::pin::Pin;

use document_domain::{
    ContentHash, Document, DocumentId, DocumentVersion, FileId, FileObject, FileSize, MediaType,
    PrincipalRef, StorageKey, StoredFileDescriptor, VersionFile,
};
use time::OffsetDateTime;
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::{
    AuditEventRecord, DomainEventRecord, RepositoryError, StorageError,
    command::{
        PublishDocumentCommand, PublishDocumentResult, PublishOperationId,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeDocument {
    document: Document,
    version: DocumentVersion,
    file: FileObject,
    version_file: VersionFile,
}

impl AuthoritativeDocument {
    pub fn from_initial(initial: document_domain::InitialDocument) -> Self {
        let (document, version, file, version_file) = initial.into_parts();
        Self {
            document,
            version,
            file,
            version_file,
        }
    }

    pub fn from_parts(
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
        }
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

    pub fn into_parts(
        self,
    ) -> (PublishOperationRecord, DomainEventRecord, AuditEventRecord) {
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

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError>;

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError>;
}
