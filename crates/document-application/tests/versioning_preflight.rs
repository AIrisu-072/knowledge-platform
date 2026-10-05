use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use document_application::{
    ApplicationError, Clock, ContentReader, FileStorage, InspectionExecutionError, RepositoryError,
    SemanticInspectionExecutor, SemanticInspectionRecord, SemanticInspectionRepository,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile, VersioningItemInput,
    VersioningPreflight, VersioningRenditionInput, VersioningRepository,
};
use document_domain::{
    ContentHash, FileId, FileObject, FileSize, LogicalPath, MediaType, StorageKey, Title,
};
use document_semantic_inspection_core::{InspectionProfileVersion, WorkerRequest, WorkerResponse};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

fn file_id(raw: u128) -> FileId {
    FileId::from_uuid(Uuid::from_u128(raw))
}

fn input(path: &str, ordinal: u32, raw_id: u128, byte: u8) -> VersioningItemInput {
    VersioningItemInput::new(
        LogicalPath::new(path).unwrap(),
        ordinal,
        file_id(raw_id),
        MediaType::new("text/plain").unwrap(),
        format!("{path}.txt"),
        Box::pin(Cursor::new(vec![byte; 3])),
    )
}

fn rendition(raw_id: u128, byte: u8) -> VersioningRenditionInput {
    VersioningRenditionInput::new(
        file_id(raw_id),
        MediaType::new("text/plain").unwrap(),
        "preview.txt",
        Box::pin(Cursor::new(vec![byte; 3])),
    )
}

#[derive(Clone, Copy)]
enum Mode {
    Plain,
    Csv,
    Unsupported,
    Ambiguous,
    UnresolvedChange,
    Comment,
    InvalidSignature,
    UnverifiableSignature,
}

#[derive(Default)]
struct FakeRepository {
    files: Mutex<HashMap<FileId, FileObject>>,
    inspections: Mutex<HashMap<FileId, SemanticInspectionRecord>>,
    snapshot: Mutex<Option<document_application::AuthoritativeDocument>>,
    mutations: Mutex<Vec<document_application::VersionMutationRecord>>,
}

impl VersioningRepository for FakeRepository {
    async fn get_version_operation(
        &self,
        _: document_application::VersionOperationId,
    ) -> Result<Option<document_application::VersionOperationRecord>, RepositoryError> {
        Ok(None)
    }
    async fn get_version_snapshot(
        &self,
        _: document_domain::DocumentId,
        _: document_domain::DocumentVersionId,
    ) -> Result<Option<document_application::AuthoritativeDocument>, RepositoryError> {
        Ok(self.snapshot.lock().unwrap().clone())
    }
    async fn update_working(
        &self,
        record: document_application::VersionMutationRecord,
    ) -> Result<document_application::VersionOperationResult, RepositoryError> {
        self.mutations.lock().unwrap().push(record);
        Err(RepositoryError::CommitOutcomeUnknown)
    }

    async fn register_file_object(&self, file: FileObject) -> Result<(), RepositoryError> {
        let mut files = self.files.lock().unwrap();
        if let Some(existing) = files.get(&file.file_id()) {
            if existing.content_hash() != file.content_hash()
                || existing.size_bytes() != file.size_bytes()
                || existing.media_type() != file.media_type()
                || existing.storage_key() != file.storage_key()
            {
                return Err(RepositoryError::IntegrityViolation);
            }
            return Ok(());
        }
        files.insert(file.file_id(), file);
        Ok(())
    }
}

impl SemanticInspectionRepository for FakeRepository {
    async fn get_file_object(
        &self,
        file_id: FileId,
    ) -> Result<Option<FileObject>, RepositoryError> {
        Ok(self.files.lock().unwrap().get(&file_id).cloned())
    }

    async fn get_semantic_inspection(
        &self,
        file_id: FileId,
        _: InspectionProfileVersion,
    ) -> Result<Option<SemanticInspectionRecord>, RepositoryError> {
        Ok(self.inspections.lock().unwrap().get(&file_id).cloned())
    }

    async fn insert_or_converge_semantic_inspection(
        &self,
        record: SemanticInspectionRecord,
    ) -> Result<SemanticInspectionRecord, RepositoryError> {
        let mut inspections = self.inspections.lock().unwrap();
        Ok(inspections
            .entry(record.file_id())
            .or_insert(record)
            .clone())
    }
}

#[derive(Default)]
struct FakeStorage {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

impl FileStorage for FakeStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let (file_id, mut content, media_type) = request.into_parts();
        let mut bytes = Vec::new();
        content
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| StorageError::WriteFailed)?;
        let hash = [bytes.first().copied().unwrap_or(0); 32];
        let key = StorageKey::new(format!("objects/{}", file_id.as_uuid())).unwrap();
        self.objects
            .lock()
            .unwrap()
            .insert(key.as_str().to_owned(), bytes.clone());
        Ok(StoredFile::new(
            key,
            ContentHash::from_slice(&hash).unwrap(),
            FileSize::new(bytes.len() as i64).unwrap(),
            media_type,
        ))
    }

    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        let bytes = self
            .objects
            .lock()
            .unwrap()
            .get(key.as_str())
            .cloned()
            .ok_or(StorageError::NotFound)?;
        Ok(Box::pin(Cursor::new(bytes)))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}

struct FakeExecutor {
    mode: Mutex<Mode>,
    calls: AtomicUsize,
}

impl SemanticInspectionExecutor for FakeExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        mut content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut bytes = Vec::new();
        content.read_to_end(&mut bytes).await.unwrap();
        let mode = *self.mode.lock().unwrap();
        if matches!(mode, Mode::Unsupported) {
            return Err(InspectionExecutionError::UnsupportedDocumentFormat);
        }
        if matches!(mode, Mode::Ambiguous) {
            return Err(InspectionExecutionError::ParserDisagreement);
        }
        let changes = if matches!(mode, Mode::UnresolvedChange) {
            vec![
                serde_json::json!({"kind":"insert","author_label":null,"timestamp":null,"source_locator":"body/1","unresolved":true}),
            ]
        } else {
            vec![]
        };
        let comments = if matches!(mode, Mode::Comment) {
            vec![
                serde_json::json!({"author_label":null,"timestamp":null,"resolved_state":"resolved","source_locator":"body/1","content":"review"}),
            ]
        } else {
            vec![]
        };
        let signatures = match mode {
            Mode::InvalidSignature | Mode::UnverifiableSignature => vec![serde_json::json!({
                "signature_type":"test", "signer_claim":null, "certificate_subject":null,
                "certificate_issuer":null, "certificate_fingerprint":null, "signed_at":null,
                "cryptographic_validity": if matches!(mode, Mode::InvalidSignature) {"invalid"} else {"unverifiable"},
                "covered_content":[], "validation_diagnostics":[]
            })],
            _ => vec![],
        };
        let digest = [bytes.first().copied().unwrap_or(0); 32];
        Ok(serde_json::from_value(serde_json::json!({
            "protocol_version":"dsi-worker-v0", "inspection_profile_version":"dsi-v0",
            "observed_raw_content_hash":request.expected_raw_content_hash,
            "observed_size_bytes":request.expected_size_bytes,
            "detected_format": if matches!(mode, Mode::Csv) { "csv" } else { "txt" }, "semantic_fingerprint":{"algorithm":"sha256","digest":digest},
            "semantic_capabilities":[],
            "editorial_provenance":{"tracked_changes":changes,"comments":comments,"document_author_labels":[],"last_modified_by":null,"modification_metadata":{}},
            "external_dependencies":[], "digital_signature_evidence":signatures,
            "extractor_provenance":{"worker_build_id":"fake","adapter_id":"txt","adapter_version":"1","parser_libraries":[],"native_dependency_identity":[]},
            "diagnostics":[]
        })).unwrap())
    }
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }
}

struct Fixture {
    service: VersioningPreflight<FakeRepository, FakeStorage, FakeExecutor, FixedClock>,
    repository: Arc<FakeRepository>,
    storage: Arc<FakeStorage>,
    executor: Arc<FakeExecutor>,
}

fn fixture(mode: Mode) -> Fixture {
    let repository = Arc::new(FakeRepository::default());
    let storage = Arc::new(FakeStorage::default());
    let executor = Arc::new(FakeExecutor {
        mode: Mutex::new(mode),
        calls: AtomicUsize::new(0),
    });
    let service = VersioningPreflight::new(
        repository.clone(),
        storage.clone(),
        executor.clone(),
        Arc::new(FixedClock),
    );
    Fixture {
        service,
        repository,
        storage,
        executor,
    }
}

#[tokio::test]
async fn preflight_inspects_every_authoritative_item_and_excludes_renditions_from_identity() {
    let with_rendition = fixture(Mode::Plain);
    let prepared = with_rendition
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![
                input("primary", 0, 1, 7).with_renditions(vec![rendition(3, 9)]),
                input("annex", 1, 2, 8),
            ],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    assert_eq!(with_rendition.executor.calls.load(Ordering::SeqCst), 2);
    assert_eq!(prepared.manifest().items().len(), 2);
    assert_eq!(prepared.items()[0].renditions().len(), 1);
    with_rendition
        .service
        .check_publish_quality(&prepared)
        .await
        .unwrap();

    let without_rendition = fixture(Mode::Plain);
    let same_authority = without_rendition
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 1, 7), input("annex", 1, 2, 8)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.manifest().identity_digest(),
        same_authority.manifest().identity_digest()
    );
}

#[tokio::test]
async fn preflight_fails_closed_on_binding_mismatch_and_unsupported_inspection() {
    let first = fixture(Mode::Plain);
    first
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 1, 7)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    let mismatch = first
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 1, 8)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap_err();
    assert_eq!(mismatch, ApplicationError::IntegrityViolation);
    assert_eq!(first.executor.calls.load(Ordering::SeqCst), 1);

    let unsupported = fixture(Mode::Unsupported);
    let error = unsupported
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 4, 7)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ApplicationError::InspectionFailed(InspectionExecutionError::UnsupportedDocumentFormat)
    );
    assert_eq!(unsupported.repository.files.lock().unwrap().len(), 1);
    assert!(
        unsupported
            .repository
            .inspections
            .lock()
            .unwrap()
            .is_empty()
    );

    let ambiguous = fixture(Mode::Ambiguous);
    let error = ambiguous
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 8, 7)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ApplicationError::InspectionFailed(InspectionExecutionError::ParserDisagreement)
    );
}

#[tokio::test]
async fn preflight_leaves_registered_orphan_when_later_version_write_fails() {
    let fixture = fixture(Mode::Plain);
    fixture
        .service
        .prepare(
            Title::new("Policy").unwrap(),
            vec![input("primary", 0, 5, 7)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    assert_eq!(fixture.repository.files.lock().unwrap().len(), 1);
    assert_eq!(fixture.storage.objects.lock().unwrap().len(), 1);
    // No Version command has committed: reconciliation sees the persisted unreferenced FileObject.
}

#[tokio::test]
async fn publish_quality_allows_unsigned_and_rejects_unresolved_editorial_or_invalid_signatures() {
    for (mode, accepted) in [
        (Mode::Plain, true),
        (Mode::UnresolvedChange, false),
        (Mode::Comment, false),
        (Mode::InvalidSignature, false),
        (Mode::UnverifiableSignature, false),
    ] {
        let fixture = fixture(mode);
        let prepared = fixture
            .service
            .prepare(
                Title::new("Policy").unwrap(),
                vec![input("primary", 0, 6, 7)],
                InspectionProfileVersion::DsiV0,
            )
            .await
            .unwrap();
        let result = fixture.service.check_publish_quality(&prepared).await;
        if accepted {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(ApplicationError::PublishQualityRejected(_))
            ));
        }
    }
}

impl document_application::DocumentRepository for FakeRepository {
    async fn create_initial_document(
        &self,
        _: document_application::CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        unreachable!()
    }
    async fn get_authoritative_document(
        &self,
        _: document_domain::DocumentId,
    ) -> Result<Option<document_application::AuthoritativeDocument>, RepositoryError> {
        Ok(self.snapshot.lock().unwrap().clone())
    }
    async fn get_authoring_document(
        &self,
        _: document_domain::DocumentId,
    ) -> Result<Option<document_application::AuthoritativeDocument>, RepositoryError> {
        unreachable!()
    }
    async fn get_current_published_document(
        &self,
        _: document_domain::DocumentId,
    ) -> Result<Option<document_application::AuthoritativeDocument>, RepositoryError> {
        unreachable!()
    }
    async fn is_current_published_version(
        &self,
        _: document_domain::DocumentId,
        _: document_domain::DocumentVersionId,
    ) -> Result<bool, RepositoryError> {
        unreachable!()
    }
    async fn list_current_published_versions(
        &self,
        _: Option<document_domain::DocumentId>,
        _: i64,
    ) -> Result<Vec<document_application::CurrentPublishedVersionRef>, RepositoryError> {
        unreachable!()
    }
    async fn file_reference_exists(&self, _: FileId) -> Result<bool, RepositoryError> {
        unreachable!()
    }
    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        unreachable!()
    }
}

struct TestIds;
impl document_application::IdGenerator for TestIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

fn initial_update_fixture() -> (
    Fixture,
    document_domain::DocumentId,
    document_domain::DocumentVersionId,
    document_domain::PrincipalRef,
) {
    use document_application::AuthoritativeDocument;
    use document_domain::{
        CreateInitialDocument, DocumentId, DocumentVersionId, FolderId, InitialDocument, Metadata,
        PrincipalRef, StoredFileDescriptor,
    };
    let f = fixture(Mode::Plain);
    let document_id = DocumentId::from_uuid(Uuid::from_u128(100));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(101));
    let actor = PrincipalRef::new("test", "editor").unwrap();
    let initial = InitialDocument::create(CreateInitialDocument {
        document_id,
        version_id,
        file_id: file_id(50),
        folder_id: FolderId::from_uuid(Uuid::from_u128(102)),
        title: Title::new("Initial").unwrap(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: actor.clone(),
        stored_file: StoredFileDescriptor::new(
            StorageKey::new("objects/initial").unwrap(),
            ContentHash::from_slice(&[7; 32]).unwrap(),
            FileSize::new(3).unwrap(),
            MediaType::new("text/plain").unwrap(),
        ),
        original_filename: "initial.txt".into(),
        created_at: OffsetDateTime::UNIX_EPOCH,
    })
    .unwrap();
    let snapshot = AuthoritativeDocument::from_initial(initial);
    f.repository
        .files
        .lock()
        .unwrap()
        .insert(snapshot.file().file_id(), snapshot.file().clone());
    f.storage
        .objects
        .lock()
        .unwrap()
        .insert("objects/initial".into(), vec![7; 3]);
    *f.repository.snapshot.lock().unwrap() = Some(snapshot);
    (f, document_id, version_id, actor)
}

#[tokio::test]
async fn initial_working_update_accepts_null_base_and_same_semantics_after_fresh_preflight() {
    use document_application::{
        DocumentVersionService, UpdateWorkingVersionCommand, VersionOperationId,
    };
    let (f, document_id, version_id, actor) = initial_update_fixture();
    let prepared = f
        .service
        .prepare(
            Title::new("Initial").unwrap(),
            vec![input("primary", 0, 51, 7)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    assert_eq!(
        f.executor.calls.load(Ordering::SeqCst),
        1,
        "new file must be inspected even when semantics are unchanged"
    );
    // An initial original without prior evidence can be repaired even if its old
    // bytes are unavailable. Only the submitted originals must inspect.
    f.storage.objects.lock().unwrap().remove("objects/initial");
    let operation_id = VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        Arc::new(FixedClock),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
    );
    let result = service
        .update_working(
            UpdateWorkingVersionCommand::new(operation_id, document_id, version_id, 0, actor)
                .unwrap(),
            prepared,
        )
        .await;
    assert_eq!(
        result,
        Err(ApplicationError::VersionCommitOutcomeUnknown {
            operation_id,
            document_id,
            document_version_id: version_id
        }),
        "the initial update must reach the atomic mutation and preserve its recovery identity"
    );
    let records = f.repository.mutations.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].identity().target_version_id(), version_id);
}

#[tokio::test]
async fn initial_working_update_rejects_authority_format_migration_without_semantic_no_change_rule()
{
    use document_application::{
        DocumentVersionService, UpdateWorkingVersionCommand, VersionOperationId,
    };
    let (f, document_id, version_id, actor) = initial_update_fixture();
    let snapshot = f.repository.snapshot.lock().unwrap().clone().unwrap();
    f.service.inspect_existing(&snapshot).await.unwrap();
    // Synthetic raw-bound cached candidate evidence isolates the DSI format
    // comparison: both immutable FileObjects still declare text/plain.
    let mut response = f
        .repository
        .inspections
        .lock()
        .unwrap()
        .get(&file_id(50))
        .unwrap()
        .response()
        .clone();
    response.detected_format = document_semantic_inspection_core::FormatId::Csv;
    response.observed_raw_content_hash = [8; 32];
    let candidate_inspection =
        SemanticInspectionRecord::restore(file_id(52), response, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    f.repository
        .inspections
        .lock()
        .unwrap()
        .insert(file_id(52), candidate_inspection);
    let prepared = f
        .service
        .prepare(
            Title::new("Changed").unwrap(),
            vec![input("primary", 0, 52, 8)],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.items()[0].file().media_type(),
        snapshot.file().media_type()
    );
    assert_ne!(
        prepared.items()[0].inspection().response().detected_format,
        f.repository
            .inspections
            .lock()
            .unwrap()
            .get(&file_id(50))
            .unwrap()
            .response()
            .detected_format
    );
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        Arc::new(FixedClock),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
    );
    let operation_id = VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let result = service
        .update_working(
            UpdateWorkingVersionCommand::new(operation_id, document_id, version_id, 0, actor)
                .unwrap(),
            prepared,
        )
        .await;
    assert_eq!(result, Err(ApplicationError::BusinessRule));
    assert!(f.repository.mutations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn initial_working_update_rejects_media_type_change_without_old_inspection() {
    use document_application::{
        DocumentVersionService, UpdateWorkingVersionCommand, VersionOperationId,
    };
    let (f, document_id, version_id, actor) = initial_update_fixture();
    f.storage.objects.lock().unwrap().remove("objects/initial");
    *f.executor.mode.lock().unwrap() = Mode::Csv;
    let prepared = f
        .service
        .prepare(
            Title::new("Changed").unwrap(),
            vec![VersioningItemInput::new(
                LogicalPath::new("primary").unwrap(),
                0,
                file_id(52),
                MediaType::new("text/csv").unwrap(),
                "changed.csv",
                Box::pin(Cursor::new(vec![8; 3])),
            )],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        Arc::new(FixedClock),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
    );
    let operation_id = VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let result = service
        .update_working(
            UpdateWorkingVersionCommand::new(operation_id, document_id, version_id, 0, actor)
                .unwrap(),
            prepared,
        )
        .await;
    assert_eq!(result, Err(ApplicationError::BusinessRule));
    assert!(f.repository.mutations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn initial_working_update_rejects_stored_inspection_binding_mismatch() {
    use document_application::{
        DocumentVersionService, UpdateWorkingVersionCommand, VersionOperationId,
    };
    for mismatched_hash in [true, false] {
        let (f, document_id, version_id, actor) = initial_update_fixture();
        let snapshot = f.repository.snapshot.lock().unwrap().clone().unwrap();
        f.service.inspect_existing(&snapshot).await.unwrap();
        let old = f
            .repository
            .inspections
            .lock()
            .unwrap()
            .get(&file_id(50))
            .unwrap()
            .clone();
        let mut response = old.response().clone();
        if mismatched_hash {
            response.observed_raw_content_hash = [99; 32];
        } else {
            response.observed_size_bytes += 1;
        }
        let corrupt =
            SemanticInspectionRecord::restore(file_id(50), response, old.inspected_at()).unwrap();
        f.repository
            .inspections
            .lock()
            .unwrap()
            .insert(file_id(50), corrupt);
        let prepared = f
            .service
            .prepare(
                Title::new("Changed").unwrap(),
                vec![input("primary", 0, 53, 8)],
                InspectionProfileVersion::DsiV0,
            )
            .await
            .unwrap();
        let service = DocumentVersionService::new(
            Arc::new(TestIds),
            Arc::new(FixedClock),
            f.storage.clone(),
            f.executor.clone(),
            f.repository.clone(),
        );
        let command = UpdateWorkingVersionCommand::new(
            VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
            document_id,
            version_id,
            0,
            actor,
        )
        .unwrap();
        assert_eq!(
            service.update_working(command, prepared).await,
            Err(ApplicationError::IntegrityViolation)
        );
        assert!(f.repository.mutations.lock().unwrap().is_empty());
    }
}
