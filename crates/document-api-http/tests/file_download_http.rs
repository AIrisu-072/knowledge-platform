use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use document_api_http::file_download::{
    file_download_router, file_download_router_with_idle_timeout,
};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_application::{
    AuditedFileGrant, ContentReader, FileStorage, IdentityResolutionError, InvocationKind,
    RepositoryError, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
    VerifiedActorContext, VersionFileAccessRepository, VersionFileRequest, VersionPurpose,
};
use document_domain::{MediaType, PolicySubject, PolicySubjectKind, PrincipalRef, StorageKey};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tokio::io::{AsyncRead, ReadBuf};
use tower::ServiceExt;
use uuid::Uuid;

const FILE_BYTES: &[u8] = b"abcdef";

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

fn context() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test-idp", "reader").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "reader").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

#[derive(Clone, Copy)]
enum RepositoryMode {
    Grant = 0,
    Forbidden = 1,
    Missing = 2,
    Unavailable = 3,
    CommitUnknown = 4,
}

#[derive(Default)]
struct FakeRepository {
    mode: AtomicU8,
    calls: AtomicUsize,
    events: Arc<Mutex<Vec<&'static str>>>,
    last_request: Mutex<Option<VersionFileRequest>>,
}

impl FakeRepository {
    fn set_mode(&self, mode: RepositoryMode) {
        self.mode.store(mode as u8, Ordering::SeqCst);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl VersionFileAccessRepository for FakeRepository {
    async fn authorize_and_audit_file(
        &self,
        _ctx: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push("audit");
        *self.last_request.lock().unwrap() = Some(request);
        match self.mode.load(Ordering::SeqCst) {
            x if x == RepositoryMode::Grant as u8 => Ok(AuditedFileGrant::new(
                StorageKey::new("private/storage/object-1").unwrap(),
                MediaType::new("text/plain").unwrap(),
                FILE_BYTES.len() as i64,
                "..\\report\"\r\nX-Evil: injected.txt".into(),
                Uuid::now_v7(),
            )),
            x if x == RepositoryMode::Forbidden as u8 => Err(RepositoryError::Forbidden),
            x if x == RepositoryMode::Missing as u8 => Err(RepositoryError::FileObjectNotFound),
            x if x == RepositoryMode::Unavailable as u8 => Err(RepositoryError::Unavailable),
            x if x == RepositoryMode::CommitUnknown as u8 => {
                Err(RepositoryError::CommitOutcomeUnknown)
            }
            _ => unreachable!(),
        }
    }
}

struct RecordingReader {
    bytes: &'static [u8],
    offset: usize,
    recorded: bool,
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct PendingReader;

impl AsyncRead for PendingReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncRead for RecordingReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if !this.recorded {
            this.events.lock().unwrap().push("read");
            this.recorded = true;
        }
        let count = buffer.remaining().min(this.bytes.len() - this.offset);
        buffer.put_slice(&this.bytes[this.offset..this.offset + count]);
        this.offset += count;
        Poll::Ready(Ok(()))
    }
}

#[derive(Default)]
struct CountingStorage {
    fail: AtomicBool,
    stall: AtomicBool,
    opens: AtomicUsize,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FileStorage for CountingStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push("open");
        if self.fail.load(Ordering::SeqCst) {
            return Err(StorageError::Unavailable);
        }
        if self.stall.load(Ordering::SeqCst) {
            return Ok(Box::pin(PendingReader));
        }
        Ok(Box::pin(RecordingReader {
            bytes: FILE_BYTES,
            offset: 0,
            recorded: false,
            events: self.events.clone(),
        }))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn stalled_download_body_fails_after_the_finite_idle_budget() {
    let repository = Arc::new(FakeRepository::default());
    let storage = Arc::new(CountingStorage {
        stall: AtomicBool::new(true),
        events: repository.events.clone(),
        ..CountingStorage::default()
    });
    let router = file_download_router_with_idle_timeout(
        repository.clone(),
        storage,
        Arc::new(FixedIdentity(context())),
        std::time::Duration::from_millis(20),
    )
    .unwrap();
    let response = request(router, &uri(ids(), "history"), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(to_bytes(response.into_body(), 1024).await.is_err());
    assert_eq!(*repository.events.lock().unwrap(), vec!["audit", "open"]);
}

fn ids() -> (Uuid, Uuid, Uuid, Uuid) {
    (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    )
}

fn uri(ids: (Uuid, Uuid, Uuid, Uuid), purpose: &str) -> String {
    let (document, version, item, representation) = ids;
    format!(
        "/v1/documents/{document}/versions/{version}/files/{item}/{representation}?purpose={purpose}"
    )
}

async fn request(router: axum::Router, uri: &str, range: Option<&str>) -> axum::response::Response {
    let mut builder = Request::builder().uri(uri);
    if let Some(range) = range {
        builder = builder.header(header::RANGE, range);
    }
    router
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn problem(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(
        !bytes
            .windows(FILE_BYTES.len())
            .any(|window| window == FILE_BYTES)
    );
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn audited_stream_sets_safe_headers_and_delays_body_read() {
    let repository = Arc::new(FakeRepository::default());
    let storage = Arc::new(CountingStorage {
        events: repository.events.clone(),
        ..CountingStorage::default()
    });
    let router = file_download_router(
        repository.clone(),
        storage.clone(),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    let request_ids = ids();
    let response = request(router, &uri(request_ids, "history"), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "6");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let disposition = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap();
    assert!(disposition.starts_with("attachment; filename=\""));
    assert!(!disposition.contains(['\r', '\n', '\\']));
    assert!(response.headers().get("x-evil").is_none());
    assert!(!format!("{:?}", response.headers()).contains("private/storage"));
    assert_eq!(*repository.events.lock().unwrap(), vec!["audit", "open"]);

    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], FILE_BYTES);
    assert_eq!(
        *repository.events.lock().unwrap(),
        vec!["audit", "open", "read"]
    );
    let captured = repository.last_request.lock().unwrap().unwrap();
    assert_eq!(captured.version.document_id.as_uuid(), request_ids.0);
    assert_eq!(
        captured.version.document_version_id.as_uuid(),
        request_ids.1
    );
    assert_eq!(captured.content_item_id, request_ids.2);
    assert_eq!(captured.representation_id, request_ids.3);
    assert_eq!(captured.version.purpose, VersionPurpose::History);
    assert_eq!(storage.opens.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn range_audit_and_storage_failures_never_stream_file_bytes() {
    let repository = Arc::new(FakeRepository::default());
    let storage = Arc::new(CountingStorage {
        events: repository.events.clone(),
        ..CountingStorage::default()
    });
    let router = file_download_router(
        repository.clone(),
        storage.clone(),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    let location = uri(ids(), "published");

    let ranged = request(router.clone(), &location, Some("bytes=0-1")).await;
    assert_eq!(ranged.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert!(to_bytes(ranged.into_body(), 1024).await.unwrap().is_empty());
    assert_eq!(repository.calls(), 0);
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);

    repository.set_mode(RepositoryMode::CommitUnknown);
    let (status, body) = problem(request(router.clone(), &location, None).await).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "COMMIT_OUTCOME_UNKNOWN");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);

    repository.set_mode(RepositoryMode::Unavailable);
    let (status, body) = problem(request(router.clone(), &location, None).await).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "DEPENDENCY_UNAVAILABLE");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);

    repository.set_mode(RepositoryMode::Forbidden);
    let (status, body) = problem(request(router.clone(), &location, None).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "FORBIDDEN");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);

    repository.set_mode(RepositoryMode::Missing);
    let (status, body) = problem(request(router.clone(), &location, None).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "DOCUMENT_VERSION_NOT_FOUND");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);

    repository.set_mode(RepositoryMode::Grant);
    storage.fail.store(true, Ordering::SeqCst);
    let (status, body) = problem(request(router, &location, None).await).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "DEPENDENCY_UNAVAILABLE");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 1);
}
