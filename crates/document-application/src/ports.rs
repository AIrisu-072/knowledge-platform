use std::pin::Pin;

use document_domain::{
    ContentHash, Document, DocumentId, DocumentVersion, FileId, FileObject, FileSize, MediaType,
    StorageKey, StoredFileDescriptor, VersionFile,
};
use time::OffsetDateTime;
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::{AuditEventRecord, DomainEventRecord, RepositoryError, StorageError};

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
}
