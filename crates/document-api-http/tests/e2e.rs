use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(not(target_os = "linux"))]
use std::io::Cursor;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::api::{DocumentApiRouters, compose_document_api};
use document_api_http::create::create_router;
use document_api_http::diff::diff_router;
use document_api_http::file_download::file_download_router;
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::management::management_router;
use document_api_http::publication::publication_router;
use document_api_http::read::read_router;
use document_api_http::versioning::versioning_router;
use document_application::document_diff::{DiffExecutionError, DiffExecutor};
use document_application::{
    BootstrapRootPolicy, Clock, ContentReader, EnsureSemanticInspection, IdGenerator,
    IdentityResolutionError, InspectionExecutionError, InvocationKind, SemanticInspectionExecutor,
    VerifiedActorContext,
};
use document_diff_core::{WorkerDiffRequest, WorkerDiffResponse};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef};
use document_repository_postgres::{PostgresDocumentRepository, migrate};
use document_semantic_inspection_core::{WorkerRequest, WorkerResponse};
use document_storage_fs::FileSystemStorage;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tempfile::TempDir;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

#[cfg(not(target_os = "linux"))]
use tokio::io::AsyncReadExt;

struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }
}

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

fn actor() -> PrincipalRef {
    PrincipalRef::new("test-idp", "acceptance-admin").unwrap()
}

fn actor_context() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        actor(),
        vec![
            PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "acceptance-admin")
                .unwrap(),
        ],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn full_grant() -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "acceptance-admin").unwrap(),
        [
            Action::Read,
            Action::ReadHistory,
            Action::Write,
            Action::Publish,
            Action::Administer,
        ],
    )
    .unwrap()
}

#[cfg(not(target_os = "linux"))]
struct AcceptanceInspectionExecutor;

#[cfg(target_os = "linux")]
struct AcceptanceInspectionExecutor(document_semantic_inspection_runner::RunnerInspectionExecutor);

impl SemanticInspectionExecutor for AcceptanceInspectionExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        #[cfg(target_os = "linux")]
        {
            self.0.inspect(request, content).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut content = content;
            let mut bytes = Vec::new();
            content
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| InspectionExecutionError::ExtractorUnavailable)?;
            let request = serde_json::to_vec(&request)
                .map_err(|_| InspectionExecutionError::InvalidWorkerResult)?;
            let mut output = Vec::new();
            let mut error = Vec::new();
            let status = document_semantic_inspection_worker::run_worker_shell(
                &request,
                &mut Cursor::new(bytes),
                &mut output,
                &mut error,
                64 * 1024,
                256 * 1024 * 1024,
            );
            if status != 0 {
                return Err(InspectionExecutionError::InvalidWorkerResult);
            }
            serde_json::from_slice(&output)
                .map_err(|_| InspectionExecutionError::InvalidWorkerResult)
        }
    }
}

#[cfg(not(target_os = "linux"))]
struct AcceptanceDiffExecutor;

#[cfg(target_os = "linux")]
struct AcceptanceDiffExecutor(document_diff_runner::RunnerDiffExecutor);

impl DiffExecutor for AcceptanceDiffExecutor {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        base: ContentReader,
        target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        #[cfg(target_os = "linux")]
        {
            self.0.compare(request, base, target).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut base = base;
            let mut target = target;
            let mut base_bytes = Vec::new();
            let mut target_bytes = Vec::new();
            base.read_to_end(&mut base_bytes)
                .await
                .map_err(|_| DiffExecutionError::Unavailable)?;
            target
                .read_to_end(&mut target_bytes)
                .await
                .map_err(|_| DiffExecutionError::Unavailable)?;
            document_diff_worker::run_worker_shell(
                &serde_json::to_vec(&request)
                    .map_err(|_| DiffExecutionError::InvalidWorkerResult)?,
                Cursor::new(base_bytes),
                Cursor::new(target_bytes),
            )
            .map_err(|_| DiffExecutionError::InvalidWorkerResult)
        }
    }
}

fn inspection_executor() -> AcceptanceInspectionExecutor {
    #[cfg(target_os = "linux")]
    {
        let worker = std::env::var_os("DSI_WORKER_BIN")
            .expect("DSI_WORKER_BIN must point to the built production worker");
        let mut config = document_semantic_inspection_runner::RunnerConfig::new(worker);
        if let Some(path) = std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH") {
            config = config.with_pdfium_runtime_dir(path);
        }
        AcceptanceInspectionExecutor(
            document_semantic_inspection_runner::RunnerInspectionExecutor::new(config).unwrap(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        AcceptanceInspectionExecutor
    }
}

fn diff_executor() -> AcceptanceDiffExecutor {
    #[cfg(target_os = "linux")]
    {
        let worker = std::env::var_os("DIFF_WORKER_BIN")
            .expect("DIFF_WORKER_BIN must point to the built production worker");
        AcceptanceDiffExecutor(
            document_diff_runner::RunnerDiffExecutor::new(document_diff_runner::RunnerConfig::new(
                worker,
            ))
            .unwrap(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        AcceptanceDiffExecutor
    }
}

struct Fixture {
    _postgres: testcontainers::ContainerAsync<GenericImage>,
    _storage_root: TempDir,
    pool: sqlx::PgPool,
    api: Router,
}

async fn fixture() -> Fixture {
    let postgres = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "document_http_acceptance")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let host = std::env::var("TESTCONTAINERS_HOST_OVERRIDE").unwrap_or_else(|_| "127.0.0.1".into());
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&format!(
            "postgres://postgres:postgres@{host}:{port}/document_http_acceptance"
        ))
        .await
        .unwrap();
    migrate(&pool).await.unwrap();

    let context = actor_context();
    PostgresDocumentRepository::new_with_bootstrap_actor(pool.clone(), actor())
        .initialize_root_policy(&context, vec![full_grant()])
        .await
        .unwrap();
    let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
    let storage_root = TempDir::new().unwrap();
    let storage = Arc::new(FileSystemStorage::new(storage_root.path()));
    let ids = Arc::new(UuidV7Ids);
    let clock = Arc::new(FixedClock);
    let inspection = Arc::new(inspection_executor());
    let diff = Arc::new(diff_executor());
    let identity: Arc<dyn IdentityAdapter> = Arc::new(FixedIdentity(context));
    let evidence = Arc::new(EnsureSemanticInspection::new(
        repository.clone(),
        storage.clone(),
        inspection.clone(),
        clock.clone(),
    ));

    let api = compose_document_api(DocumentApiRouters::new(
        read_router(repository.clone(), identity.clone()).unwrap(),
        management_router(repository.clone(), identity.clone()).unwrap(),
        create_router(
            ids.clone(),
            clock.clone(),
            storage.clone(),
            repository.clone(),
            identity.clone(),
        )
        .unwrap(),
        versioning_router(
            ids.clone(),
            clock.clone(),
            storage.clone(),
            inspection.clone(),
            repository.clone(),
            identity.clone(),
        )
        .unwrap(),
        publication_router(
            ids,
            clock,
            storage.clone(),
            inspection,
            repository.clone(),
            identity.clone(),
        )
        .unwrap(),
        file_download_router(repository.clone(), storage.clone(), identity.clone()).unwrap(),
        diff_router(repository, storage, diff, evidence, identity).unwrap(),
    ));

    Fixture {
        _postgres: postgres,
        _storage_root: storage_root,
        pool,
        api,
    }
}

#[derive(Clone)]
struct Part {
    name: &'static str,
    filename: Option<&'static str>,
    content_type: &'static str,
    part_id: Option<&'static str>,
    body: Vec<u8>,
}

fn multipart(parts: &[Part]) -> (String, Vec<u8>) {
    let boundary = "document-http-acceptance";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let filename = part
            .filename
            .map(|value| format!("; filename=\"{value}\""))
            .unwrap_or_default();
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"{filename}\r\n",
                part.name
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {}\r\n", part.content_type).as_bytes());
        if let Some(part_id) = part.part_id {
            body.extend_from_slice(format!("X-Part-Id: {part_id}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&part.body);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn send_json(
    api: &Router,
    method: Method,
    uri: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method.clone()).uri(uri);
    let body = if let Some(value) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(value).unwrap())
    } else {
        Body::empty()
    };
    let response = api
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "expected JSON response for {method} {uri}: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

async fn send_multipart(
    api: &Router,
    method: Method,
    uri: &str,
    parts: &[Part],
) -> (StatusCode, Value) {
    let (content_type, body) = multipart(parts);
    let response = api
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "expected multipart JSON response: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

fn request_part(value: Value) -> Part {
    Part {
        name: "request",
        filename: None,
        content_type: "application/json",
        part_id: None,
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn initial_file(bytes: &[u8]) -> Part {
    Part {
        name: "file",
        filename: Some("initial.txt"),
        content_type: "text/plain",
        part_id: None,
        body: bytes.to_vec(),
    }
}

fn version_file(part_id: &'static str, bytes: &[u8]) -> Part {
    Part {
        name: "files",
        filename: Some("version.txt"),
        content_type: "text/plain",
        part_id: Some(part_id),
        body: bytes.to_vec(),
    }
}

fn version_request(
    operation_id: Uuid,
    target_version_id: Uuid,
    expected_revision: i64,
    file_id: Uuid,
    title: &str,
) -> Value {
    json!({
        "operationId": operation_id,
        "targetVersionId": target_version_id,
        "expectedRevision": expected_revision,
        "title": title,
        "items": [{
            "logicalPath": "primary",
            "ordinal": 0,
            "fileId": file_id,
            "partId": "primary",
            "mediaType": "text/plain",
            "originalFilename": "version.txt",
            "renditions": []
        }]
    })
}

fn policy(operation_id: Uuid, revision: i64, subject: &str) -> Value {
    json!({
        "operationId": operation_id,
        "expectedPolicyRevision": revision,
        "reason": "HTTP acceptance policy transition",
        "mode": "explicit",
        "grants": [{
            "subjectKind": "principal",
            "identityProvider": "test-idp",
            "subjectId": subject,
            "actions": ["read", "readHistory", "write", "publish", "administer"]
        }]
    })
}

#[tokio::test]
async fn postgres_filesystem_and_workers_complete_the_document_http_journey() {
    let fixture = fixture().await;
    let api = &fixture.api;

    let (status, root) = send_json(api, Method::GET, "/v1/folders/root", None).await;
    assert_eq!(status, StatusCode::OK, "{root}");
    let root_id = root["folderId"].as_str().unwrap();
    let root_revision = root["revision"].as_i64().unwrap();

    let folder_id = Uuid::now_v7();
    let folder_operation = Uuid::now_v7();
    let folder_request = json!({
        "operationId": folder_operation,
        "folderId": folder_id,
        "parentFolderId": root_id,
        "expectedParentRevision": root_revision,
        "name": "Acceptance",
        "reason": "Create acceptance folder"
    });
    let (status, created_folder) =
        send_json(api, Method::POST, "/v1/folders", Some(&folder_request)).await;
    assert_eq!(status, StatusCode::CREATED, "{created_folder}");
    let (_, replayed_folder) =
        send_json(api, Method::POST, "/v1/folders", Some(&folder_request)).await;
    assert_eq!(replayed_folder, created_folder);
    let mut changed_target = folder_request.clone();
    changed_target["folderId"] = json!(Uuid::now_v7());
    let (status, conflict) =
        send_json(api, Method::POST, "/v1/folders", Some(&changed_target)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "OPERATION_CONFLICT");

    let children_uri = format!("/v1/folders/{root_id}/children?pageSize=100");
    let (status, children) = send_json(api, Method::GET, &children_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{children}");
    assert!(
        children["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["folderId"] == folder_id.to_string())
    );
    let folder_policy_uri = format!("/v1/folders/{folder_id}/access-policy");
    let (status, folder_policy) = send_json(api, Method::GET, &folder_policy_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{folder_policy}");
    let folder_policy_write = policy(
        Uuid::now_v7(),
        folder_policy["policyRevision"].as_i64().unwrap(),
        "acceptance-admin",
    );
    let (status, folder_policy_result) = send_json(
        api,
        Method::PUT,
        &folder_policy_uri,
        Some(&folder_policy_write),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{folder_policy_result}");

    let create_request = json!({
        "folderId": folder_id,
        "title": "Initial acceptance document",
        "documentMetadata": {"category": "acceptance"},
        "versionMetadata": {"source": "http"}
    });
    let (status, created) = send_multipart(
        api,
        Method::POST,
        "/v1/documents",
        &[
            request_part(create_request),
            initial_file(b"initial acceptance text\n"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let document_id = created["documentId"].as_str().unwrap();
    let initial_version_id = created["documentVersionId"].as_str().unwrap();

    let (status, authoring) =
        send_json(api, Method::GET, "/v1/documents?view=authoring", None).await;
    assert_eq!(status, StatusCode::OK, "{authoring}");
    let authoring_document = authoring["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["documentId"] == document_id && item["documentVersionId"] == initial_version_id
        })
        .unwrap();

    let initial_uri = format!("/v1/documents/{document_id}/versions/{initial_version_id}");
    let initial_publish = json!({
        "operationId": Uuid::now_v7(),
        "expectedRevision": authoring_document["revision"]
    });
    let (status, initial_published) = send_json(
        api,
        Method::POST,
        &format!("{initial_uri}:publish"),
        Some(&initial_publish),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{initial_published}");

    let metadata_operation = Uuid::now_v7();
    let metadata_uri = format!("/v1/documents/{document_id}/metadata");
    let metadata = json!({
        "operationId": metadata_operation,
        "expectedDocumentRevision": initial_published["resultingDocumentRevision"],
        "set": {"owning_department": "quality"},
        "unset": ["category"],
        "reason": "Create a metadata minor revision"
    });
    let (status, metadata_result) =
        send_json(api, Method::PATCH, &metadata_uri, Some(&metadata)).await;
    assert_eq!(status, StatusCode::OK, "{metadata_result}");

    let revisions_uri = format!("/v1/documents/{document_id}/revisions?pageSize=100");
    let (status, first_revision_history) = send_json(api, Method::GET, &revisions_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{first_revision_history}");
    let first_revision_id = first_revision_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|revision| revision["major"] == 1 && revision["minor"] == 0)
        .unwrap()["revisionId"]
        .as_str()
        .unwrap();
    let metadata_revision_id = first_revision_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|revision| revision["major"] == 1 && revision["minor"] == 1)
        .unwrap()["revisionId"]
        .as_str()
        .unwrap();
    let revision_comparison_uri = format!("/v1/documents/{document_id}/revision-comparisons");
    let metadata_comparison = json!({
        "baseRevisionId": first_revision_id,
        "targetRevisionId": metadata_revision_id,
        "projection": "comparisonTable"
    });
    let (status, metadata_diff) = send_json(
        api,
        Method::POST,
        &revision_comparison_uri,
        Some(&metadata_comparison),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{metadata_diff}");
    assert_eq!(
        metadata_diff["contentComparisonStatus"],
        "sameAuthoritativeVersion"
    );
    assert_eq!(metadata_diff["metadataComparisonStatus"], "different");
    assert!(
        !metadata_diff["metadataChanges"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let target_version_id = Uuid::now_v7();
    let create_version_operation = Uuid::now_v7();
    let create_version = version_request(
        create_version_operation,
        target_version_id,
        metadata_result["resultingRevision"].as_i64().unwrap(),
        Uuid::now_v7(),
        "Acceptance replacement",
    );
    let version_uri = format!("/v1/documents/{document_id}/versions");
    let create_version_parts = [
        request_part(create_version.clone()),
        version_file("primary", b"replacement acceptance text\n"),
    ];
    let (status, version_created) =
        send_multipart(api, Method::POST, &version_uri, &create_version_parts).await;
    assert_eq!(status, StatusCode::CREATED, "{version_created}");
    let (_, version_replay) =
        send_multipart(api, Method::POST, &version_uri, &create_version_parts).await;
    assert_eq!(version_replay, version_created);

    let update_version = version_request(
        Uuid::now_v7(),
        target_version_id,
        version_created["resultingRevision"].as_i64().unwrap(),
        Uuid::now_v7(),
        "Acceptance published version",
    );
    let target_uri = format!("/v1/documents/{document_id}/versions/{target_version_id}");
    let (status, version_updated) = send_multipart(
        api,
        Method::PUT,
        &target_uri,
        &[
            request_part(update_version),
            version_file("primary", b"published acceptance text\n"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{version_updated}");

    let publish_operation = Uuid::now_v7();
    let publish = json!({
        "operationId": publish_operation,
        "expectedRevision": version_updated["resultingRevision"]
    });
    let publish_uri = format!("{target_uri}:publish");
    let (status, published) = send_json(api, Method::POST, &publish_uri, Some(&publish)).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let (_, publish_replay) = send_json(api, Method::POST, &publish_uri, Some(&publish)).await;
    assert_eq!(publish_replay, published);

    let (status, major_revision_history) = send_json(api, Method::GET, &revisions_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{major_revision_history}");
    let major_revision_id = major_revision_history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|revision| revision["documentVersionId"] == target_version_id.to_string())
        .unwrap()["revisionId"]
        .as_str()
        .unwrap();
    let content_revision_comparison = json!({
        "baseRevisionId": first_revision_id,
        "targetRevisionId": major_revision_id,
        "projection": "comparisonTable"
    });
    let (status, revision_diff) = send_json(
        api,
        Method::POST,
        &revision_comparison_uri,
        Some(&content_revision_comparison),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{revision_diff}");
    assert_eq!(
        revision_diff["contentComparisonStatus"],
        "differentAuthoritativeVersions"
    );
    assert_eq!(revision_diff["verdict"], "different");
    assert_eq!(revision_diff["coverage"], "full");

    let published_uri = format!("/v1/documents/{document_id}?view=published");
    let (status, published_read) = send_json(api, Method::GET, &published_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{published_read}");
    assert_eq!(
        published_read["documentVersionId"],
        target_version_id.to_string()
    );
    let (status, versions) = send_json(
        api,
        Method::GET,
        &format!("{version_uri}?purpose=published"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{versions}");
    assert_eq!(
        versions["items"][0]["versionId"],
        target_version_id.to_string()
    );
    let (status, version_detail) = send_json(
        api,
        Method::GET,
        &format!("{target_uri}?purpose=published"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{version_detail}");
    assert_eq!(version_detail["versionId"], target_version_id.to_string());

    let read_state_uri = format!("{target_uri}/read-state");
    let (status, read_state) = send_json(api, Method::PUT, &read_state_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{read_state}");
    assert_eq!(read_state["inserted"], true);
    let (_, read_replay) = send_json(api, Method::PUT, &read_state_uri, None).await;
    assert_eq!(read_replay["inserted"], false);
    assert_eq!(read_replay["firstReadAt"], read_state["firstReadAt"]);

    let move_request = json!({
        "operationId": Uuid::now_v7(),
        "fromFolderId": folder_id,
        "toFolderId": root_id,
        "expectedDocumentRevision": published["resultingDocumentRevision"],
        "reason": "Move qualified document"
    });
    let move_uri = format!("/v1/documents/{document_id}:move");
    let (status, moved) = send_json(api, Method::POST, &move_uri, Some(&move_request)).await;
    assert_eq!(status, StatusCode::OK, "{moved}");

    let policy_uri = format!("/v1/documents/{document_id}/access-policy");
    let (status, current_policy) = send_json(api, Method::GET, &policy_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{current_policy}");
    let set_policy = policy(
        Uuid::now_v7(),
        current_policy["policyRevision"].as_i64().unwrap(),
        "acceptance-admin",
    );
    let (status, policy_result) = send_json(api, Method::PUT, &policy_uri, Some(&set_policy)).await;
    assert_eq!(status, StatusCode::OK, "{policy_result}");

    let history_uri = format!("/v1/documents/{document_id}/history?pageSize=100");
    let (status, history) = send_json(api, Method::GET, &history_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert!(!history["items"].as_array().unwrap().is_empty());

    let files_uri = format!("{target_uri}/files?purpose=published");
    let (status, files) = send_json(api, Method::GET, &files_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{files}");
    let file = &files["items"][0];
    let download_uri = format!(
        "{target_uri}/files/{}/{}?purpose=published",
        file["contentItemId"].as_str().unwrap(),
        file["representationId"].as_str().unwrap()
    );
    let response = api
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(&download_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let downloaded = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(downloaded.as_ref(), b"published acceptance text\n");

    let comparison_uri = format!("/v1/documents/{document_id}/comparisons");
    let comparison = json!({
        "baseVersionId": initial_version_id,
        "targetVersionId": target_version_id,
        "profile": "document-diff-v0",
        "projection": "comparisonTable"
    });
    let (status, diff) = send_json(api, Method::POST, &comparison_uri, Some(&comparison)).await;
    assert_eq!(status, StatusCode::OK, "{diff}");
    assert_eq!(diff["verdict"], "different");
    assert_eq!(diff["coverage"], "full");

    let withdraw_uri = format!("{target_uri}:withdraw");
    let withdraw = json!({
        "operationId": Uuid::now_v7(),
        "expectedRevision": moved["resultingRevision"],
        "reason": "Restore the preceding published version"
    });
    let (status, withdrawn) = send_json(api, Method::POST, &withdraw_uri, Some(&withdraw)).await;
    assert_eq!(status, StatusCode::OK, "{withdrawn}");
    assert_eq!(
        withdrawn["formerCurrentVersionId"],
        target_version_id.to_string()
    );
    assert_eq!(withdrawn["resultingCurrentVersionId"], initial_version_id);

    let (status, restored) = send_json(api, Method::GET, &published_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["documentVersionId"], initial_version_id);
    let (status, final_revision_history) = send_json(api, Method::GET, &revisions_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{final_revision_history}");
    let final_revisions = final_revision_history["items"].as_array().unwrap();
    let revision_numbers = final_revisions
        .iter()
        .map(|revision| {
            (
                revision["major"].as_i64().unwrap(),
                revision["minor"].as_i64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(revision_numbers, vec![(3, 0), (2, 0), (1, 1), (1, 0)]);
    assert_eq!(final_revisions[0]["sourceKind"], "withdrawFallback");
    assert_eq!(final_revisions[0]["documentVersionId"], initial_version_id);

    let revoke = policy(
        Uuid::now_v7(),
        policy_result["resultingRevision"].as_i64().unwrap(),
        "other-principal",
    );
    let (status, revoked) = send_json(api, Method::PUT, &policy_uri, Some(&revoke)).await;
    assert_eq!(status, StatusCode::OK, "{revoked}");

    for (method, uri, body) in [
        (Method::GET, published_uri.as_str(), None),
        (Method::GET, history_uri.as_str(), None),
        (
            Method::POST,
            revision_comparison_uri.as_str(),
            Some(&content_revision_comparison),
        ),
    ] {
        let (status, problem) = send_json(api, method, uri, body).await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "{uri}: {status} {problem}"
        );
    }

    let document_count: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(document_count, 1);
}
