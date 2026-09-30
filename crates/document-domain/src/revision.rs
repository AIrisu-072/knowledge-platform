use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DocumentId, DocumentRevisionId, DocumentVersionId, DomainError, Metadata, PrincipalRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentRevisionNumber {
    major_no: i64,
    minor_no: i64,
}

impl DocumentRevisionNumber {
    pub fn new(major_no: i64, minor_no: i64) -> Result<Self, DomainError> {
        if major_no < 1 || minor_no < 0 {
            return Err(DomainError::InvalidDocumentRevisionNumber);
        }
        Ok(Self { major_no, minor_no })
    }

    pub const fn major_no(self) -> i64 {
        self.major_no
    }

    pub const fn minor_no(self) -> i64 {
        self.minor_no
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMetadataSnapshot {
    document_type: Option<String>,
    owning_department: Option<String>,
    category: Option<String>,
    extensions: Option<Map<String, Value>>,
}

impl DocumentMetadataSnapshot {
    pub fn from_metadata(metadata: &Metadata) -> Result<Self, DomainError> {
        let values = metadata.as_map();
        let optional_string = |key: &str| match values.get(key) {
            None => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(DomainError::InvalidDocumentMetadataSnapshot),
        };
        let extensions = match values.get("extensions") {
            None => None,
            Some(Value::Object(value)) => Some(value.clone()),
            Some(_) => return Err(DomainError::InvalidDocumentMetadataSnapshot),
        };

        Ok(Self {
            document_type: optional_string("document_type")?,
            owning_department: optional_string("owning_department")?,
            category: optional_string("category")?,
            extensions,
        })
    }

    pub fn document_type(&self) -> Option<&str> {
        self.document_type.as_deref()
    }

    pub fn owning_department(&self) -> Option<&str> {
        self.owning_department.as_deref()
    }

    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    pub fn extensions(&self) -> Option<&Map<String, Value>> {
        self.extensions.as_ref()
    }

    pub fn as_json(&self) -> Value {
        let extensions = self
            .extensions
            .clone()
            .map(Value::Object)
            .unwrap_or(Value::Null);
        json!({
            "document_type": self.document_type,
            "owning_department": self.owning_department,
            "category": self.category,
            "extensions": extensions,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentRevisionMetadataStatus {
    Complete,
    UnavailableLegacy,
}

impl DocumentRevisionMetadataStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::UnavailableLegacy => "unavailable_legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentRevisionSourceKind {
    InitialPublication,
    ContentPublication,
    MetadataRevision,
    WithdrawFallback,
    LegacyBackfill,
}

impl DocumentRevisionSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InitialPublication => "initialPublication",
            Self::ContentPublication => "contentPublication",
            Self::MetadataRevision => "metadataRevision",
            Self::WithdrawFallback => "withdrawFallback",
            Self::LegacyBackfill => "legacyBackfill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRevision {
    revision_id: DocumentRevisionId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    number: DocumentRevisionNumber,
    metadata_snapshot: Option<DocumentMetadataSnapshot>,
    metadata_snapshot_status: DocumentRevisionMetadataStatus,
    source_kind: DocumentRevisionSourceKind,
    operation_id: Option<Uuid>,
    created_at: OffsetDateTime,
    actor: Option<PrincipalRef>,
    reason: Option<String>,
}

impl DocumentRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision_id: DocumentRevisionId,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
        number: DocumentRevisionNumber,
        metadata_snapshot: Option<DocumentMetadataSnapshot>,
        metadata_snapshot_status: DocumentRevisionMetadataStatus,
        source_kind: DocumentRevisionSourceKind,
        operation_id: Option<Uuid>,
        created_at: OffsetDateTime,
        actor: Option<PrincipalRef>,
        reason: Option<String>,
    ) -> Result<Self, DomainError> {
        let is_legacy = source_kind == DocumentRevisionSourceKind::LegacyBackfill;
        let snapshot_valid = matches!(
            (metadata_snapshot_status, metadata_snapshot.is_some()),
            (DocumentRevisionMetadataStatus::Complete, true)
                | (DocumentRevisionMetadataStatus::UnavailableLegacy, false)
        );
        let provenance_valid = if is_legacy {
            operation_id.is_none() && actor.is_none() && reason.is_none()
        } else {
            operation_id.is_some() && actor.is_some()
        };
        if !snapshot_valid
            || !provenance_valid
            || (metadata_snapshot_status == DocumentRevisionMetadataStatus::UnavailableLegacy
                && !is_legacy)
        {
            return Err(DomainError::InvalidDocumentMetadataSnapshot);
        }

        Ok(Self {
            revision_id,
            document_id,
            document_version_id,
            number,
            metadata_snapshot,
            metadata_snapshot_status,
            source_kind,
            operation_id,
            created_at,
            actor,
            reason,
        })
    }

    pub const fn revision_id(&self) -> DocumentRevisionId {
        self.revision_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn document_version_id(&self) -> DocumentVersionId {
        self.document_version_id
    }

    pub const fn number(&self) -> DocumentRevisionNumber {
        self.number
    }

    pub fn metadata_snapshot(&self) -> Option<&DocumentMetadataSnapshot> {
        self.metadata_snapshot.as_ref()
    }

    pub const fn metadata_snapshot_status(&self) -> DocumentRevisionMetadataStatus {
        self.metadata_snapshot_status
    }

    pub const fn source_kind(&self) -> DocumentRevisionSourceKind {
        self.source_kind
    }

    pub const fn operation_id(&self) -> Option<Uuid> {
        self.operation_id
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub fn actor(&self) -> Option<&PrincipalRef> {
        self.actor.as_ref()
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}
