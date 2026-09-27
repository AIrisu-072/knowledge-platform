use document_domain::{AuditEventId, DocumentId, DocumentVersionId, EventId, PrincipalRef};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset};

use crate::{ApplicationError, PublishOperationId, VersionOperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePublishCommand {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
    scheduled_publish_at: OffsetDateTime,
}
impl SchedulePublishCommand {
    pub fn new(
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        expected_revision: i64,
        actor: PrincipalRef,
        scheduled_publish_at: OffsetDateTime,
    ) -> Result<Self, ApplicationError> {
        if expected_revision < 0 || scheduled_publish_at.offset() != UtcOffset::UTC {
            return Err(ApplicationError::Validation(
                "schedule requires nonnegative revision and UTC due time".to_owned(),
            ));
        }
        Ok(Self {
            publish_operation_id,
            document_id,
            target_version_id,
            expected_revision,
            actor,
            scheduled_publish_at,
        })
    }
    pub const fn publish_operation_id(&self) -> PublishOperationId {
        self.publish_operation_id
    }
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
    pub const fn target_version_id(&self) -> DocumentVersionId {
        self.target_version_id
    }
    pub const fn expected_revision(&self) -> i64 {
        self.expected_revision
    }
    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }
    pub const fn scheduled_publish_at(&self) -> OffsetDateTime {
        self.scheduled_publish_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePublishResult {
    pub publish_operation_id: PublishOperationId,
    pub document_id: DocumentId,
    pub target_version_id: DocumentVersionId,
    pub accepted_revision: i64,
    pub scheduled_publish_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleOperationRecord {
    pub command: SchedulePublishCommand,
    pub result: SchedulePublishResult,
    pub manifest_digest: [u8; 32],
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePublishRecord {
    pub command: SchedulePublishCommand,
    pub expected_current_version_id: Option<DocumentVersionId>,
    pub manifest_digest: [u8; 32],
    pub domain_event_id: EventId,
    pub audit_event_id: AuditEventId,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelScheduleCommand {
    operation_id: VersionOperationId,
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
}
impl CancelScheduleCommand {
    pub fn new(
        operation_id: VersionOperationId,
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        expected_revision: i64,
        actor: PrincipalRef,
    ) -> Result<Self, ApplicationError> {
        if expected_revision < 0 {
            return Err(ApplicationError::Validation(
                "expected revision cannot be negative".to_owned(),
            ));
        }
        Ok(Self {
            operation_id,
            publish_operation_id,
            document_id,
            target_version_id,
            expected_revision,
            actor,
        })
    }
    pub const fn operation_id(&self) -> VersionOperationId {
        self.operation_id
    }
    pub const fn publish_operation_id(&self) -> PublishOperationId {
        self.publish_operation_id
    }
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
    pub const fn target_version_id(&self) -> DocumentVersionId {
        self.target_version_id
    }
    pub const fn expected_revision(&self) -> i64 {
        self.expected_revision
    }
    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }
    pub fn command_digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"document-version-cancel-schedule-v0\0");
        hash.update(self.publish_operation_id.as_uuid().as_bytes());
        hash.update(self.document_id.as_uuid().as_bytes());
        hash.update(self.target_version_id.as_uuid().as_bytes());
        hash.update(self.expected_revision.to_be_bytes());
        for field in [self.actor.identity_provider(), self.actor.principal_id()] {
            hash.update((field.len() as u32).to_be_bytes());
            hash.update(field.as_bytes());
        }
        hash.finalize().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelScheduleResult {
    pub operation_id: VersionOperationId,
    pub publish_operation_id: PublishOperationId,
    pub document_id: DocumentId,
    pub target_version_id: DocumentVersionId,
    pub resulting_revision: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelOperationRecord {
    pub command_digest: [u8; 32],
    pub result: CancelScheduleResult,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelScheduleRecord {
    pub command: CancelScheduleCommand,
    pub domain_event_id: EventId,
    pub audit_event_id: AuditEventId,
    pub occurred_at: OffsetDateTime,
}
