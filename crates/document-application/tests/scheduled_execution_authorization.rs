//! Deterministic scheduling seams: a retry can become due between repository
//! reads, but an authorization-free replay path must never execute publication.

use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, AuthoritativeDocument, AuthorizationScope, CancelOperationRecord,
    CancelScheduleRecord, CancelScheduleResult, Clock, ContentReader, CreateInitialDocumentRecord,
    CurrentPublishedVersionRef, DocumentPublishRepository, DocumentRepository,
    DocumentVersionService, DueExecutionOutcome, DueTerminalRecord, FileStorage, IdGenerator,
    IdentityContextResolver, IdentityResolutionError, InspectionExecutionError, InvocationKind,
    PublicationScheduleRepository, PublishCandidate, PublishCommandIdentity, PublishDocumentResult,
    PublishInitialVersionRecord, PublishOperationId, PublishOperationRecord, RepositoryError,
    ScheduleOperationRecord, SchedulePublishCommand, SchedulePublishRecord, SchedulePublishResult,
    SemanticInspectionExecutor, SemanticInspectionRecord, SemanticInspectionRepository,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile, VerifiedActorContext,
    VersionOperationId, VersioningRepository,
};
use document_domain::{
    ContentHash, CreateInitialDocument, Document, DocumentId, DocumentVersionId, FileId,
    FileObject, FileSize, FolderId, InitialDocument, MediaType, Metadata, PolicySubject,
    PolicySubjectKind, PrincipalRef, RestoreDocument, StorageKey, StoredFileDescriptor, Title,
};
use document_semantic_inspection_core::{InspectionProfileVersion, WorkerRequest, WorkerResponse};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn requester() -> PrincipalRef {
    PrincipalRef::new("test", "requester").unwrap()
}

fn executor() -> PrincipalRef {
    PrincipalRef::new("service", "scheduler").unwrap()
}

fn operation_id() -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap(),
    )
    .unwrap()
}

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
}

struct FixtureDependencies;

impl Clock for FixtureDependencies {
    fn now(&self) -> OffsetDateTime {
        now()
    }
}

impl IdGenerator for FixtureDependencies {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

impl FileStorage for FixtureDependencies {
    async fn put_immutable(&self, _: StoreFileRequest) -> Result<StoredFile, StorageError> {
        panic!("due execution must not upload a file")
    }

    async fn open(&self, _: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(Cursor::new(b"text".to_vec())))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        panic!("due execution must not list storage")
    }
}

impl SemanticInspectionExecutor for FixtureDependencies {
    async fn inspect(
        &self,
        _: WorkerRequest,
        _: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        panic!("fixture provides a valid cached inspection")
    }
}

struct Resolver {
    result: Result<VerifiedActorContext, IdentityResolutionError>,
    requests: Mutex<Vec<PrincipalRef>>,
}

impl Resolver {
    fn valid() -> Self {
        Self {
            result: Ok(VerifiedActorContext::from_trusted_adapter(
                requester(),
                vec![
                    PolicySubject::new(PolicySubjectKind::Principal, "test", "requester").unwrap(),
                ],
                OffsetDateTime::now_utc() + Duration::hours(1),
                InvocationKind::HumanInteractive,
                None,
            )
            .unwrap()),
            requests: Mutex::new(vec![]),
        }
    }

    fn invalid() -> Self {
        Self {
            result: Err(IdentityResolutionError::InvalidIdentity),
            requests: Mutex::new(vec![]),
        }
    }
}

impl IdentityContextResolver for Resolver {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        self.requests.lock().unwrap().push(principal.clone());
        self.result.clone()
    }
}

struct State {
    due: VecDeque<bool>,
    stored: Option<PublishOperationRecord>,
    // Simulate another scheduler committing between the two replay lookups.
    publish_reads_before_visible: usize,
    publications: Vec<Option<VerifiedActorContext>>,
    terminal: Vec<DueTerminalRecord>,
    retries: usize,
    acl_allowed: bool,
    authorizations: Vec<VerifiedActorContext>,
}

#[derive(Clone)]
struct Repository {
    schedule: ScheduleOperationRecord,
    snapshot: AuthoritativeDocument,
    state: Arc<Mutex<State>>,
    ctx: Option<VerifiedActorContext>,
}

impl Repository {
    fn new(due: impl IntoIterator<Item = bool>) -> Self {
        let initial = InitialDocument::create(CreateInitialDocument {
            document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
            version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
            file_id: FileId::from_uuid(Uuid::from_u128(3)),
            folder_id: FolderId::from_uuid(Uuid::from_u128(4)),
            title: Title::new("Scheduled text").unwrap(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: requester(),
            stored_file: StoredFileDescriptor::new(
                StorageKey::new("objects/text").unwrap(),
                ContentHash::from_slice(&[7; 32]).unwrap(),
                FileSize::new(4).unwrap(),
                MediaType::new("text/plain").unwrap(),
            ),
            original_filename: "text.txt".to_owned(),
            created_at: now() - Duration::hours(1),
        })
        .unwrap();
        let (document, version, file, version_file) = initial.into_parts();
        let document = Document::restore(RestoreDocument {
            document_id: document.document_id(),
            folder_id: document.folder_id(),
            current_version_id: None,
            revision: 1,
            metadata: document.metadata().clone(),
            created_at: document.created_at(),
        })
        .unwrap();
        let command = SchedulePublishCommand::new(
            operation_id(),
            document.document_id(),
            version.document_version_id(),
            0,
            requester(),
            now() - Duration::seconds(1),
        )
        .unwrap();
        let result = SchedulePublishResult {
            publish_operation_id: operation_id(),
            document_id: command.document_id(),
            target_version_id: command.target_version_id(),
            accepted_revision: 1,
            scheduled_publish_at: command.scheduled_publish_at(),
        };
        Self {
            schedule: ScheduleOperationRecord {
                command,
                result,
                manifest_digest: [0; 32],
                status: "PENDING".to_owned(),
            },
            snapshot: AuthoritativeDocument::from_parts(document, version, file, version_file),
            state: Arc::new(Mutex::new(State {
                due: due.into_iter().collect(),
                stored: None,
                publish_reads_before_visible: 0,
                publications: vec![],
                terminal: vec![],
                retries: 0,
                acl_allowed: true,
                authorizations: vec![],
            })),
            ctx: None,
        }
    }

    fn stored_operation(&self, actor: PrincipalRef) -> PublishOperationRecord {
        PublishOperationRecord::new(
            PublishCommandIdentity::from_persisted(
                operation_id(),
                self.schedule.command.document_id(),
                self.schedule.command.target_version_id(),
                self.schedule.result.accepted_revision,
                actor,
            ),
            PublishDocumentResult::from_persisted(
                operation_id(),
                self.schedule.command.document_id(),
                self.schedule.command.target_version_id(),
                2,
                now(),
            ),
        )
    }
}

impl AuthorizationScope for Repository {
    fn with_verified_actor(&self, ctx: VerifiedActorContext) -> Self {
        Self {
            ctx: Some(ctx),
            ..self.clone()
        }
    }
}

impl DocumentPublishRepository for Repository {
    async fn get_publish_operation(
        &self,
        _: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        if state.publish_reads_before_visible > 0 {
            state.publish_reads_before_visible -= 1;
            return Ok(None);
        }
        Ok(state.stored.clone())
    }

    async fn get_publish_candidate(
        &self,
        _: DocumentId,
        _: DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError> {
        panic!("version service loads the version snapshot")
    }

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        assert!(record.scheduled_due());
        let (operation, _, _) = record.into_parts();
        let mut state = self.state.lock().unwrap();
        state.publications.push(self.ctx.clone());
        state.stored = Some(operation.clone());
        Ok(operation.result().clone())
    }
}

impl PublicationScheduleRepository for Repository {
    async fn authorize_due_document(
        &self,
        ctx: &VerifiedActorContext,
        _: DocumentId,
    ) -> Result<bool, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        state.authorizations.push(ctx.clone());
        Ok(state.acl_allowed)
    }

    async fn get_schedule(
        &self,
        _: PublishOperationId,
    ) -> Result<Option<ScheduleOperationRecord>, RepositoryError> {
        Ok(Some(self.schedule.clone()))
    }

    async fn is_due(&self, _: PublishOperationId) -> Result<bool, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let due = *state.due.front().expect("test must supply due state");
        if state.due.len() > 1 {
            state.due.pop_front();
        }
        Ok(due)
    }

    async fn database_now(&self) -> Result<OffsetDateTime, RepositoryError> {
        Ok(now())
    }

    async fn terminalize(&self, record: DueTerminalRecord) -> Result<(), RepositoryError> {
        self.state.lock().unwrap().terminal.push(record);
        Ok(())
    }

    async fn record_retry(&self, _: PublishOperationId) -> Result<OffsetDateTime, RepositoryError> {
        self.state.lock().unwrap().retries += 1;
        Ok(now() + Duration::seconds(1))
    }

    async fn reserve(
        &self,
        _: SchedulePublishRecord,
    ) -> Result<SchedulePublishResult, RepositoryError> {
        panic!("not used")
    }
    async fn get_cancel_operation(
        &self,
        _: VersionOperationId,
    ) -> Result<Option<CancelOperationRecord>, RepositoryError> {
        panic!("not used")
    }
    async fn cancel(
        &self,
        _: CancelScheduleRecord,
    ) -> Result<CancelScheduleResult, RepositoryError> {
        panic!("not used")
    }
    async fn list_due(
        &self,
        _: OffsetDateTime,
        _: i64,
    ) -> Result<Vec<PublishOperationId>, RepositoryError> {
        panic!("not used")
    }
}

impl VersioningRepository for Repository {
    async fn register_file_object(&self, _: FileObject) -> Result<(), RepositoryError> {
        panic!("not used")
    }

    async fn get_version_snapshot(
        &self,
        _: DocumentId,
        _: DocumentVersionId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(Some(self.snapshot.clone()))
    }
}

impl SemanticInspectionRepository for Repository {
    async fn get_file_object(&self, _: FileId) -> Result<Option<FileObject>, RepositoryError> {
        Ok(Some(self.snapshot.file().clone()))
    }

    async fn get_semantic_inspection(
        &self,
        file_id: FileId,
        _: InspectionProfileVersion,
    ) -> Result<Option<SemanticInspectionRecord>, RepositoryError> {
        let response = serde_json::from_value(serde_json::json!({
            "protocol_version": "dsi-worker-v0", "inspection_profile_version": "dsi-v0",
            "observed_raw_content_hash": vec![7; 32], "observed_size_bytes": 4,
            "detected_format": "txt", "semantic_fingerprint": {"algorithm": "sha256", "digest": vec![8; 32]},
            "semantic_capabilities": [],
            "editorial_provenance": {"tracked_changes": [], "comments": [], "document_author_labels": [], "last_modified_by": null, "modification_metadata": {}},
            "external_dependencies": [], "digital_signature_evidence": [],
            "extractor_provenance": {"worker_build_id": "fixture", "adapter_id": "txt", "adapter_version": "1", "parser_libraries": [], "native_dependency_identity": []},
            "diagnostics": []
        })).unwrap();
        Ok(Some(
            SemanticInspectionRecord::restore(file_id, response, now()).unwrap(),
        ))
    }

    async fn insert_or_converge_semantic_inspection(
        &self,
        _: SemanticInspectionRecord,
    ) -> Result<SemanticInspectionRecord, RepositoryError> {
        panic!("cached fixture")
    }
}

impl DocumentRepository for Repository {
    async fn create_initial_document(
        &self,
        _: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        panic!("not used")
    }
    async fn get_authoritative_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        panic!("not used")
    }
    async fn get_authoring_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        panic!("not used")
    }
    async fn get_current_published_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        panic!("not used")
    }
    async fn is_current_published_version(
        &self,
        _: DocumentId,
        _: DocumentVersionId,
    ) -> Result<bool, RepositoryError> {
        panic!("not used")
    }
    async fn list_current_published_versions(
        &self,
        _: Option<DocumentId>,
        _: i64,
    ) -> Result<Vec<CurrentPublishedVersionRef>, RepositoryError> {
        panic!("not used")
    }
    async fn file_reference_exists(&self, _: FileId) -> Result<bool, RepositoryError> {
        panic!("not used")
    }
    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        panic!("not used")
    }
}

fn service(
    repository: &Repository,
) -> DocumentVersionService<
    FixtureDependencies,
    FixtureDependencies,
    FixtureDependencies,
    FixtureDependencies,
    Repository,
> {
    let dependencies = Arc::new(FixtureDependencies);
    DocumentVersionService::new(
        dependencies.clone(),
        dependencies.clone(),
        dependencies.clone(),
        dependencies,
        Arc::new(repository.clone()),
    )
}

#[tokio::test]
async fn not_due_to_due_transition_cannot_publish_without_authorization() {
    // Another scheduler's retry deadline is pending on the first check and has
    // expired on a subsequent check. No sleeps or wall-clock races are needed.
    let repository = Repository::new([false, true]);
    let resolver = Resolver::invalid();
    let service = service(&repository);
    assert_eq!(
        service
            .execute_due_authorized(operation_id(), &resolver, &executor())
            .await,
        Ok(DueExecutionOutcome::NotDue),
    );
    {
        let state = repository.state.lock().unwrap();
        assert!(state.publications.is_empty());
        assert!(state.terminal.is_empty());
        assert_eq!(state.retries, 0);
    }
    assert!(resolver.requests.lock().unwrap().is_empty());

    // A subsequent execution must re-resolve the stored requester and fail
    // closed, recording the executor only as attribution.
    assert_eq!(
        service
            .execute_due_authorized(operation_id(), &resolver, &executor())
            .await,
        Ok(DueExecutionOutcome::Terminal("identity_invalid".to_owned())),
    );
    assert_eq!(*resolver.requests.lock().unwrap(), vec![requester()]);
    let state = repository.state.lock().unwrap();
    assert!(state.publications.is_empty());
    assert_eq!(state.terminal[0].service_executor, Some(executor()));
}

#[tokio::test]
async fn due_execution_resolves_requester_checks_acl_and_scopes_publication() {
    let repository = Repository::new([true]);
    let resolver = Resolver::valid();
    let result = service(&repository)
        .execute_due_authorized(operation_id(), &resolver, &executor())
        .await
        .unwrap();
    assert!(matches!(result, DueExecutionOutcome::Published(_)));
    assert_eq!(*resolver.requests.lock().unwrap(), vec![requester()]);
    let state = repository.state.lock().unwrap();
    assert_eq!(state.publications.len(), 1);
    let ctx = state.publications[0].as_ref().unwrap();
    assert_eq!(ctx.principal(), &requester());
    assert_eq!(ctx.invocation_kind(), InvocationKind::Service);
    assert_eq!(ctx.service_executor(), Some(&executor()));
    assert_eq!(ctx.subjects().len(), 1);
    assert_eq!(ctx.subjects()[0].subject_id(), "requester");
    assert_eq!(state.authorizations.len(), 1);
    let authorized = &state.authorizations[0];
    assert_eq!(authorized.principal(), ctx.principal());
    assert_eq!(authorized.subjects(), ctx.subjects());
    assert_eq!(authorized.service_executor(), ctx.service_executor());
    assert_eq!(authorized.invocation_kind(), ctx.invocation_kind());
}

#[tokio::test]
async fn due_execution_with_revoked_acl_cannot_publish() {
    let repository = Repository::new([true]);
    repository.state.lock().unwrap().acl_allowed = false;
    let resolver = Resolver::valid();
    assert_eq!(
        service(&repository)
            .execute_due_authorized(operation_id(), &resolver, &executor())
            .await,
        Ok(DueExecutionOutcome::Terminal(
            "authorization_revoked".to_owned()
        ))
    );
    let state = repository.state.lock().unwrap();
    assert!(state.publications.is_empty());
    assert_eq!(state.terminal[0].service_executor, Some(executor()));
    assert_eq!(*resolver.requests.lock().unwrap(), vec![requester()]);
}

#[tokio::test]
async fn authorized_replay_preserves_both_lookup_windows_and_identity_conflicts() {
    for reads_before_visible in [0, 1] {
        for matches_identity in [true, false] {
            let repository = Repository::new([false]);
            let actor = if matches_identity {
                requester()
            } else {
                executor()
            };
            let stored = repository.stored_operation(actor);
            let expected = if matches_identity {
                Ok(DueExecutionOutcome::Published(stored.result().clone()))
            } else {
                Err(ApplicationError::IntegrityViolation)
            };
            {
                let mut state = repository.state.lock().unwrap();
                state.stored = Some(stored);
                state.publish_reads_before_visible = reads_before_visible;
            }
            let resolver = Resolver::invalid();
            assert_eq!(
                service(&repository)
                    .execute_due_authorized(operation_id(), &resolver, &executor())
                    .await,
                expected
            );
            assert!(resolver.requests.lock().unwrap().is_empty());
            assert!(repository.state.lock().unwrap().publications.is_empty());
        }
    }
}

#[tokio::test]
async fn inactive_schedules_preserve_completed_replay_and_do_not_resolve_identity() {
    for status in ["PUBLISHED", "CANCELLED", "FAILED"] {
        let mut repository = Repository::new([]);
        repository.schedule.status = status.to_owned();
        let resolver = Resolver::invalid();
        assert_eq!(
            service(&repository)
                .execute_due_authorized(operation_id(), &resolver, &executor())
                .await,
            Ok(DueExecutionOutcome::Inactive)
        );
        let stored = repository.stored_operation(requester());
        repository.state.lock().unwrap().stored = Some(stored.clone());
        assert_eq!(
            service(&repository)
                .execute_due_authorized(operation_id(), &resolver, &executor())
                .await,
            Ok(DueExecutionOutcome::Published(stored.result().clone()))
        );
        assert!(resolver.requests.lock().unwrap().is_empty());
        assert!(repository.state.lock().unwrap().publications.is_empty());
    }
}

#[tokio::test]
async fn legacy_due_execution_remains_executable_without_authorization_context() {
    let repository = Repository::new([true]);
    let result = service(&repository)
        .execute_due(operation_id())
        .await
        .unwrap();
    assert!(matches!(result, DueExecutionOutcome::Published(_)));
    let state = repository.state.lock().unwrap();
    assert_eq!(state.publications.len(), 1);
    assert!(state.publications[0].is_none());
}
