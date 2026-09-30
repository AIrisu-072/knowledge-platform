use document_domain::{DocumentId, DocumentVersionId, FolderId, LifecycleState, Title};
use time::OffsetDateTime;
use uuid::Uuid;

/// Metadata fields explicitly allowed for Search; the arbitrary Document metadata map is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermittedDocumentMetadata {
    pub document_type: Option<String>,
    pub category: Option<String>,
}

/// An opaque prefilter hint, never an ACL body or a final authorization decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentAccessProjectionInput {
    pub access_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationEndRecord {
    pub operation_id: Uuid,
    pub ended_at: OffsetDateTime,
}

/// DSI capabilities and evidence pointers; no extracted text or content fingerprint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DsiEvidenceRefs {
    pub capability_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
}

/// One DocumentVersion read from one authoritative Document Source snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSourceSnapshot {
    pub source_snapshot: String,
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub current_version_id: Option<DocumentVersionId>,
    pub publication_end: Option<PublicationEndRecord>,
    pub lifecycle_state: LifecycleState,
    pub title: Title,
    pub metadata: PermittedDocumentMetadata,
    pub folder_id: FolderId,
    pub created_at: OffsetDateTime,
    pub published_at: Option<OffsetDateTime>,
    pub withdrawn_at: Option<OffsetDateTime>,
    pub effective_from: Option<OffsetDateTime>,
    pub effective_to: Option<OffsetDateTime>,
    pub access: DocumentAccessProjectionInput,
    pub dsi: Option<DsiEvidenceRefs>,
}
