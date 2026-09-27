#![forbid(unsafe_code)]

//! Application services and ports for the authoritative document core.

mod command;
mod error;
mod events;
mod ports;
mod publish_quality;
mod reconciliation;
mod semantic_inspection;
mod service;
mod versioning_command;
mod versioning_preflight;
mod versioning_service;

pub use command::{
    CreateDocumentCommand, CreateDocumentResult, PublishDocumentCommand, PublishDocumentResult,
    PublishOperationId,
};
pub use error::{ApplicationError, InspectionExecutionError, RepositoryError, StorageError};
pub use events::{
    AUDIT_DOCUMENT_CREATED, AUDIT_DOCUMENT_VERSION_CREATED, AUDIT_DOCUMENT_VERSION_PUBLISHED,
    AUDIT_DOCUMENT_VERSION_REBASED, AUDIT_DOCUMENT_VERSION_UPDATED, AuditEventRecord,
    DOCUMENT_CREATED, DOCUMENT_VERSION_CREATED, DOCUMENT_VERSION_PUBLISHED,
    DOCUMENT_VERSION_REBASED, DOCUMENT_VERSION_UPDATED, DomainEventRecord,
};
pub use ports::{
    AuthoritativeContentItem, AuthoritativeDocument, Clock, ContentReader,
    CreateInitialDocumentRecord, DocumentPublishRepository, DocumentRepository, FileStorage,
    IdGenerator, PublishCandidate, PublishCommandIdentity, PublishInitialVersionRecord,
    PublishOperationRecord, SemanticInspectionExecutor, SemanticInspectionRepository,
    StorageObjectInfo, StorageObjectKind, StoreFileRequest, StoredFile, VersioningRepository,
};
pub use reconciliation::{ReconciliationClassification, ReconciliationFinding, classify};
pub use semantic_inspection::{EnsureSemanticInspection, SemanticInspectionRecord};
pub use service::DocumentService;
pub use versioning_command::{
    CreateVersionCommand, RebaseWorkingVersionCommand, UpdateWorkingVersionCommand,
    VersionCommandIdentity, VersionMutationRecord, VersionOperationId, VersionOperationKind,
    VersionOperationRecord, VersionOperationResult,
};
pub use versioning_preflight::{
    PreparedContentItem, PreparedManifest, PreparedRendition, VersioningItemInput,
    VersioningPreflight, VersioningRenditionInput,
};
pub use versioning_service::DocumentVersionService;
