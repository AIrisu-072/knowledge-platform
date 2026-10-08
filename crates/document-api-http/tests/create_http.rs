use std::future::Future;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::create::{create_router, create_router_with_limits};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::limits::{
    MAX_FILENAME_BYTES, MAX_JSON_BODY_BYTES, MAX_MULTIPART_FILE_BYTES, MAX_MULTIPART_HEADER_BYTES,
    MAX_MULTIPART_JSON_BYTES, MAX_MULTIPART_PARTS, MAX_MULTIPART_TOTAL_BYTES, UploadLimits,
};
use document_application::{
    AuthoritativeDocument, AuthorizationScope, Clock, ContentReader, CreateInitialDocumentRecord,
    CreateOutcomeProbe, CreateOutcomeRepository, CurrentPublishedVersionRef, DocumentRepository,
    FileStorage, IdGenerator, IdentityResolutionError, InvocationKind, RepositoryError,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile, VerifiedActorContext,
};
use document_domain::{
    ContentHash, DocumentId, DocumentVersionId, FileId, FileSize, PolicySubject, PolicySubjectKind,
    PrincipalRef, StorageKey,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use tokio::io::AsyncReadExt;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct FixedIdentity(VerifiedActorContext);

impl IdentityAdapter for FixedIdentity {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

fn actor() -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", "author-1").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "author-1").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

struct GeneratedIds;

impl IdGenerator for GeneratedIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }
}

#[derive(Default)]
struct StorageState {
    bytes: Vec<u8>,
    media_type: String,
    put_calls: usize,
}

#[derive(Default)]
struct RecordingStorage(Mutex<StorageState>);

impl FileStorage for RecordingStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let (file_id, mut content, media_type) = request.into_parts();
        let mut bytes = Vec::new();
        content
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| StorageError::Internal(error.to_string()))?;
        let mut state = self.0.lock().unwrap();
        state.put_calls += 1;
        state.bytes = bytes;
        state.media_type = media_type.as_str().to_owned();
        Ok(StoredFile::new(
            StorageKey::new(format!("objects/{file_id:?}")).unwrap(),
            ContentHash::from_slice(&[7; 32]).unwrap(),
            FileSize::new(state.bytes.len() as i64).unwrap(),
            media_type,
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(Cursor::new(self.0.lock().unwrap().bytes.clone())))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct RepositoryState {
    document: Option<AuthoritativeDocument>,
    create_calls: usize,
    ambiguous_commit: bool,
}

#[derive(Clone, Default)]
struct RecordingRepository {
    state: Arc<Mutex<RepositoryState>>,
    scoped_actor: Option<VerifiedActorContext>,
}

impl RecordingRepository {
    fn ambiguous() -> Self {
        Self {
            state: Arc::new(Mutex::new(RepositoryState {
                ambiguous_commit: true,
                ..RepositoryState::default()
            })),
            scoped_actor: None,
        }
    }

    fn create_calls(&self) -> usize {
        self.state.lock().unwrap().create_calls
    }
}

impl AuthorizationScope for RecordingRepository {
    fn with_verified_actor(&self, ctx: VerifiedActorContext) -> Self {
        Self {
            state: self.state.clone(),
            scoped_actor: Some(ctx),
        }
    }
}

impl DocumentRepository for RecordingRepository {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        let actor = self
            .scoped_actor
            .as_ref()
            .ok_or(RepositoryError::Forbidden)?;
        if record.authoritative().version().created_by() != actor.principal() {
            return Err(RepositoryError::Forbidden);
        }
        let mut state = self.state.lock().unwrap();
        state.create_calls += 1;
        state.document = Some(record.authoritative().clone());
        if state.ambiguous_commit {
            Err(RepositoryError::CommitOutcomeUnknown)
        } else {
            Ok(())
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

    async fn get_authoring_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        self.get_authoritative_document(id).await
    }

    async fn get_current_published_document(
        &self,
        _id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(None)
    }

    async fn is_current_published_version(
        &self,
        _document_id: DocumentId,
        _version_id: DocumentVersionId,
    ) -> Result<bool, RepositoryError> {
        Ok(false)
    }

    async fn list_current_published_versions(
        &self,
        _after: Option<DocumentId>,
        _limit: i64,
    ) -> Result<Vec<CurrentPublishedVersionRef>, RepositoryError> {
        Ok(Vec::new())
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

impl CreateOutcomeRepository for RecordingRepository {
    async fn recover_initial_create(
        &self,
        ctx: &VerifiedActorContext,
        probe: CreateOutcomeProbe,
    ) -> Result<bool, RepositoryError> {
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        Ok(self
            .state
            .lock()
            .unwrap()
            .document
            .as_ref()
            .is_some_and(|document| {
                document.document().document_id() == probe.document_id
                    && document.version().document_version_id() == probe.document_version_id
                    && document.file().file_id() == probe.file_id
                    && document.version().created_by() == ctx.principal()
            }))
    }
}

#[derive(Clone)]
struct Part {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    extra_header: Option<String>,
    body: Vec<u8>,
}

impl Part {
    fn request(body: Vec<u8>) -> Self {
        Self {
            name: "request".into(),
            filename: None,
            content_type: Some("application/json".into()),
            extra_header: None,
            body,
        }
    }

    fn file(body: Vec<u8>) -> Self {
        Self {
            name: "file".into(),
            filename: Some("policy.txt".into()),
            content_type: Some("text/plain".into()),
            extra_header: None,
            body,
        }
    }
}

fn create_request_json() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "folderId": Uuid::from_u128(700),
        "title": "Initial policy",
        "documentMetadata": {"category": "policy"},
        "versionMetadata": {"source": "upload"}
    }))
    .unwrap()
}

fn multipart(parts: &[Part], terminated: bool) -> (String, Vec<u8>) {
    let boundary = "knowledge-platform-boundary";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let filename = part
            .filename
            .as_ref()
            .map(|value| format!("; filename=\"{value}\""))
            .unwrap_or_default();
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"{filename}\r\n",
                part.name
            )
            .as_bytes(),
        );
        if let Some(content_type) = &part.content_type {
            body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
        }
        if let Some(value) = &part.extra_header {
            body.extend_from_slice(format!("X-Padding: {value}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&part.body);
        body.extend_from_slice(b"\r\n");
    }
    if terminated {
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    }
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn send(router: Router, parts: &[Part], terminated: bool) -> (StatusCode, Value) {
    let (content_type, body) = multipart(parts, terminated);
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/documents")
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "expected JSON response: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

fn router(repository: Arc<RecordingRepository>, storage: Arc<RecordingStorage>) -> Router {
    create_router(
        Arc::new(GeneratedIds),
        Arc::new(FixedClock),
        storage,
        repository,
        Arc::new(FixedIdentity(actor())),
    )
    .unwrap()
}

fn constrained_router(
    repository: Arc<RecordingRepository>,
    storage: Arc<RecordingStorage>,
) -> Router {
    let limits = UploadLimits::tightened(512, 8, 4096, 8, 16, 256).unwrap();
    router_with_limits(repository, storage, limits)
}

fn router_with_limits(
    repository: Arc<RecordingRepository>,
    storage: Arc<RecordingStorage>,
    limits: UploadLimits,
) -> Router {
    create_router_with_limits(
        Arc::new(GeneratedIds),
        Arc::new(FixedClock),
        storage,
        repository,
        Arc::new(FixedIdentity(actor())),
        limits,
    )
    .unwrap()
}

fn encoded_header_bytes(part: &Part) -> usize {
    let filename = part
        .filename
        .as_ref()
        .map(|value| format!("; filename=\"{value}\""))
        .unwrap_or_default();
    let disposition = format!("form-data; name=\"{}\"{filename}", part.name);
    let mut bytes = "content-disposition".len() + disposition.len() + 4;
    if let Some(content_type) = &part.content_type {
        bytes += "content-type".len() + content_type.len() + 4;
    }
    if let Some(value) = &part.extra_header {
        bytes += "x-extra".len() + value.len() + 4;
    }
    bytes
}

#[test]
fn production_upload_profile_is_finite_and_fixed() {
    assert_eq!(MAX_MULTIPART_JSON_BYTES, 1024 * 1024);
    assert_eq!(MAX_MULTIPART_FILE_BYTES, 256 * 1024 * 1024);
    assert_eq!(MAX_MULTIPART_TOTAL_BYTES, 1024 * 1024 * 1024);
    assert_eq!(MAX_MULTIPART_PARTS, 64);
    assert_eq!(MAX_FILENAME_BYTES, 1024);
    assert_eq!(MAX_MULTIPART_HEADER_BYTES, 32 * 1024);
    assert!(
        UploadLimits::tightened(
            MAX_MULTIPART_JSON_BYTES + 1,
            MAX_MULTIPART_FILE_BYTES,
            MAX_MULTIPART_TOTAL_BYTES,
            MAX_MULTIPART_PARTS,
            MAX_FILENAME_BYTES,
            MAX_MULTIPART_HEADER_BYTES,
        )
        .is_err()
    );
}

#[tokio::test]
async fn tightened_multipart_limits_accept_exact_boundaries_and_reject_one_over() {
    let request = Part::request(create_request_json());
    let file = Part::file(vec![7; 8]);
    let parts = [request, file];
    let total_bytes = multipart(&parts, true).1.len();
    let json_bytes = parts[0].body.len();
    let file_bytes = parts[1].body.len();
    let filename_bytes = parts[1].filename.as_ref().unwrap().len();
    let header_bytes = parts.iter().map(encoded_header_bytes).max().unwrap();
    let exact = UploadLimits::tightened(
        json_bytes,
        file_bytes,
        total_bytes,
        parts.len(),
        filename_bytes,
        header_bytes,
    )
    .unwrap();
    let repository = Arc::new(RecordingRepository::default());
    let (status, body) = send(
        router_with_limits(
            repository.clone(),
            Arc::new(RecordingStorage::default()),
            exact,
        ),
        &parts,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(repository.create_calls(), 1);

    let one_over_profiles = [
        UploadLimits::tightened(
            json_bytes - 1,
            file_bytes,
            total_bytes,
            parts.len(),
            filename_bytes,
            header_bytes,
        )
        .unwrap(),
        UploadLimits::tightened(
            json_bytes,
            file_bytes - 1,
            total_bytes,
            parts.len(),
            filename_bytes,
            header_bytes,
        )
        .unwrap(),
        UploadLimits::tightened(
            json_bytes,
            file_bytes,
            total_bytes - 1,
            parts.len(),
            filename_bytes,
            header_bytes,
        )
        .unwrap(),
        UploadLimits::tightened(
            json_bytes,
            file_bytes,
            total_bytes,
            parts.len() - 1,
            filename_bytes,
            header_bytes,
        )
        .unwrap(),
        UploadLimits::tightened(
            json_bytes,
            file_bytes,
            total_bytes,
            parts.len(),
            filename_bytes - 1,
            header_bytes,
        )
        .unwrap(),
        UploadLimits::tightened(
            json_bytes,
            file_bytes,
            total_bytes,
            parts.len(),
            filename_bytes,
            header_bytes - 1,
        )
        .unwrap(),
    ];
    for limits in one_over_profiles {
        let repository = Arc::new(RecordingRepository::default());
        let (status, body) = send(
            router_with_limits(
                repository.clone(),
                Arc::new(RecordingStorage::default()),
                limits,
            ),
            &parts,
            true,
        )
        .await;
        assert!(status.is_client_error(), "{status} {body}");
        assert_eq!(repository.create_calls(), 0);
    }
}

#[tokio::test]
async fn multipart_profile_accepts_a_representative_file_larger_than_the_json_limit() {
    let repository = Arc::new(RecordingRepository::default());
    let storage = Arc::new(RecordingStorage::default());
    let file = vec![5; MAX_JSON_BODY_BYTES + 1];
    let (status, body) = send(
        router(repository.clone(), storage.clone()),
        &[Part::request(create_request_json()), Part::file(file)],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(repository.create_calls(), 1);
    assert_eq!(
        storage.0.lock().unwrap().bytes.len(),
        MAX_JSON_BODY_BYTES + 1
    );
}

#[tokio::test]
async fn valid_create_streams_primary_file_and_authorized_recovery_matches_all_ids() {
    let repository = Arc::new(RecordingRepository::default());
    let storage = Arc::new(RecordingStorage::default());
    let api = router(repository.clone(), storage.clone());
    let (status, created) = send(
        api.clone(),
        &[
            Part::file(b"authoritative content".to_vec()),
            Part::request(create_request_json()),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(repository.create_calls(), 1);
    {
        let stored = storage.0.lock().unwrap();
        assert_eq!(stored.bytes, b"authoritative content");
        assert_eq!(stored.media_type, "text/plain");
    }

    let recovery_uri = format!(
        "/v1/document-creation-outcomes/{}?documentVersionId={}&fileId={}",
        created["documentId"].as_str().unwrap(),
        created["documentVersionId"].as_str().unwrap(),
        created["fileId"].as_str().unwrap()
    );
    let response = api
        .clone()
        .oneshot(
            Request::builder()
                .uri(&recovery_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let recovered: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(recovered, created);

    let wrong = format!(
        "/v1/document-creation-outcomes/{}?documentVersionId={}&fileId={}",
        created["documentId"].as_str().unwrap(),
        created["documentVersionId"].as_str().unwrap(),
        Uuid::now_v7()
    );
    let response = api
        .oneshot(Request::builder().uri(wrong).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_multipart_shape_media_and_limits_fail_before_create_commit() {
    let cases = [
        (
            "missing request",
            vec![Part::file(vec![1])],
            "VALIDATION_FAILED",
        ),
        (
            "missing file",
            vec![Part::request(create_request_json())],
            "VALIDATION_FAILED",
        ),
        (
            "duplicate request",
            vec![
                Part::request(create_request_json()),
                Part::request(create_request_json()),
                Part::file(vec![1]),
            ],
            "VALIDATION_FAILED",
        ),
        (
            "duplicate file",
            vec![
                Part::request(create_request_json()),
                Part::file(vec![1]),
                Part::file(vec![2]),
            ],
            "VALIDATION_FAILED",
        ),
        (
            "unknown part",
            vec![
                Part::request(create_request_json()),
                Part::file(vec![1]),
                Part {
                    name: "actor".into(),
                    filename: None,
                    content_type: Some("text/plain".into()),
                    extra_header: None,
                    body: vec![1],
                },
            ],
            "VALIDATION_FAILED",
        ),
        (
            "oversized metadata",
            vec![
                Part::request(serde_json::to_vec(&json!({"padding": "x".repeat(520)})).unwrap()),
                Part::file(vec![1]),
            ],
            "VALIDATION_FAILED",
        ),
        (
            "oversized filename",
            vec![
                Part::request(create_request_json()),
                Part {
                    filename: Some("x".repeat(17)),
                    ..Part::file(vec![1])
                },
            ],
            "VALIDATION_FAILED",
        ),
        (
            "oversized file",
            vec![Part::request(create_request_json()), Part::file(vec![1; 9])],
            "VALIDATION_FAILED",
        ),
        (
            "oversized part header",
            vec![
                Part::request(create_request_json()),
                Part {
                    extra_header: Some("x".repeat(300)),
                    ..Part::file(vec![1])
                },
            ],
            "VALIDATION_FAILED",
        ),
        (
            "invalid media type",
            vec![
                Part::request(create_request_json()),
                Part {
                    content_type: Some("not-a-media-type".into()),
                    ..Part::file(vec![1])
                },
            ],
            "UNSUPPORTED_MEDIA_TYPE",
        ),
    ];

    for (name, parts, code) in cases {
        let repository = Arc::new(RecordingRepository::default());
        let (status, problem) = send(
            constrained_router(repository.clone(), Arc::new(RecordingStorage::default())),
            &parts,
            true,
        )
        .await;
        assert!(
            status.is_client_error(),
            "{name}: expected client error, got {status} {problem}"
        );
        assert_eq!(problem["code"], code, "{name}: {problem}");
        assert_eq!(repository.create_calls(), 0, "{name}");
    }
}

#[tokio::test]
async fn disconnect_before_commit_is_closed_and_unknown_commit_has_no_blind_retry_signal() {
    let disconnected_repository = Arc::new(RecordingRepository::default());
    let (status, problem) = send(
        constrained_router(
            disconnected_repository.clone(),
            Arc::new(RecordingStorage::default()),
        ),
        &[
            Part::request(create_request_json()),
            Part::file(b"partial".to_vec()),
        ],
        false,
    )
    .await;
    assert!(status.is_client_error(), "{problem}");
    assert_eq!(problem["code"], "VALIDATION_FAILED");
    assert_eq!(disconnected_repository.create_calls(), 0);

    let ambiguous_repository = Arc::new(RecordingRepository::ambiguous());
    let api = constrained_router(
        ambiguous_repository.clone(),
        Arc::new(RecordingStorage::default()),
    );
    let (status, problem) = send(
        api.clone(),
        &[
            Part::request(create_request_json()),
            Part::file(b"content".to_vec()),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{problem}");
    assert_eq!(problem["code"], "COMMIT_OUTCOME_UNKNOWN");
    assert_eq!(problem["retryable"], false);
    assert!(problem.get("exactRetry").is_none());
    assert_eq!(ambiguous_repository.create_calls(), 1);

    let recovery = &problem["recovery"];
    let recovery_uri = format!(
        "{}?documentVersionId={}&fileId={}",
        recovery["recoveryEndpoint"].as_str().unwrap(),
        recovery["documentVersionId"].as_str().unwrap(),
        recovery["fileId"].as_str().unwrap()
    );
    let response = api
        .oneshot(
            Request::builder()
                .uri(recovery_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(ambiguous_repository.create_calls(), 1);
}

#[tokio::test]
async fn initial_manifest_binary_wire_creates_two_originals() {
    let repository = Arc::new(RecordingRepository::default());
    let storage = Arc::new(RecordingStorage::default());
    let app = router(repository.clone(), storage.clone());
    let mut request: Value = serde_json::from_slice(&create_request_json()).unwrap();
    request["items"] = json!([
        {"logicalPath":"a.txt","ordinal":0,"partId":"a","mediaType":"text/plain","originalFilename":"a.txt"},
        {"logicalPath":"b.txt","ordinal":1,"partId":"b","mediaType":"text/plain","originalFilename":"b.txt"}
    ]);
    let boundary = "initial-multiple";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\nContent-Type: application/json\r\n\r\n{request}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\nX-Part-Id: a\r\n\r\nfirst\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"b.txt\"\r\nX-Part-Id: b\r\n\r\nsecond\r\n--{boundary}--\r\n"
    );
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/documents")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let result: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(result["fileIds"].as_array().unwrap().len(), 2);
    assert_eq!(result["fileId"], result["fileIds"][0]);
    assert_eq!(storage.0.lock().unwrap().put_calls, 2);
    assert_eq!(
        repository
            .state
            .lock()
            .unwrap()
            .document
            .as_ref()
            .unwrap()
            .content_items()
            .len(),
        2
    );
}

#[tokio::test]
async fn null_initial_manifest_is_not_legacy_creation() {
    let repository = Arc::new(RecordingRepository::default());
    let storage = Arc::new(RecordingStorage::default());
    let mut request: Value = serde_json::from_slice(&create_request_json()).unwrap();
    request["items"] = Value::Null;
    let (status, _) = send(
        router(repository.clone(), storage.clone()),
        &[
            Part::request(serde_json::to_vec(&request).unwrap()),
            Part::file(b"bytes".to_vec()),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(repository.create_calls(), 0);
    assert_eq!(storage.0.lock().unwrap().put_calls, 0);
}

async fn send_initial_manifest(
    app: Router,
    request: Value,
    binaries: &[(&str, &str)],
) -> (StatusCode, Value) {
    let boundary = "initial-manifest-errors";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\nContent-Type: application/json\r\n\r\n{request}\r\n"
    );
    for (part, bytes) in binaries {
        body.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files\"\r\nX-Part-Id: {part}\r\n\r\n{bytes}\r\n"));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/documents")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let result =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    (status, result)
}

#[tokio::test]
async fn invalid_initial_manifest_bindings_never_write_storage_or_database() {
    let mut base: Value = serde_json::from_slice(&create_request_json()).unwrap();
    base["items"] = json!([
        {"logicalPath":"a.txt","ordinal":0,"partId":"a","mediaType":"text/plain","originalFilename":"a.txt"},
        {"logicalPath":"b.txt","ordinal":1,"partId":"b","mediaType":"text/plain","originalFilename":"b.txt"}
    ]);
    let mut cases = vec![
        (base.clone(), vec![("a", "first")]),
        (
            base.clone(),
            vec![("a", "first"), ("b", "second"), ("extra", "unreferenced")],
        ),
        (base.clone(), vec![("a", "first"), ("a", "second")]),
    ];
    for (pointer, value) in [
        ("/items", json!([])),
        ("/items/1/partId", json!("a")),
        ("/items/1/logicalPath", json!("../invalid")),
        ("/items/0/mediaType", json!("invalid")),
        ("/items/0/originalFilename", json!(" ")),
    ] {
        let mut request = base.clone();
        *request.pointer_mut(pointer).unwrap() = value;
        cases.push((request, vec![("a", "first"), ("b", "second")]));
    }
    let mut duplicate_anchor = base.clone();
    duplicate_anchor["items"][1]["logicalPath"] = json!("a.txt");
    duplicate_anchor["items"][1]["ordinal"] = json!(0);
    cases.push((duplicate_anchor, vec![("a", "first"), ("b", "second")]));
    for (request, binaries) in cases {
        let repository = Arc::new(RecordingRepository::default());
        let storage = Arc::new(RecordingStorage::default());
        let (status, _) = send_initial_manifest(
            router(repository.clone(), storage.clone()),
            request.clone(),
            &binaries,
        )
        .await;
        assert!(
            status == StatusCode::UNPROCESSABLE_ENTITY
                || status == StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unexpected {status} for {request}"
        );
        assert_eq!(repository.create_calls(), 0);
        assert_eq!(storage.0.lock().unwrap().put_calls, 0);
    }
}

#[tokio::test]
async fn initial_manifest_unknown_response_has_complete_receipt() {
    let mut request: Value = serde_json::from_slice(&create_request_json()).unwrap();
    request["items"] = json!([
        {"logicalPath":"a.txt","ordinal":0,"partId":"a","mediaType":"text/plain","originalFilename":"a.txt"},
        {"logicalPath":"b.txt","ordinal":1,"partId":"b","mediaType":"text/plain","originalFilename":"b.txt"}
    ]);
    let (status, result) = send_initial_manifest(
        router(
            Arc::new(RecordingRepository::ambiguous()),
            Arc::new(RecordingStorage::default()),
        ),
        request,
        &[("a", "first"), ("b", "second")],
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(result["recovery"]["fileIds"].as_array().unwrap().len(), 2);
    assert_eq!(
        result["recovery"]["fileId"],
        result["recovery"]["fileIds"][0]
    );
}
