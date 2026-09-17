use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    AUDIT_DOCUMENT_VERSION_PUBLISHED, ApplicationError, Clock, ContentReader,
    DOCUMENT_VERSION_PUBLISHED, DocumentPublishRepository, DocumentService, FileStorage, IdGenerator,
    PublishCandidate, PublishCommandIdentity, PublishDocumentCommand, PublishDocumentResult,
    PublishInitialVersionRecord, PublishOperationId, PublishOperationRecord, RepositoryError,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{
    ContentHash, CreateInitialDocument, DocumentId, DocumentVersionId, FileId, FileSize, FolderId,
    InitialDocument, MediaType, Metadata, PrincipalRef, StorageKey, StoredFileDescriptor, Title,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn valid_operation_id(value: u8) -> PublishOperationId {
    let raw = format!("01890f7a-6f6e-7b0a-8000-{value:012x}");
    PublishOperationId::try_from_uuid(Uuid::parse_str(&raw).unwrap()).unwrap()
}

#[test]
fn publish_operation_id_requires_uuid_v7() {
    let valid = Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap();
    let invalid = Uuid::from_u128(1);

    assert_eq!(
        PublishOperationId::try_from_uuid(valid).unwrap().as_uuid(),
        valid
    );
    assert_eq!(
        PublishOperationId::try_from_uuid(invalid),
        Err(ApplicationError::Validation(
            "publish operation id must be UUIDv7".to_owned()
        ))
    );
}

#[test]
fn publish_command_rejects_negative_expected_revision() {
    let result = PublishDocumentCommand::new(
        valid_operation_id(1),
        DocumentId::from_uuid(Uuid::from_u128(10)),
        DocumentVersionId::from_uuid(Uuid::from_u128(11)),
        -1,
        PrincipalRef::new("test-idp", "actor-1").unwrap(),
    );

    assert_eq!(
        result.unwrap_err(),
        ApplicationError::Validation("expected document revision cannot be negative".to_owned())
    );
}

#[test]
fn publish_operation_record_matches_only_the_exact_command_identity() {
    let command = PublishDocumentCommand::new(
        valid_operation_id(2),
        DocumentId::from_uuid(Uuid::from_u128(20)),
        DocumentVersionId::from_uuid(Uuid::from_u128(21)),
        0,
        PrincipalRef::new("test-idp", "actor-1").unwrap(),
    )
    .unwrap();
    let identity = PublishCommandIdentity::from_command(&command);
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_000_010).unwrap();
    let result = PublishDocumentResult::from_persisted(
        command.publish_operation_id(),
        command.document_id(),
        command.target_document_version_id(),
        1,
        published_at,
    );
    let stored = PublishOperationRecord::new(identity.clone(), result.clone());

    assert!(stored.matches_identity(&identity));
    assert_eq!(stored.result(), &result);

    let different_actor = PublishDocumentCommand::new(
        command.publish_operation_id(),
        command.document_id(),
        command.target_document_version_id(),
        command.expected_document_revision(),
        PrincipalRef::new("test-idp", "actor-2").unwrap(),
    )
    .unwrap();
    assert!(!stored.matches_identity(&PublishCommandIdentity::from_command(&different_actor)));
}

#[test]
fn publish_result_round_trips_persisted_fields_and_event_names_are_stable() {
    let operation_id = valid_operation_id(3);
    let document_id = DocumentId::from_uuid(Uuid::from_u128(30));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(31));
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_000_020).unwrap();
    let result = PublishDocumentResult::from_persisted(
        operation_id,
        document_id,
        version_id,
        1,
        published_at,
    );

    assert_eq!(result.publish_operation_id(), operation_id);
    assert_eq!(result.document_id(), document_id);
    assert_eq!(result.document_version_id(), version_id);
    assert_eq!(result.resulting_document_revision(), 1);
    assert_eq!(result.published_at(), published_at);
    assert_eq!(DOCUMENT_VERSION_PUBLISHED, "DocumentVersionPublished");
    assert_eq!(
        AUDIT_DOCUMENT_VERSION_PUBLISHED,
        "document.version.published"
    );
}


#[derive(Clone)]
struct FixedClock(OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

#[derive(Clone)]
struct FixedIds {
    values: Arc<Mutex<VecDeque<Uuid>>>,
}

impl FixedIds {
    fn new() -> Self {
        Self {
            values: Arc::new(Mutex::new(
                (100_u128..=110)
                    .map(Uuid::from_u128)
                    .collect::<VecDeque<_>>(),
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
struct FakeStorage {
    state: Arc<Mutex<StorageState>>,
}

#[derive(Clone)]
struct StorageState {
    open_calls: usize,
    open_result: Result<Vec<u8>, StorageError>,
}

impl FakeStorage {
    fn readable() -> Self {
        Self {
            state: Arc::new(Mutex::new(StorageState {
                open_calls: 0,
                open_result: Ok(b"authoritative".to_vec()),
            })),
        }
    }

    fn failing(error: StorageError) -> Self {
        Self {
            state: Arc::new(Mutex::new(StorageState {
                open_calls: 0,
                open_result: Err(error),
            })),
        }
    }

    fn open_calls(&self) -> usize {
        self.state.lock().unwrap().open_calls
    }
}

impl FileStorage for FakeStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Err(StorageError::Internal("not used by publish contract tests".to_owned()))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        let mut state = self.state.lock().unwrap();
        state.open_calls += 1;
        match state.open_result.clone() {
            Ok(bytes) => Ok(Box::pin(Cursor::new(bytes))),
            Err(error) => Err(error),
        }
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[derive(Clone)]
struct FakePublishRepository {
    state: Arc<Mutex<PublishRepoState>>,
}

#[derive(Clone)]
struct PublishRepoState {
    stored: Option<PublishOperationRecord>,
    candidate: Option<PublishCandidate>,
    write_result: Result<PublishDocumentResult, RepositoryError>,
    candidate_calls: usize,
    write_calls: usize,
}

impl FakePublishRepository {
    fn new(
        candidate: Option<PublishCandidate>,
        write_result: Result<PublishDocumentResult, RepositoryError>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(PublishRepoState {
                stored: None,
                candidate,
                write_result,
                candidate_calls: 0,
                write_calls: 0,
            })),
        }
    }

    fn set_stored(&self, stored: PublishOperationRecord) {
        self.state.lock().unwrap().stored = Some(stored);
    }

    fn candidate_calls(&self) -> usize {
        self.state.lock().unwrap().candidate_calls
    }

    fn write_calls(&self) -> usize {
        self.state.lock().unwrap().write_calls
    }
}

impl DocumentPublishRepository for FakePublishRepository {
    async fn get_publish_operation(
        &self,
        _operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError> {
        Ok(self.state.lock().unwrap().stored.clone())
    }

    async fn get_publish_candidate(
        &self,
        _document_id: DocumentId,
        _target_version_id: DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        state.candidate_calls += 1;
        state
            .candidate
            .clone()
            .ok_or(RepositoryError::DocumentNotFound)
    }

    async fn publish_initial_version(
        &self,
        _record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        state.write_calls += 1;
        state.write_result.clone()
    }
}

fn publish_candidate() -> PublishCandidate {
    let initial = InitialDocument::create(CreateInitialDocument {
        document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        file_id: FileId::from_uuid(Uuid::from_u128(3)),
        folder_id: FolderId::from_uuid(Uuid::from_u128(4)),
        title: Title::new("Policy v1").unwrap(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("test-idp", "actor-1").unwrap(),
        stored_file: StoredFileDescriptor::new(
            StorageKey::new("objects/ab/file").unwrap(),
            ContentHash::from_slice(&[7_u8; 32]).unwrap(),
            FileSize::new(13).unwrap(),
            MediaType::new("application/pdf").unwrap(),
        ),
        original_filename: "policy.pdf".to_owned(),
        created_at: OffsetDateTime::UNIX_EPOCH,
    })
    .unwrap();
    let (document, version, file, version_file) = initial.into_parts();
    PublishCandidate::new(document, version, file, version_file)
}

fn publish_command(operation_value: u8, actor: &str) -> PublishDocumentCommand {
    PublishDocumentCommand::new(
        valid_operation_id(operation_value),
        DocumentId::from_uuid(Uuid::from_u128(1)),
        DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        0,
        PrincipalRef::new("test-idp", actor).unwrap(),
    )
    .unwrap()
}

fn publish_result(
    command: &PublishDocumentCommand,
    published_at: OffsetDateTime,
) -> PublishDocumentResult {
    PublishDocumentResult::from_persisted(
        command.publish_operation_id(),
        command.document_id(),
        command.target_document_version_id(),
        1,
        published_at,
    )
}

fn publish_service(
    now: OffsetDateTime,
    storage: Arc<FakeStorage>,
    repository: Arc<FakePublishRepository>,
) -> DocumentService<FixedIds, FixedClock, FakeStorage, FakePublishRepository> {
    DocumentService::new(Arc::new(FixedIds::new()), Arc::new(FixedClock(now)), storage, repository)
}

#[tokio::test]
async fn publish_replay_returns_before_candidate_or_storage_preflight() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_100).unwrap();
    let command = publish_command(10, "actor-1");
    let stored_result = publish_result(&command, now);
    let repository = Arc::new(FakePublishRepository::new(
        None,
        Ok(stored_result.clone()),
    ));
    repository.set_stored(PublishOperationRecord::new(
        PublishCommandIdentity::from_command(&command),
        stored_result.clone(),
    ));
    let storage = Arc::new(FakeStorage::failing(StorageError::NotFound));
    let service = publish_service(now, storage.clone(), repository.clone());

    let result = service.publish_document(command).await.unwrap();

    assert_eq!(result, stored_result);
    assert_eq!(repository.candidate_calls(), 0);
    assert_eq!(storage.open_calls(), 0);
    assert_eq!(repository.write_calls(), 0);
}

#[tokio::test]
async fn publish_operation_id_misuse_conflicts_before_storage_preflight() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_101).unwrap();
    let original = publish_command(11, "actor-1");
    let incoming = publish_command(11, "actor-2");
    let stored_result = publish_result(&original, now);
    let repository = Arc::new(FakePublishRepository::new(
        None,
        Ok(stored_result.clone()),
    ));
    repository.set_stored(PublishOperationRecord::new(
        PublishCommandIdentity::from_command(&original),
        stored_result,
    ));
    let storage = Arc::new(FakeStorage::readable());
    let service = publish_service(now, storage.clone(), repository.clone());

    let error = service.publish_document(incoming).await.unwrap_err();

    assert_eq!(error, ApplicationError::Conflict);
    assert_eq!(repository.candidate_calls(), 0);
    assert_eq!(storage.open_calls(), 0);
    assert_eq!(repository.write_calls(), 0);
}

#[tokio::test]
async fn publish_missing_primary_object_is_integrity_violation_without_write() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_102).unwrap();
    let command = publish_command(12, "actor-1");
    let repository = Arc::new(FakePublishRepository::new(
        Some(publish_candidate()),
        Ok(publish_result(&command, now)),
    ));
    let storage = Arc::new(FakeStorage::failing(StorageError::NotFound));
    let service = publish_service(now, storage.clone(), repository.clone());

    let error = service.publish_document(command).await.unwrap_err();

    assert_eq!(error, ApplicationError::IntegrityViolation);
    assert_eq!(repository.candidate_calls(), 1);
    assert_eq!(storage.open_calls(), 1);
    assert_eq!(repository.write_calls(), 0);
}

#[tokio::test]
async fn publish_storage_outage_remains_storage_unavailable_without_write() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_103).unwrap();
    let command = publish_command(13, "actor-1");
    let repository = Arc::new(FakePublishRepository::new(
        Some(publish_candidate()),
        Ok(publish_result(&command, now)),
    ));
    let storage = Arc::new(FakeStorage::failing(StorageError::Unavailable));
    let service = publish_service(now, storage.clone(), repository.clone());

    let error = service.publish_document(command).await.unwrap_err();

    assert_eq!(error, ApplicationError::StorageUnavailable);
    assert_eq!(storage.open_calls(), 1);
    assert_eq!(repository.write_calls(), 0);
}

#[tokio::test]
async fn publish_readable_primary_calls_repository_write_once() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_104).unwrap();
    let command = publish_command(14, "actor-1");
    let expected = publish_result(&command, now);
    let repository = Arc::new(FakePublishRepository::new(
        Some(publish_candidate()),
        Ok(expected.clone()),
    ));
    let storage = Arc::new(FakeStorage::readable());
    let service = publish_service(now, storage.clone(), repository.clone());

    let result = service.publish_document(command).await.unwrap();

    assert_eq!(result, expected);
    assert_eq!(storage.open_calls(), 1);
    assert_eq!(repository.write_calls(), 1);
}

#[tokio::test]
async fn publish_unknown_commit_retains_operation_document_and_version_ids() {
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_105).unwrap();
    let command = publish_command(15, "actor-1");
    let operation_id = command.publish_operation_id();
    let document_id = command.document_id();
    let version_id = command.target_document_version_id();
    let repository = Arc::new(FakePublishRepository::new(
        Some(publish_candidate()),
        Err(RepositoryError::CommitOutcomeUnknown),
    ));
    let storage = Arc::new(FakeStorage::readable());
    let service = publish_service(now, storage, repository);

    let error = service.publish_document(command).await.unwrap_err();

    assert_eq!(
        error,
        ApplicationError::PublishCommitOutcomeUnknown {
            publish_operation_id: operation_id,
            document_id,
            document_version_id: version_id,
        }
    );
}
