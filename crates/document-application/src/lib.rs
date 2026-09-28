#![forbid(unsafe_code)]

//! Application services and ports for the authoritative document core.

mod access_context;
mod access_policy_service;
mod authorized_document;
mod command;
mod error;
mod events;
mod management_command;
mod management_digest;
mod management_ports;
mod ports;
mod publication_end;
mod publish_quality;
mod reconciliation;
mod schedule;
mod scheduled_authorization;
mod semantic_inspection;
mod service;
mod versioning_command;
mod versioning_preflight;
mod versioning_service;

pub use access_context::{IdentityResolutionError, InvocationKind, VerifiedActorContext};
pub use access_policy_service::AccessPolicyService;
pub use authorized_document::{AuthorizationScope, AuthorizedDocumentService};
pub use command::{
    CreateDocumentCommand, CreateDocumentResult, PublishDocumentCommand, PublishDocumentResult,
    PublishOperationId,
};
pub use error::{ApplicationError, InspectionExecutionError, RepositoryError, StorageError};
pub use events::{
    AUDIT_DOCUMENT_CREATED, AUDIT_DOCUMENT_PUBLICATION_ENDED, AUDIT_DOCUMENT_VERSION_CREATED,
    AUDIT_DOCUMENT_VERSION_PUBLICATION_CANCELLED, AUDIT_DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
    AUDIT_DOCUMENT_VERSION_PUBLISHED, AUDIT_DOCUMENT_VERSION_REBASED,
    AUDIT_DOCUMENT_VERSION_UPDATED, AUDIT_DOCUMENT_VERSION_WITHDRAWN, AuditEventRecord,
    DOCUMENT_CREATED, DOCUMENT_PUBLICATION_ENDED, DOCUMENT_VERSION_CREATED,
    DOCUMENT_VERSION_PUBLICATION_CANCELLED, DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
    DOCUMENT_VERSION_PUBLISHED, DOCUMENT_VERSION_REBASED, DOCUMENT_VERSION_UPDATED,
    DOCUMENT_VERSION_WITHDRAWN, DomainEventRecord,
};
pub use management_command::{
    ManagementCommand, ManagementErrorCode, ManagementMutationResult, ManagementOperationId,
    ManagementResult,
};
pub use management_digest::{
    canonical_command_bytes, canonical_json_bytes, management_command_digest,
};
pub use management_ports::{BootstrapRootPolicy, IdentityContextResolver, ManagementRepository};
pub use ports::{
    AuthoritativeContentItem, AuthoritativeDocument, Clock, ContentReader,
    CreateInitialDocumentRecord, CurrentPublishedVersionRef, DocumentPublishRepository,
    DocumentRepository, FileStorage, IdGenerator, PublicationEndRepository,
    PublicationScheduleRepository, PublishCandidate, PublishCommandIdentity,
    PublishInitialVersionRecord, PublishOperationRecord, PublishVersionRecord,
    SemanticInspectionExecutor, SemanticInspectionRepository, StorageObjectInfo, StorageObjectKind,
    StoreFileRequest, StoredFile, VersioningRepository,
};
pub use publication_end::{
    DocumentPublicationEndService, EndDocumentPublicationCommand, EndDocumentPublicationResult,
    EndPublicationCandidate, EndPublicationOperationRecord, EndPublicationRecord,
    PublicationEndOperationId,
};
pub use reconciliation::{ReconciliationClassification, ReconciliationFinding, classify};
pub use schedule::{
    CancelOperationRecord, CancelScheduleCommand, CancelScheduleRecord, CancelScheduleResult,
    DueExecutionOutcome, DueTerminalRecord, ScheduleOperationRecord, SchedulePublishCommand,
    SchedulePublishRecord, SchedulePublishResult,
};
pub use scheduled_authorization::authorize_scheduled_publish;
pub use semantic_inspection::{EnsureSemanticInspection, SemanticInspectionRecord};
pub use service::DocumentService;
pub use versioning_command::{
    CreateVersionCommand, RebaseWorkingVersionCommand, UpdateWorkingVersionCommand,
    VersionCommandIdentity, VersionMutationRecord, VersionOperationId, VersionOperationKind,
    VersionOperationRecord, VersionOperationResult, WithdrawOperationRecord,
    WithdrawVersionCommand, WithdrawVersionRecord, WithdrawVersionResult,
};
pub use versioning_preflight::{
    PreparedContentItem, PreparedManifest, PreparedRendition, VersioningItemInput,
    VersioningPreflight, VersioningRenditionInput,
};
pub use versioning_service::DocumentVersionService;
