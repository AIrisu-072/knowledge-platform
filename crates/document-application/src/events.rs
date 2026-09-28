use document_domain::{AuditEventId, DocumentId, DocumentVersionId, EventId, PrincipalRef};
use serde_json::Value;
use time::OffsetDateTime;

pub const DOCUMENT_CREATED: &str = "DocumentCreated";
pub const DOCUMENT_VERSION_CREATED: &str = "DocumentVersionCreated";
pub const DOCUMENT_VERSION_PUBLISHED: &str = "DocumentVersionPublished";
pub const AUDIT_DOCUMENT_CREATED: &str = "document.created";
pub const AUDIT_DOCUMENT_VERSION_CREATED: &str = "document.version.created";
pub const AUDIT_DOCUMENT_VERSION_PUBLISHED: &str = "document.version.published";
pub const DOCUMENT_VERSION_UPDATED: &str = "DocumentVersionUpdated";
pub const DOCUMENT_VERSION_REBASED: &str = "DocumentVersionRebased";
pub const AUDIT_DOCUMENT_VERSION_UPDATED: &str = "document.version.updated";
pub const AUDIT_DOCUMENT_VERSION_REBASED: &str = "document.version.rebased";
pub const DOCUMENT_VERSION_WITHDRAWN: &str = "DocumentVersionWithdrawn";
pub const AUDIT_DOCUMENT_VERSION_WITHDRAWN: &str = "document.version.withdrawn";
pub const DOCUMENT_VERSION_PUBLICATION_SCHEDULED: &str = "DocumentVersionPublicationScheduled";
pub const AUDIT_DOCUMENT_VERSION_PUBLICATION_SCHEDULED: &str =
    "document.version.publication.scheduled";
pub const DOCUMENT_VERSION_PUBLICATION_CANCELLED: &str = "DocumentVersionPublicationCancelled";
pub const AUDIT_DOCUMENT_VERSION_PUBLICATION_CANCELLED: &str =
    "document.version.publication.cancelled";
pub const DOCUMENT_PUBLICATION_ENDED: &str = "DocumentPublicationEnded";
pub const AUDIT_DOCUMENT_PUBLICATION_ENDED: &str = "document.publication.ended";

#[derive(Debug, Clone, PartialEq)]
pub struct DomainEventRecord {
    event_id: EventId,
    event_type: &'static str,
    aggregate_type: &'static str,
    aggregate_id: DocumentId,
    payload: Value,
    occurred_at: OffsetDateTime,
}

impl DomainEventRecord {
    pub(crate) fn new(
        event_id: EventId,
        event_type: &'static str,
        aggregate_id: DocumentId,
        payload: Value,
        occurred_at: OffsetDateTime,
    ) -> Self {
        Self {
            event_id,
            event_type,
            aggregate_type: "Document",
            aggregate_id,
            payload,
            occurred_at,
        }
    }

    pub const fn event_id(&self) -> EventId {
        self.event_id
    }

    pub const fn event_type(&self) -> &'static str {
        self.event_type
    }

    pub const fn aggregate_type(&self) -> &'static str {
        self.aggregate_type
    }

    pub const fn aggregate_id(&self) -> DocumentId {
        self.aggregate_id
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }

    pub const fn occurred_at(&self) -> OffsetDateTime {
        self.occurred_at
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEventRecord {
    event_id: AuditEventId,
    event_type: &'static str,
    actor: PrincipalRef,
    resource_id: DocumentId,
    resource_version_id: Option<DocumentVersionId>,
    result: &'static str,
    data: Value,
    occurred_at: OffsetDateTime,
}

impl AuditEventRecord {
    pub(crate) fn new(
        event_id: AuditEventId,
        event_type: &'static str,
        actor: PrincipalRef,
        resource_id: DocumentId,
        resource_version_id: Option<DocumentVersionId>,
        data: Value,
        occurred_at: OffsetDateTime,
    ) -> Self {
        Self {
            event_id,
            event_type,
            actor,
            resource_id,
            resource_version_id,
            result: "success",
            data,
            occurred_at,
        }
    }

    pub const fn event_id(&self) -> AuditEventId {
        self.event_id
    }

    pub const fn event_type(&self) -> &'static str {
        self.event_type
    }

    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }

    pub const fn resource_id(&self) -> DocumentId {
        self.resource_id
    }

    pub const fn resource_version_id(&self) -> Option<DocumentVersionId> {
        self.resource_version_id
    }

    pub const fn result(&self) -> &'static str {
        self.result
    }

    pub const fn data(&self) -> &Value {
        &self.data
    }

    pub const fn occurred_at(&self) -> OffsetDateTime {
        self.occurred_at
    }
}
