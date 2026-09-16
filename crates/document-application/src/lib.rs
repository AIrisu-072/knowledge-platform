#![forbid(unsafe_code)]

//! Application services and ports for the authoritative document core.

mod command;
mod error;
mod events;
mod ports;
mod reconciliation;
mod service;

pub use command::{CreateDocumentCommand, CreateDocumentResult};
pub use error::{ApplicationError, RepositoryError, StorageError};
pub use events::{
    AUDIT_DOCUMENT_CREATED, AUDIT_DOCUMENT_VERSION_CREATED, AuditEventRecord, DOCUMENT_CREATED,
    DOCUMENT_VERSION_CREATED, DomainEventRecord,
};
pub use ports::{
    AuthoritativeDocument, Clock, ContentReader, CreateInitialDocumentRecord, DocumentRepository,
    FileStorage, IdGenerator, StorageObjectInfo, StorageObjectKind, StoreFileRequest, StoredFile,
};
pub use reconciliation::{ReconciliationClassification, classify};
pub use service::DocumentService;
