//! Task 10 RED: the Application owns cache order and worker-result validation.

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, Clock, ContentReader, EnsureSemanticInspection, FileStorage,
    InspectionExecutionError, RepositoryError, SemanticInspectionExecutor,
    SemanticInspectionRecord, SemanticInspectionRepository, StorageError, StorageObjectInfo,
    StoreFileRequest, StoredFile,
};
use document_domain::{
    ContentHash, FileId, FileObject, FileSize, MediaType, StorageKey, StoredFileDescriptor,
};
use document_semantic_inspection_core::{
    CapabilityEvidence, CapabilityState, FormatId, InspectionProfileVersion, WorkerRequest,
    WorkerResponse,
};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const INPUT: &[u8] = b"input-vector";
const HASH: [u8; 32] = [
    0x3d, 0xaf, 0x69, 0x8f, 0x18, 0x25, 0xd9, 0x14, 0xec, 0x34, 0xac, 0x9f, 0x0f, 0xab, 0xf6, 0xe7,
    0xe3, 0x32, 0x78, 0x2c, 0xb3, 0xe2, 0x1c, 0x5f, 0xa0, 0x00, 0xd6, 0x6a, 0xee, 0x5b, 0x29, 0x1d,
];

fn file_id() -> FileId {
    FileId::from_uuid(Uuid::from_u128(1))
}

fn file_object(hash: [u8; 32]) -> FileObject {
    FileObject::restore(
        file_id(),
        StoredFileDescriptor::new(
            StorageKey::new("authoritative/document.bin").unwrap(),
            ContentHash::from_slice(&hash).unwrap(),
            FileSize::new(INPUT.len() as i64).unwrap(),
            MediaType::new("text/plain").unwrap(),
        ),
        OffsetDateTime::UNIX_EPOCH,
    )
}

fn response() -> WorkerResponse {
    serde_json::from_value(serde_json::json!({
        "protocol_version": "dsi-worker-v0",
        "inspection_profile_version": "dsi-v0",
        "observed_raw_content_hash": HASH,
        "observed_size_bytes": INPUT.len(),
        "detected_format": "txt",
        "semantic_fingerprint": {"algorithm": "sha256", "digest": vec![0u8; 32]},
        "semantic_capabilities": [],
        "editorial_provenance": {
            "tracked_changes": [], "comments": [], "document_author_labels": [],
            "last_modified_by": null, "modification_metadata": {}
        },
        "external_dependencies": [],
        "digital_signature_evidence": [],
        "extractor_provenance": {
            "worker_build_id": "task10-test", "adapter_id": "text", "adapter_version": "0",
            "parser_libraries": [], "native_dependency_identity": []
        },
        "diagnostics": []
    }))
    .unwrap()
}

type Events = Arc<Mutex<Vec<&'static str>>>;

struct FakeRepository {
    file: Mutex<FileObject>,
    cached: Mutex<Option<SemanticInspectionRecord>>,
    events: Events,
}

impl SemanticInspectionRepository for FakeRepository {
    async fn get_file_object(
        &self,
        requested_file_id: FileId,
    ) -> Result<Option<FileObject>, RepositoryError> {
        self.events.lock().unwrap().push("file");
        assert_eq!(requested_file_id, file_id());
        Ok(Some(self.file.lock().unwrap().clone()))
    }

    async fn get_semantic_inspection(
        &self,
        requested_file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> Result<Option<SemanticInspectionRecord>, RepositoryError> {
        self.events.lock().unwrap().push("cache");
        assert_eq!(requested_file_id, file_id());
        assert_eq!(profile, InspectionProfileVersion::DsiV0);
        Ok(self.cached.lock().unwrap().clone())
    }

    async fn insert_or_converge_semantic_inspection(
        &self,
        record: SemanticInspectionRecord,
    ) -> Result<SemanticInspectionRecord, RepositoryError> {
        self.events.lock().unwrap().push("insert");
        let mut cached = self.cached.lock().unwrap();
        assert!(cached.is_none(), "first inspection should insert once");
        *cached = Some(record.clone());
        Ok(record)
    }
}

struct FakeStorage {
    events: Events,
}

impl FileStorage for FakeStorage {
    async fn put_immutable(&self, _: StoreFileRequest) -> Result<StoredFile, StorageError> {
        panic!("inspection must not write authoritative storage")
    }

    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        self.events.lock().unwrap().push("open");
        assert_eq!(key.as_str(), "authoritative/document.bin");
        Ok(Box::pin(Cursor::new(INPUT.to_vec())))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        panic!("inspection must not list storage")
    }
}

struct FakeExecutor {
    events: Events,
    response: Mutex<WorkerResponse>,
}

impl SemanticInspectionExecutor for FakeExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        mut content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        self.events.lock().unwrap().push("inspect");
        assert_eq!(
            request.inspection_profile_version,
            InspectionProfileVersion::DsiV0
        );
        assert_eq!(request.declared_media_type, "text/plain");
        assert_eq!(request.expected_raw_content_hash, HASH);
        assert_eq!(request.expected_size_bytes, INPUT.len() as u64);
        let mut bytes = Vec::new();
        content.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, INPUT);
        Ok(self.response.lock().unwrap().clone())
    }
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }
}

struct Fixture {
    service: EnsureSemanticInspection<FakeRepository, FakeStorage, FakeExecutor, FixedClock>,
    repository: Arc<FakeRepository>,
    executor: Arc<FakeExecutor>,
    events: Events,
}

fn fixture() -> Fixture {
    let events = Arc::new(Mutex::new(Vec::new()));
    let repository = Arc::new(FakeRepository {
        file: Mutex::new(file_object(HASH)),
        cached: Mutex::new(None),
        events: events.clone(),
    });
    let storage = Arc::new(FakeStorage {
        events: events.clone(),
    });
    let executor = Arc::new(FakeExecutor {
        events: events.clone(),
        response: Mutex::new(response()),
    });
    let service = EnsureSemanticInspection::new(
        repository.clone(),
        storage,
        executor.clone(),
        Arc::new(FixedClock),
    );
    Fixture {
        service,
        repository,
        executor,
        events,
    }
}

#[tokio::test]
async fn miss_runs_in_order_and_hit_uses_only_authoritative_metadata_and_cache() {
    let fixture = fixture();
    let first = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap();
    assert_eq!(first.file_id(), file_id());
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["file", "cache", "open", "inspect", "insert"]
    );

    fixture.events.lock().unwrap().clear();
    let second = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(*fixture.events.lock().unwrap(), ["file", "cache"]);
}

#[tokio::test]
async fn cache_raw_binding_change_is_integrity_violation_without_reinspection() {
    let fixture = fixture();
    fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap();
    *fixture.repository.file.lock().unwrap() = file_object([0x7a; 32]);
    fixture.events.lock().unwrap().clear();

    let error = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::IntegrityViolation);
    assert_eq!(*fixture.events.lock().unwrap(), ["file", "cache"]);
}

#[tokio::test]
async fn invalid_worker_binding_never_reaches_persistence() {
    let fixture = fixture();
    fixture
        .executor
        .response
        .lock()
        .unwrap()
        .observed_raw_content_hash = [0x7a; 32];
    let error = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::IntegrityViolation);
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["file", "cache", "open", "inspect"]
    );
    assert!(fixture.repository.cached.lock().unwrap().is_none());
}

#[tokio::test]
async fn incompatible_detected_format_never_reaches_persistence() {
    let fixture = fixture();
    fixture.executor.response.lock().unwrap().detected_format = FormatId::Pdf;

    let error = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::InvalidWorkerResult);
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["file", "cache", "open", "inspect"]
    );
}

#[tokio::test]
async fn duplicate_capability_and_blank_provenance_fail_before_insert() {
    for mutate in [
        (|result: &mut WorkerResponse| {
            let capability = CapabilityEvidence {
                capability_id: "visible_text".into(),
                presence: CapabilityState::Present,
                version_significant: true,
                equivalence_fingerprint: None,
            };
            result.semantic_capabilities = vec![capability.clone(), capability];
        }) as fn(&mut WorkerResponse),
        |result: &mut WorkerResponse| result.extractor_provenance.worker_build_id.clear(),
    ] {
        let fixture = fixture();
        mutate(&mut fixture.executor.response.lock().unwrap());
        let error = fixture
            .service
            .ensure(file_id(), InspectionProfileVersion::DsiV0)
            .await
            .unwrap_err();
        assert_eq!(error, ApplicationError::InvalidWorkerResult);
        assert!(fixture.repository.cached.lock().unwrap().is_none());
    }
}

#[tokio::test]
async fn oversized_diagnostics_fail_before_insert() {
    let fixture = fixture();
    fixture.executor.response.lock().unwrap().diagnostics.push(
        document_semantic_inspection_core::Diagnostic {
            code: "oversized".into(),
            message: "x".repeat(16 * 1024 * 1024 + 1),
        },
    );
    let error = fixture
        .service
        .ensure(file_id(), InspectionProfileVersion::DsiV0)
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::InvalidWorkerResult);
    assert!(fixture.repository.cached.lock().unwrap().is_none());
}
