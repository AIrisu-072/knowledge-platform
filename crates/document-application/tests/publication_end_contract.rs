use std::sync::{Arc, Mutex};

use document_application::{
    ApplicationError, Clock, DocumentPublicationEndService, EndDocumentPublicationCommand,
    EndDocumentPublicationResult, EndPublicationCandidate, EndPublicationOperationRecord,
    EndPublicationRecord, IdGenerator, PublicationEndOperationId, PublicationEndRepository,
    RepositoryError,
};
use document_domain::{
    AuditEventId, Document, DocumentId, DocumentVersion, DocumentVersionId, EventId, FolderId,
    LifecycleState, Metadata, PrincipalRef, RestoreDocument, RestoreDocumentVersion, Title,
    VersionNo,
};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

fn operation_id() -> PublicationEndOperationId {
    PublicationEndOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap(),
    )
    .unwrap()
}

fn command(reason: &str, actor_id: &str) -> EndDocumentPublicationCommand {
    EndDocumentPublicationCommand::new(
        operation_id(),
        DocumentId::from_uuid(Uuid::from_u128(1)),
        4,
        DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        PrincipalRef::new("test-idp", actor_id).unwrap(),
        reason.to_owned(),
    )
    .unwrap()
}

#[test]
fn publication_end_timestamp_is_normalized_to_utc() {
    let local = OffsetDateTime::UNIX_EPOCH.to_offset(UtcOffset::from_hms(9, 0, 0).unwrap());
    let result = EndDocumentPublicationResult::from_persisted(
        operation_id(),
        DocumentId::from_uuid(Uuid::from_u128(1)),
        DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        5,
        local,
    );
    assert_eq!(result.ended_at().offset(), UtcOffset::UTC);
    let record = EndPublicationRecord::new(
        command("end", "editor"),
        local,
        EventId::from_uuid(Uuid::from_u128(3)),
        AuditEventId::from_uuid(Uuid::from_u128(4)),
    );
    assert_eq!(record.ended_at().offset(), UtcOffset::UTC);
}

fn candidate() -> EndPublicationCandidate {
    let created_at = OffsetDateTime::UNIX_EPOCH;
    let document = Document::restore(RestoreDocument {
        document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
        folder_id: FolderId::from_uuid(Uuid::from_u128(3)),
        current_version_id: Some(DocumentVersionId::from_uuid(Uuid::from_u128(2))),
        revision: 4,
        metadata: Metadata::default(),
        created_at,
    })
    .unwrap();
    let version = DocumentVersion::restore(RestoreDocumentVersion {
        document_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        document_id: document.document_id(),
        version_no: VersionNo::new(1).unwrap(),
        base_document_version_id: None,
        lifecycle_state: LifecycleState::Published,
        title: Title::new("Published").unwrap(),
        revision_reason: None,
        approved_at: None,
        scheduled_publish_at: None,
        published_at: Some(created_at),
        withdrawn_at: None,
        effective_from: None,
        effective_to: None,
        created_by: PrincipalRef::new("test-idp", "author").unwrap(),
        metadata: Metadata::default(),
        created_at,
    })
    .unwrap();
    EndPublicationCandidate::new(document, Some(version))
}

#[test]
fn publication_end_command_rejects_invalid_id_revision_and_blank_reason() {
    assert!(PublicationEndOperationId::try_from_uuid(Uuid::from_u128(1)).is_err());
    let valid = command("superseded", "editor");
    assert_eq!(valid.operation_id(), operation_id());
    assert!(
        EndDocumentPublicationCommand::new(
            operation_id(),
            valid.document_id(),
            -1,
            valid.expected_current_version_id(),
            valid.actor().clone(),
            "superseded".to_owned(),
        )
        .is_err()
    );
    assert!(
        EndDocumentPublicationCommand::new(
            operation_id(),
            valid.document_id(),
            4,
            valid.expected_current_version_id(),
            valid.actor().clone(),
            "  ".to_owned(),
        )
        .is_err()
    );
}

#[test]
fn publication_end_digest_binds_actor_reason_revision_and_current_version() {
    let original = command("superseded", "editor");
    assert_ne!(
        original.command_digest(),
        command("other", "editor").command_digest()
    );
    assert_ne!(
        original.command_digest(),
        command("superseded", "other").command_digest()
    );
    let revised = EndDocumentPublicationCommand::new(
        operation_id(),
        original.document_id(),
        5,
        original.expected_current_version_id(),
        original.actor().clone(),
        "superseded".to_owned(),
    )
    .unwrap();
    assert_ne!(original.command_digest(), revised.command_digest());
    let other_version = EndDocumentPublicationCommand::new(
        operation_id(),
        original.document_id(),
        4,
        DocumentVersionId::from_uuid(Uuid::from_u128(9)),
        original.actor().clone(),
        "superseded".to_owned(),
    )
    .unwrap();
    assert_ne!(original.command_digest(), other_version.command_digest());
}

struct FixedIds(Mutex<u64>);

impl IdGenerator for FixedIds {
    fn next_uuid_v7(&self) -> Uuid {
        let mut next = self.0.lock().unwrap();
        *next += 1;
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{:012x}", *next)).unwrap()
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_001).unwrap()
    }
}

#[derive(Default)]
struct RepoState {
    candidate: Option<EndPublicationCandidate>,
    stored: Option<EndPublicationOperationRecord>,
    writes: usize,
    unknown_once: bool,
    return_business_rule_once: bool,
    hide_first_stored_lookup: bool,
    operation_reads: usize,
    last_record: Option<EndPublicationRecord>,
}

#[derive(Clone, Default)]
struct FakeRepository(Arc<Mutex<RepoState>>);

impl FakeRepository {
    fn published() -> Self {
        Self(Arc::new(Mutex::new(RepoState {
            candidate: Some(candidate()),
            ..RepoState::default()
        })))
    }

    fn writes(&self) -> usize {
        self.0.lock().unwrap().writes
    }
}

impl PublicationEndRepository for FakeRepository {
    async fn get_end_operation(
        &self,
        id: PublicationEndOperationId,
    ) -> Result<Option<EndPublicationOperationRecord>, RepositoryError> {
        let mut state = self.0.lock().unwrap();
        state.operation_reads += 1;
        if state.hide_first_stored_lookup && state.operation_reads == 1 {
            return Ok(None);
        }
        Ok(state
            .stored
            .clone()
            .filter(|stored| stored.result().operation_id() == id))
    }

    async fn get_end_candidate(
        &self,
        document_id: DocumentId,
    ) -> Result<Option<EndPublicationCandidate>, RepositoryError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .candidate
            .clone()
            .filter(|candidate| candidate.document().document_id() == document_id))
    }

    async fn end_document_publication(
        &self,
        record: EndPublicationRecord,
    ) -> Result<EndDocumentPublicationResult, RepositoryError> {
        let mut state = self.0.lock().unwrap();
        if state.return_business_rule_once {
            state.return_business_rule_once = false;
            return Err(RepositoryError::BusinessRule);
        }
        state.writes += 1;
        let candidate = state.candidate.as_ref().unwrap();
        let mut document = candidate.document().clone();
        let current = candidate.current().unwrap().clone();
        let transition = document.end_publication(&current).unwrap();
        let result = EndDocumentPublicationResult::from_persisted(
            record.command().operation_id(),
            document.document_id(),
            current.document_version_id(),
            transition.resulting_document_revision(),
            record.ended_at(),
        );
        state.candidate = Some(EndPublicationCandidate::new(document, Some(current)));
        state.stored = Some(EndPublicationOperationRecord::new(
            record.command().command_digest(),
            result.clone(),
        ));
        state.last_record = Some(record);
        if state.unknown_once {
            state.unknown_once = false;
            Err(RepositoryError::CommitOutcomeUnknown)
        } else {
            Ok(result)
        }
    }
}

fn service(
    repo: FakeRepository,
) -> DocumentPublicationEndService<FixedIds, FixedClock, FakeRepository> {
    DocumentPublicationEndService::new(
        Arc::new(FixedIds(Mutex::new(100))),
        Arc::new(FixedClock),
        Arc::new(repo),
    )
}

#[tokio::test]
async fn publication_end_replays_same_command_before_stale_preconditions() {
    let repo = FakeRepository::published();
    let service = service(repo.clone());
    let request = command("superseded", "editor");
    let first = service
        .end_document_publication(request.clone())
        .await
        .unwrap();

    assert_eq!(first.operation_id(), operation_id());
    assert_eq!(
        first.former_current_version_id(),
        request.expected_current_version_id()
    );
    assert_eq!(first.resulting_current_version_id(), None);
    assert_eq!(first.resulting_document_revision(), 5);
    assert_eq!(repo.writes(), 1);
    let stored_record = repo.0.lock().unwrap().last_record.clone().unwrap();
    assert_ne!(
        stored_record.domain_event_id(),
        EventId::from_uuid(Uuid::nil())
    );
    assert_ne!(
        stored_record.audit_event_id(),
        AuditEventId::from_uuid(Uuid::nil())
    );

    assert_eq!(
        service.end_document_publication(request).await.unwrap(),
        first
    );
    assert_eq!(repo.writes(), 1);
    assert_eq!(
        service
            .end_document_publication(command("different", "editor"))
            .await,
        Err(ApplicationError::OperationConflict)
    );
    assert_eq!(
        service
            .end_document_publication(command("superseded", "another-editor"))
            .await,
        Err(ApplicationError::OperationConflict)
    );
    assert_eq!(repo.writes(), 1);
}

#[tokio::test]
async fn publication_end_unknown_commit_is_recovered_with_same_operation_id() {
    let repo = FakeRepository::published();
    repo.0.lock().unwrap().unknown_once = true;
    let service = service(repo.clone());
    let request = command("superseded", "editor");

    assert!(matches!(
        service.end_document_publication(request.clone()).await,
        Err(ApplicationError::PublicationEndCommitOutcomeUnknown { operation_id, .. })
            if operation_id == request.operation_id()
    ));
    let result = service.end_document_publication(request).await.unwrap();
    assert_eq!(result.resulting_document_revision(), 5);
    assert_eq!(repo.writes(), 1);
}

#[tokio::test]
async fn publication_end_rechecks_ledger_if_matching_commit_wins_between_reads() {
    let repo = FakeRepository::published();
    let request = command("superseded", "editor");
    let result = EndDocumentPublicationResult::from_persisted(
        request.operation_id(),
        request.document_id(),
        request.expected_current_version_id(),
        5,
        FixedClock.now(),
    );
    {
        let mut state = repo.0.lock().unwrap();
        let previous = state.candidate.take().unwrap();
        let mut ended = previous.document().clone();
        ended.end_publication(previous.current().unwrap()).unwrap();
        state.candidate = Some(EndPublicationCandidate::new(ended, None));
        state.stored = Some(EndPublicationOperationRecord::new(
            request.command_digest(),
            result.clone(),
        ));
        state.hide_first_stored_lookup = true;
    }

    assert_eq!(
        service(repo.clone())
            .end_document_publication(request)
            .await,
        Ok(result)
    );
    assert_eq!(repo.writes(), 0);
}

#[tokio::test]
async fn publication_end_keeps_distinct_finished_document_as_business_rule() {
    let repo = FakeRepository::published();
    repo.0.lock().unwrap().return_business_rule_once = true;

    assert_eq!(
        service(repo.clone())
            .end_document_publication(command("superseded", "editor"))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(repo.writes(), 0);
}

#[tokio::test]
async fn publication_end_rejects_absent_unpublished_and_stale_documents_before_writing() {
    let absent = FakeRepository::default();
    assert_eq!(
        service(absent.clone())
            .end_document_publication(command("end", "editor"))
            .await,
        Err(ApplicationError::DocumentNotFound)
    );
    assert_eq!(absent.writes(), 0);

    let unpublished = FakeRepository::published();
    {
        let mut state = unpublished.0.lock().unwrap();
        state.candidate = Some(EndPublicationCandidate::new(
            Document::restore(RestoreDocument {
                document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
                folder_id: FolderId::from_uuid(Uuid::from_u128(3)),
                current_version_id: None,
                revision: 4,
                metadata: Metadata::default(),
                created_at: OffsetDateTime::UNIX_EPOCH,
            })
            .unwrap(),
            None,
        ));
    }
    assert_eq!(
        service(unpublished.clone())
            .end_document_publication(command("end", "editor"))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(unpublished.writes(), 0);

    let stale = FakeRepository::published();
    let stale_request = EndDocumentPublicationCommand::new(
        operation_id(),
        DocumentId::from_uuid(Uuid::from_u128(1)),
        3,
        DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        PrincipalRef::new("test-idp", "editor").unwrap(),
        "end".to_owned(),
    )
    .unwrap();
    assert_eq!(
        service(stale.clone())
            .end_document_publication(stale_request)
            .await,
        Err(ApplicationError::Conflict)
    );
    assert_eq!(stale.writes(), 0);
}
