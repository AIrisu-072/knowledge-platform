use std::sync::Arc;

use document_domain::{
    AuditEventId, Document, DocumentId, DocumentVersion, DocumentVersionId, DomainError, EventId,
    LifecycleState, PrincipalRef,
};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::{ApplicationError, Clock, IdGenerator, PublicationEndRepository, RepositoryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicationEndOperationId(Uuid);

impl PublicationEndOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() == 7 {
            Ok(Self(value))
        } else {
            Err(ApplicationError::Validation(
                "publication end operation id must be UUIDv7".to_owned(),
            ))
        }
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndDocumentPublicationCommand {
    operation_id: PublicationEndOperationId,
    document_id: DocumentId,
    expected_revision: i64,
    expected_current_version_id: DocumentVersionId,
    actor: PrincipalRef,
    reason: String,
    command_digest: [u8; 32],
}

impl EndDocumentPublicationCommand {
    pub fn new(
        operation_id: PublicationEndOperationId,
        document_id: DocumentId,
        expected_revision: i64,
        expected_current_version_id: DocumentVersionId,
        actor: PrincipalRef,
        reason: String,
    ) -> Result<Self, ApplicationError> {
        if expected_revision < 0 {
            return Err(ApplicationError::Validation(
                "expected document revision cannot be negative".to_owned(),
            ));
        }
        if reason.trim().is_empty() {
            return Err(ApplicationError::Validation(
                "publication end reason cannot be blank".to_owned(),
            ));
        }
        if [
            actor.identity_provider(),
            actor.principal_id(),
            reason.as_str(),
        ]
        .iter()
        .any(|value| u32::try_from(value.len()).is_err())
        {
            return Err(ApplicationError::Validation(
                "publication end identity or reason is too long".to_owned(),
            ));
        }
        let command_digest = digest_command(
            operation_id,
            document_id,
            expected_revision,
            expected_current_version_id,
            &actor,
            &reason,
        );
        Ok(Self {
            operation_id,
            document_id,
            expected_revision,
            expected_current_version_id,
            actor,
            reason,
            command_digest,
        })
    }

    pub const fn operation_id(&self) -> PublicationEndOperationId {
        self.operation_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn expected_revision(&self) -> i64 {
        self.expected_revision
    }

    pub const fn expected_current_version_id(&self) -> DocumentVersionId {
        self.expected_current_version_id
    }

    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub const fn command_digest(&self) -> [u8; 32] {
        self.command_digest
    }
}

fn digest_command(
    operation_id: PublicationEndOperationId,
    document_id: DocumentId,
    expected_revision: i64,
    expected_current_version_id: DocumentVersionId,
    actor: &PrincipalRef,
    reason: &str,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"document-publication-end-v0\0");
    digest.update(operation_id.as_uuid().as_bytes());
    digest.update(document_id.as_uuid().as_bytes());
    digest.update(expected_current_version_id.as_uuid().as_bytes());
    digest.update(expected_revision.to_be_bytes());
    for value in [actor.identity_provider(), actor.principal_id(), reason] {
        let size = u32::try_from(value.len()).expect("command constructor checked text length");
        digest.update(size.to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.finalize().into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndDocumentPublicationResult {
    operation_id: PublicationEndOperationId,
    document_id: DocumentId,
    former_current_version_id: DocumentVersionId,
    resulting_document_revision: i64,
    ended_at: OffsetDateTime,
}

impl EndDocumentPublicationResult {
    pub fn from_persisted(
        operation_id: PublicationEndOperationId,
        document_id: DocumentId,
        former_current_version_id: DocumentVersionId,
        resulting_document_revision: i64,
        ended_at: OffsetDateTime,
    ) -> Self {
        Self {
            operation_id,
            document_id,
            former_current_version_id,
            resulting_document_revision,
            ended_at: ended_at.to_offset(UtcOffset::UTC),
        }
    }

    pub const fn operation_id(&self) -> PublicationEndOperationId {
        self.operation_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn former_current_version_id(&self) -> DocumentVersionId {
        self.former_current_version_id
    }

    pub const fn resulting_current_version_id(&self) -> Option<DocumentVersionId> {
        None
    }

    pub const fn resulting_document_revision(&self) -> i64 {
        self.resulting_document_revision
    }

    pub const fn ended_at(&self) -> OffsetDateTime {
        self.ended_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndPublicationCandidate {
    document: Document,
    current: Option<DocumentVersion>,
}

impl EndPublicationCandidate {
    pub fn new(document: Document, current: Option<DocumentVersion>) -> Self {
        Self { document, current }
    }

    pub const fn document(&self) -> &Document {
        &self.document
    }

    pub const fn current(&self) -> Option<&DocumentVersion> {
        self.current.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndPublicationOperationRecord {
    command_digest: [u8; 32],
    result: EndDocumentPublicationResult,
}

impl EndPublicationOperationRecord {
    pub fn new(command_digest: [u8; 32], result: EndDocumentPublicationResult) -> Self {
        Self {
            command_digest,
            result,
        }
    }

    pub const fn command_digest(&self) -> [u8; 32] {
        self.command_digest
    }

    pub const fn result(&self) -> &EndDocumentPublicationResult {
        &self.result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndPublicationRecord {
    command: EndDocumentPublicationCommand,
    ended_at: OffsetDateTime,
    domain_event_id: EventId,
    audit_event_id: AuditEventId,
}

impl EndPublicationRecord {
    pub fn new(
        command: EndDocumentPublicationCommand,
        ended_at: OffsetDateTime,
        domain_event_id: EventId,
        audit_event_id: AuditEventId,
    ) -> Self {
        Self {
            command,
            ended_at: ended_at.to_offset(UtcOffset::UTC),
            domain_event_id,
            audit_event_id,
        }
    }

    pub const fn command(&self) -> &EndDocumentPublicationCommand {
        &self.command
    }

    pub const fn ended_at(&self) -> OffsetDateTime {
        self.ended_at
    }

    pub const fn domain_event_id(&self) -> EventId {
        self.domain_event_id
    }

    pub const fn audit_event_id(&self) -> AuditEventId {
        self.audit_event_id
    }
}

pub struct DocumentPublicationEndService<I, C, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    repository: Arc<R>,
}

impl<I, C, R> DocumentPublicationEndService<I, C, R>
where
    I: IdGenerator,
    C: Clock,
    R: PublicationEndRepository,
{
    pub fn new(ids: Arc<I>, clock: Arc<C>, repository: Arc<R>) -> Self {
        Self {
            ids,
            clock,
            repository,
        }
    }

    pub async fn end_document_publication(
        &self,
        command: EndDocumentPublicationCommand,
    ) -> Result<EndDocumentPublicationResult, ApplicationError> {
        if let Some(result) = self.replay(&command).await? {
            return Ok(result);
        }

        let candidate = match self
            .repository
            .get_end_candidate(command.document_id())
            .await
        {
            Ok(Some(candidate)) => candidate,
            Ok(None) => return Err(ApplicationError::DocumentNotFound),
            Err(RepositoryError::Conflict) => {
                return self.recheck_or(&command, ApplicationError::Conflict).await;
            }
            Err(RepositoryError::BusinessRule) => {
                return self
                    .recheck_or(&command, ApplicationError::BusinessRule)
                    .await;
            }
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = validate_candidate(&candidate, &command) {
            return self.recheck_or(&command, error).await;
        }
        let record = EndPublicationRecord::new(
            command.clone(),
            self.clock.now(),
            EventId::from_uuid(self.ids.next_uuid_v7()),
            AuditEventId::from_uuid(self.ids.next_uuid_v7()),
        );
        match self.repository.end_document_publication(record).await {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::PublicationEndCommitOutcomeUnknown {
                    operation_id: command.operation_id(),
                    document_id: command.document_id(),
                })
            }
            Err(RepositoryError::Conflict) => {
                self.recheck_or(&command, ApplicationError::Conflict).await
            }
            Err(RepositoryError::BusinessRule) => {
                self.recheck_or(&command, ApplicationError::BusinessRule)
                    .await
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn replay(
        &self,
        command: &EndDocumentPublicationCommand,
    ) -> Result<Option<EndDocumentPublicationResult>, ApplicationError> {
        match self
            .repository
            .get_end_operation(command.operation_id())
            .await?
        {
            Some(stored) if stored.command_digest() == command.command_digest() => {
                Ok(Some(stored.result().clone()))
            }
            Some(_) => Err(ApplicationError::OperationConflict),
            None => Ok(None),
        }
    }

    async fn recheck_or(
        &self,
        command: &EndDocumentPublicationCommand,
        error: ApplicationError,
    ) -> Result<EndDocumentPublicationResult, ApplicationError> {
        self.replay(command).await?.ok_or(error)
    }
}

fn validate_candidate(
    candidate: &EndPublicationCandidate,
    command: &EndDocumentPublicationCommand,
) -> Result<(), ApplicationError> {
    if candidate.document().revision() != command.expected_revision() {
        return Err(ApplicationError::Conflict);
    }
    let Some(current_id) = candidate.document().current_version_id() else {
        return Err(ApplicationError::BusinessRule);
    };
    if current_id != command.expected_current_version_id() {
        return Err(ApplicationError::Conflict);
    }
    let current = candidate
        .current()
        .ok_or(ApplicationError::IntegrityViolation)?;
    if current.document_id() != command.document_id() {
        return Err(ApplicationError::IntegrityViolation);
    }
    if current.lifecycle_state() != LifecycleState::Published {
        return Err(ApplicationError::BusinessRule);
    }
    let mut document = candidate.document().clone();
    document
        .end_publication(current)
        .map_err(map_domain_error)?;
    Ok(())
}

fn map_domain_error(error: DomainError) -> ApplicationError {
    match error {
        DomainError::NoCurrentPublishedVersion | DomainError::VersionNotPublished => {
            ApplicationError::BusinessRule
        }
        DomainError::StaleVersionBase => ApplicationError::Conflict,
        DomainError::VersionDocumentMismatch | DomainError::RevisionOverflow => {
            ApplicationError::IntegrityViolation
        }
        other => ApplicationError::Validation(other.to_string()),
    }
}
