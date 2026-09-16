use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use document_application::{
    ApplicationError, AuthoritativeDocument, Clock, ContentReader, CreateDocumentCommand,
    CreateInitialDocumentRecord, DocumentRepository, DocumentService, FileStorage, IdGenerator,
    ReconciliationClassification, RepositoryError, StorageError, StorageObjectInfo,
    StorageObjectKind, StoreFileRequest, StoredFile, classify,
};
use document_domain::{
    ContentHash, CreateInitialDocument, DocumentId, DocumentVersionId, FileId, FileSize, FolderId,
    InitialDocument, MediaType, Metadata, PrincipalRef, StorageKey, StoredFileDescriptor, Title,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
struct FixedIds {
    values: Arc<Mutex<VecDeque<Uuid>>>,
}

impl FixedIds {
    fn sequence() -> Self {
        Self {
            values: Arc::new(Mutex::new(
                (1_u128..=16).map(Uuid::from_u128).collect::<VecDeque<_>>(),
            )),
        }
    }
}

impl IdGenerator for FixedIds {
    fn next_uuid_v7(&self) -> Uuid {
        self.values.lock().unwrap().pop_front().unwrap()
    }
}

#[derive(Clone)]
struct FixedClock(OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

#[derive(Default)]
struct StepState {
    next: usize,
    storage_finalized: Option<usize>,
    repository_called: Option<usize>,
}

impl StepState {
    fn mark_storage(&mut self) {
        self.next += 1;
        self.storage_finalized = Some(self.next);
    }

    fn mark_repository(&mut self) {
        self.next += 1;
        self.repository_called = Some(self.next);
    }
}

#[derive(Clone)]
struct FakeStorage {
    steps: Arc<Mutex<StepState>>,
    finalized: Arc<Mutex<bool>>,
    open_result: Arc<Mutex<Result<Vec<u8>, StorageError>>>,
    objects: Arc<Mutex<Vec<StorageObjectInfo>>>,
    object_modified_at: OffsetDateTime,
}

impl FakeStorage {
    fn new(steps: Arc<Mutex<StepState>>) -> Self {
        Self {
            steps,
            finalized: Arc::new(Mutex::new(false)),
            open_result: Arc::new(Mutex::new(Ok(b"authoritative-content".to_vec()))),
            objects: Arc::new(Mutex::new(Vec::new())),
            object_modified_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn set_open_error(&self, error: StorageError) {
        *self.open_result.lock().unwrap() = Err(error);
    }

    fn is_finalized(&self) -> bool {
        *self.finalized.lock().unwrap()
    }
}

impl FileStorage for FakeStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let file_id = request.file_id();
        self.steps.lock().unwrap().mark_storage();
        *self.finalized.lock().unwrap() = true;
        self.objects.lock().unwrap().push(StorageObjectInfo::new(
            "objects/00/file",
            StorageObjectKind::Final,
            Some(file_id),
            self.object_modified_at,
        ));
        Ok(StoredFile::new(
            StorageKey::new("objects/00/file").unwrap(),
            ContentHash::from_slice(&[7_u8; 32]).unwrap(),
            FileSize::new(21).unwrap(),
            MediaType::new("application/pdf").unwrap(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        match self.open_result.lock().unwrap().clone() {
            Ok(bytes) => Ok(Box::pin(Cursor::new(bytes))),
            Err(error) => Err(error),
        }
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(self.objects.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct RepoState {
    create_error: Option<RepositoryError>,
    persist_before_error: bool,
    create_calls: usize,
    document: Option<AuthoritativeDocument>,
    domain_event_types: Vec<String>,
    audit_event_types: Vec<String>,
    event_times: Vec<OffsetDateTime>,
}

#[derive(Clone)]
struct FakeRepository {
    steps: Arc<Mutex<StepState>>,
    state: Arc<Mutex<RepoState>>,
}

impl FakeRepository {
    fn new(steps: Arc<Mutex<StepState>>) -> Self {
        Self {
            steps,
            state: Arc::new(Mutex::new(RepoState::default())),
        }
    }

    fn fail_create_with(&self, error: RepositoryError) {
        self.state.lock().unwrap().create_error = Some(error);
    }

    fn persist_then_fail_with(&self, error: RepositoryError) {
        let mut state = self.state.lock().unwrap();
        state.persist_before_error = true;
        state.create_error = Some(error);
    }

    fn set_document(&self, document: AuthoritativeDocument) {
        self.state.lock().unwrap().document = Some(document);
    }

    fn create_calls(&self) -> usize {
        self.state.lock().unwrap().create_calls
    }
}

impl DocumentRepository for FakeRepository {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        self.steps.lock().unwrap().mark_repository();
        let mut state = self.state.lock().unwrap();
        state.create_calls += 1;
        state.domain_event_types = record
            .domain_events()
            .iter()
            .map(|event| event.event_type().to_owned())
            .collect();
        state.audit_event_types = record
            .audit_events()
            .iter()
            .map(|event| event.event_type().to_owned())
            .collect();
        state.event_times = record
            .domain_events()
            .iter()
            .map(|event| event.occurred_at())
            .chain(
                record
                    .audit_events()
                    .iter()
                    .map(|event| event.occurred_at()),
            )
            .collect();

        if state.persist_before_error || state.create_error.is_none() {
            state.document = Some(record.authoritative().clone());
        }

        match state.create_error.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn get_authoritative_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .document
            .clone()
            .filter(|document| document.document().document_id() == id))
    }

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .document
            .as_ref()
            .is_some_and(|document| document.file().file_id() == file_id))
    }

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .document
            .as_ref()
            .map(|document| vec![document.file().file_id()])
            .unwrap_or_default())
    }
}

fn content() -> ContentReader {
    Box::pin(Cursor::new(b"authoritative-content".to_vec()))
}

fn command() -> CreateDocumentCommand {
    CreateDocumentCommand {
        folder_id: FolderId::from_uuid(Uuid::from_u128(100)),
        title: "Policy v1".into(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("windows", "principal-1").unwrap(),
        original_filename: "policy.pdf".into(),
        media_type: MediaType::new("application/pdf").unwrap(),
        content: content(),
    }
}

fn authoritative_document() -> AuthoritativeDocument {
    let aggregate = InitialDocument::create(CreateInitialDocument {
        document_id: DocumentId::from_uuid(Uuid::from_u128(201)),
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(202)),
        file_id: FileId::from_uuid(Uuid::from_u128(203)),
        folder_id: FolderId::from_uuid(Uuid::from_u128(204)),
        title: Title::new("Policy v1").unwrap(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("windows", "principal-1").unwrap(),
        stored_file: StoredFileDescriptor::new(
            StorageKey::new("objects/00/file").unwrap(),
            ContentHash::from_slice(&[9_u8; 32]).unwrap(),
            FileSize::new(21).unwrap(),
            MediaType::new("application/pdf").unwrap(),
        ),
        original_filename: "policy.pdf".into(),
        created_at: OffsetDateTime::UNIX_EPOCH,
    })
    .unwrap();
    AuthoritativeDocument::from_initial(aggregate)
}

fn service(
    storage: Arc<FakeStorage>,
    repository: Arc<FakeRepository>,
) -> DocumentService<FixedIds, FixedClock, FakeStorage, FakeRepository> {
    service_at(storage, repository, OffsetDateTime::UNIX_EPOCH)
}

fn service_at(
    storage: Arc<FakeStorage>,
    repository: Arc<FakeRepository>,
    now: OffsetDateTime,
) -> DocumentService<FixedIds, FixedClock, FakeStorage, FakeRepository> {
    DocumentService::new(
        Arc::new(FixedIds::sequence()),
        Arc::new(FixedClock(now)),
        storage,
        repository,
    )
}

#[test]
fn reconciliation_classification_is_conservative_across_db_storage_and_grace_states() {
    let now = OffsetDateTime::UNIX_EPOCH + Duration::hours(2);
    let grace = Duration::hours(1);
    let old = now - grace - Duration::seconds(1);
    let recent = now - Duration::minutes(10);
    let file_id = FileId::from_uuid(Uuid::from_u128(300));

    let final_object = StorageObjectInfo::new(
        "objects/00/file",
        StorageObjectKind::Final,
        Some(file_id),
        old,
    );
    let staging_object = StorageObjectInfo::new(
        "staging/file.part",
        StorageObjectKind::Staging,
        Some(file_id),
        old,
    );
    let recent_final = StorageObjectInfo::new(
        "objects/00/file",
        StorageObjectKind::Final,
        Some(file_id),
        recent,
    );

    assert_eq!(
        classify(true, Some(&final_object), now, grace),
        Some(ReconciliationClassification::Healthy)
    );
    assert_eq!(
        classify(false, Some(&staging_object), now, grace),
        Some(ReconciliationClassification::StaleStaging)
    );
    assert_eq!(
        classify(false, Some(&final_object), now, grace),
        Some(ReconciliationClassification::Orphan)
    );
    assert_eq!(
        classify(true, None, now, grace),
        Some(ReconciliationClassification::IntegrityViolation)
    );
    assert_eq!(classify(false, Some(&recent_final), now, grace), None);
}

#[tokio::test]
async fn create_finalizes_storage_before_calling_atomic_repository() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    let repository = Arc::new(FakeRepository::new(steps.clone()));
    let service = service(storage, repository.clone());

    let result = service.create_document(command()).await.unwrap();

    let steps = steps.lock().unwrap();
    assert!(steps.storage_finalized.unwrap() < steps.repository_called.unwrap());
    assert_eq!(result.document_id().as_uuid(), Uuid::from_u128(1));
    assert_eq!(result.document_version_id().as_uuid(), Uuid::from_u128(2));
    assert_eq!(result.file_id().as_uuid(), Uuid::from_u128(3));

    let state = repository.state.lock().unwrap();
    assert_eq!(
        state.domain_event_types,
        ["DocumentCreated", "DocumentVersionCreated"]
    );
    assert_eq!(
        state.audit_event_types,
        ["document.created", "document.version.created"]
    );
    assert!(
        state
            .event_times
            .iter()
            .all(|occurred_at| *occurred_at == OffsetDateTime::UNIX_EPOCH)
    );
}

#[tokio::test]
async fn ambiguous_commit_keeps_finalized_file_and_surfaces_unknown_outcome() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    let repository = Arc::new(FakeRepository::new(steps));
    repository.fail_create_with(RepositoryError::CommitOutcomeUnknown);
    let service = service(storage.clone(), repository);

    let error = service.create_document(command()).await.unwrap_err();

    assert!(matches!(
        error,
        ApplicationError::CommitOutcomeUnknown { .. }
    ));
    assert!(storage.is_finalized());
}

#[tokio::test]
async fn ambiguous_commit_can_be_resolved_by_known_document_id_without_retrying_create() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    let repository = Arc::new(FakeRepository::new(steps));
    repository.persist_then_fail_with(RepositoryError::CommitOutcomeUnknown);
    let service = service(storage.clone(), repository.clone());

    let error = service.create_document(command()).await.unwrap_err();
    let known_document_id = match error {
        ApplicationError::CommitOutcomeUnknown { document_id, .. } => document_id,
        other => panic!("expected ambiguous create identity, got {other:?}"),
    };
    assert!(storage.is_finalized());

    let recovered = service
        .lookup_create_outcome(known_document_id)
        .await
        .unwrap()
        .expect("ambiguous commit was persisted and must be discoverable by the known ID");

    assert_eq!(recovered.document().document_id(), known_document_id);
    assert_eq!(repository.create_calls(), 1);
}

#[tokio::test]
async fn ambiguous_commit_without_persistence_becomes_orphan_only_after_grace() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    let repository = Arc::new(FakeRepository::new(steps));
    repository.fail_create_with(RepositoryError::CommitOutcomeUnknown);
    let create_service = service(storage.clone(), repository.clone());
    let grace = Duration::hours(1);

    let error = create_service.create_document(command()).await.unwrap_err();
    let (known_document_id, expected_file_id) = match error {
        ApplicationError::CommitOutcomeUnknown {
            document_id,
            file_id,
            ..
        } => (document_id, file_id),
        other => panic!("expected ambiguous create identity, got {other:?}"),
    };
    assert!(storage.is_finalized());
    assert!(
        create_service
            .lookup_create_outcome(known_document_id)
            .await
            .unwrap()
            .is_none()
    );

    let recent_service = service_at(
        storage.clone(),
        repository.clone(),
        OffsetDateTime::UNIX_EPOCH + Duration::minutes(30),
    );
    assert!(
        recent_service
            .reconcile_storage(grace)
            .await
            .unwrap()
            .is_empty()
    );

    let late_service = service_at(
        storage,
        repository.clone(),
        OffsetDateTime::UNIX_EPOCH + Duration::hours(2),
    );
    let findings = late_service.reconcile_storage(grace).await.unwrap();

    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].classification(),
        ReconciliationClassification::Orphan
    );
    assert_eq!(findings[0].file_id(), expected_file_id);
    assert_eq!(
        findings[0].object().and_then(StorageObjectInfo::file_id),
        Some(expected_file_id)
    );
    assert_eq!(repository.create_calls(), 1);
}

#[tokio::test]
async fn get_document_maps_missing_authoritative_record_to_not_found() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    let repository = Arc::new(FakeRepository::new(steps));
    let service = service(storage, repository);

    let error = service
        .get_document(DocumentId::from_uuid(Uuid::from_u128(999)))
        .await
        .unwrap_err();

    assert_eq!(error, ApplicationError::DocumentNotFound);
}

#[tokio::test]
async fn missing_referenced_binary_is_integrity_violation_not_document_not_found() {
    let steps = Arc::new(Mutex::new(StepState::default()));
    let storage = Arc::new(FakeStorage::new(steps.clone()));
    storage.set_open_error(StorageError::NotFound);
    let repository = Arc::new(FakeRepository::new(steps));
    let document = authoritative_document();
    let document_id = document.document().document_id();
    repository.set_document(document);
    let service = service(storage, repository);

    let error = match service.open_primary_file(document_id).await {
        Ok(_) => panic!("expected missing referenced binary to fail"),
        Err(error) => error,
    };

    assert_eq!(error, ApplicationError::IntegrityViolation);
}
