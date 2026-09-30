#[path = "../../document-repository-postgres/tests/support/management.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::management::management_router;
use document_api_http::read::read_router;
use document_application::{
    BootstrapRootPolicy, IdentityResolutionError, InvocationKind, VerifiedActorContext,
};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::{Value, json};
use support::{Fixture, context, fixture};
use time::{Duration, OffsetDateTime};
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

fn actor_context(kind: InvocationKind) -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", "policy-admin").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        kind,
        None,
    )
    .unwrap()
}

fn full_grant() -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
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

async fn allow_all(fixture: &Fixture) {
    fixture
        .repository
        .initialize_root_policy(&context(), vec![full_grant()])
        .await
        .unwrap();
}

async fn json_request(
    router: Router,
    method: Method,
    uri: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(&method).uri(uri);
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(body).unwrap())
        }
        None => Body::empty(),
    };
    let response = router.oneshot(request.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "expected JSON response for {method} {uri}: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

fn request_operation() -> Uuid {
    Uuid::now_v7()
}

async fn create_folder(
    router: Router,
    operation_id: Uuid,
    folder_id: Uuid,
    parent_folder_id: Uuid,
    name: &str,
) -> Value {
    let body = json!({
        "operationId": operation_id,
        "folderId": folder_id,
        "parentFolderId": parent_folder_id,
        "expectedParentRevision": 0,
        "name": name,
        "reason": "HTTP management contract test"
    });
    let (status, response) = json_request(router, Method::POST, "/v1/folders", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    response
}

#[tokio::test]
async fn management_commands_preserve_replay_revision_and_typed_error_contracts() {
    let f = fixture().await;
    allow_all(&f).await;
    sqlx::query("UPDATE documents SET metadata = $1 WHERE document_id = $2")
        .bind(json!({"category": "old", "extensions": {"keep": true}}))
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();

    let identity: Arc<dyn IdentityAdapter> = Arc::new(FixedIdentity(context()));
    let router = management_router(f.repository.clone(), identity.clone()).unwrap();
    let read = read_router(f.repository.clone(), identity).unwrap();
    let document_id = f.document_id.as_uuid();
    let root_id = f.root_id.as_uuid();

    let metadata_operation = request_operation();
    let metadata = json!({
        "operationId": metadata_operation,
        "expectedDocumentRevision": 1,
        "set": {"owning_department": "operations"},
        "unset": ["category"],
        "reason": "Correct management metadata"
    });
    let metadata_uri = format!("/v1/documents/{document_id}/metadata");
    let (status, first_metadata) = json_request(
        router.clone(),
        Method::PATCH,
        &metadata_uri,
        Some(&metadata),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first_metadata}");
    assert_eq!(
        first_metadata["operationId"],
        metadata_operation.to_string()
    );
    assert_eq!(first_metadata["resourceId"], document_id.to_string());
    assert_eq!(first_metadata["resultingRevision"], 2);
    assert_eq!(first_metadata["changed"], true);

    let (status, replayed_metadata) = json_request(
        router.clone(),
        Method::PATCH,
        &metadata_uri,
        Some(&metadata),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replayed_metadata}");
    assert_eq!(replayed_metadata, first_metadata);
    let persisted_metadata: Value =
        sqlx::query_scalar("SELECT metadata FROM documents WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        persisted_metadata,
        json!({"extensions": {"keep": true}, "owning_department": "operations"})
    );

    let changed_replay = json!({
        "operationId": metadata_operation,
        "expectedDocumentRevision": 1,
        "set": {"owning_department": "different"},
        "unset": ["category"],
        "reason": "Correct management metadata"
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::PATCH,
        &metadata_uri,
        Some(&changed_replay),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "OPERATION_CONFLICT");

    let stale = json!({
        "operationId": request_operation(),
        "expectedDocumentRevision": 1,
        "set": {"category": "stale"},
        "unset": [],
        "reason": "Stale write"
    });
    let (status, problem) =
        json_request(router.clone(), Method::PATCH, &metadata_uri, Some(&stale)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "REVISION_CONFLICT");

    let folder_a = Uuid::now_v7();
    let folder_b = Uuid::now_v7();
    let create_a_operation = request_operation();
    let created_a = create_folder(
        router.clone(),
        create_a_operation,
        folder_a,
        root_id,
        "Operations",
    )
    .await;
    let replayed_a = create_folder(
        router.clone(),
        create_a_operation,
        folder_a,
        root_id,
        "Operations",
    )
    .await;
    assert_eq!(replayed_a, created_a);
    create_folder(
        router.clone(),
        request_operation(),
        folder_b,
        root_id,
        "Archive",
    )
    .await;

    let mismatched_retry = json!({
        "operationId": create_a_operation,
        "folderId": Uuid::now_v7(),
        "parentFolderId": root_id,
        "expectedParentRevision": 0,
        "name": "Operations",
        "reason": "HTTP management contract test"
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::POST,
        "/v1/folders",
        Some(&mismatched_retry),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "OPERATION_CONFLICT");

    let rename = json!({
        "operationId": request_operation(),
        "expectedFolderRevision": 0,
        "name": "Active Operations",
        "reason": "Rename folder"
    });
    let (status, renamed) = json_request(
        router.clone(),
        Method::PATCH,
        &format!("/v1/folders/{folder_a}"),
        Some(&rename),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["resultingRevision"], 1);

    let move_folder = json!({
        "operationId": request_operation(),
        "fromParentId": root_id,
        "toParentId": folder_b,
        "expectedFolderRevision": 1,
        "reason": "Archive folder"
    });
    let (status, moved) = json_request(
        router.clone(),
        Method::POST,
        &format!("/v1/folders/{folder_a}:move"),
        Some(&move_folder),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["resultingRevision"], 2);

    let cycle = json!({
        "operationId": request_operation(),
        "fromParentId": root_id,
        "toParentId": folder_a,
        "expectedFolderRevision": 0,
        "reason": "Invalid cycle"
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::POST,
        &format!("/v1/folders/{folder_b}:move"),
        Some(&cycle),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "FOLDER_CYCLE");

    let rename_root = json!({
        "operationId": request_operation(),
        "expectedFolderRevision": 0,
        "name": "Other Root",
        "reason": "Invalid root rename"
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::PATCH,
        &format!("/v1/folders/{root_id}"),
        Some(&rename_root),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "ROOT_PROTECTED");

    let move_document = json!({
        "operationId": request_operation(),
        "fromFolderId": root_id,
        "toFolderId": folder_b,
        "expectedDocumentRevision": 2,
        "reason": "File with owning team"
    });
    let (status, moved_document) = json_request(
        router.clone(),
        Method::POST,
        &format!("/v1/documents/{document_id}:move"),
        Some(&move_document),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved_document}");
    assert_eq!(moved_document["resultingRevision"], 3);
    let persisted_folder: Uuid =
        sqlx::query_scalar("SELECT folder_id FROM documents WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(persisted_folder, folder_b);

    let policy_uri = format!("/v1/documents/{document_id}/access-policy");
    let (status, current_policy) = json_request(read, Method::GET, &policy_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{current_policy}");
    assert_eq!(current_policy["policyRevision"], 0);

    let empty_explicit = json!({
        "operationId": request_operation(),
        "expectedPolicyRevision": 0,
        "reason": "Empty policies are invalid",
        "mode": "explicit",
        "grants": []
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::PUT,
        &policy_uri,
        Some(&empty_explicit),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "VALIDATION_FAILED");

    let policy = json!({
        "operationId": request_operation(),
        "expectedPolicyRevision": current_policy["policyRevision"],
        "reason": "Bind document policy",
        "mode": "explicit",
        "grants": [{
            "subjectKind": "principal",
            "identityProvider": "test-idp",
            "subjectId": "policy-admin",
            "actions": ["read", "readHistory", "write", "publish", "administer"]
        }]
    });
    let (status, policy_result) =
        json_request(router.clone(), Method::PUT, &policy_uri, Some(&policy)).await;
    assert_eq!(status, StatusCode::OK, "{policy_result}");
    assert_eq!(policy_result["resultingRevision"], 1);

    let stale_policy = json!({
        "operationId": request_operation(),
        "expectedPolicyRevision": 0,
        "reason": "Stale policy update",
        "mode": "inherit"
    });
    let (status, problem) = json_request(
        router.clone(),
        Method::PUT,
        &policy_uri,
        Some(&stale_policy),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "REVISION_CONFLICT");

    let reserved_version = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(reserved_version)
        .bind(document_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,3,4,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(Uuid::now_v7())
        .bind(document_id)
        .bind(reserved_version)
        .bind(vec![1_u8; 32])
        .execute(&f.pool)
        .await
        .unwrap();
    let reserved_update = json!({
        "operationId": request_operation(),
        "expectedDocumentRevision": 3,
        "set": {"category": "reserved"},
        "unset": [],
        "reason": "Rejected while scheduled"
    });
    let (status, problem) =
        json_request(router, Method::PATCH, &metadata_uri, Some(&reserved_update)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "RESERVED_DOCUMENT");
}

#[tokio::test]
async fn read_state_is_explicit_idempotent_and_human_interactive_only() {
    let f = fixture().await;
    allow_all(&f).await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Published',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();

    let uri = format!(
        "/v1/documents/{}/versions/{version_id}/read-state",
        f.document_id.as_uuid()
    );
    for kind in [InvocationKind::Agent, InvocationKind::Service] {
        let router = management_router(
            f.repository.clone(),
            Arc::new(FixedIdentity(actor_context(kind))),
        )
        .unwrap();
        let (status, problem) = json_request(router, Method::PUT, &uri, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{kind:?}: {problem}");
        assert_eq!(problem["code"], "FORBIDDEN");
    }

    let router = management_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(actor_context(
            InvocationKind::HumanInteractive,
        ))),
    )
    .unwrap();
    let (status, first) = json_request(router.clone(), Method::PUT, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["documentId"], f.document_id.as_uuid().to_string());
    assert_eq!(first["versionId"], version_id.to_string());
    assert_eq!(first["inserted"], true);

    let (status, replay) = json_request(router, Method::PUT, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["inserted"], false);
    assert_eq!(replay["firstReadAt"], first["firstReadAt"]);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM document_read_states")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn capability_snapshot_does_not_authorize_a_later_mutation_after_schedule_reservation() {
    let f = fixture().await;
    allow_all(&f).await;
    let published_id = Uuid::now_v7();
    let working_id = Uuid::now_v7();
    let publish_operation_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Published',now(),'test-idp','policy-admin','{}',now())")
        .bind(published_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(published_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();

    let identity: Arc<dyn IdentityAdapter> = Arc::new(FixedIdentity(context()));
    let read = read_router(f.repository.clone(), identity.clone()).unwrap();
    let management = management_router(f.repository.clone(), identity).unwrap();
    let document_id = f.document_id.as_uuid();
    let detail_uri = format!("/v1/documents/{document_id}?view=published");
    let (status, before) = json_request(read.clone(), Method::GET, &detail_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(
        before["capabilities"]["updateMetadata"]["status"],
        "available"
    );

    let document_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query("DELETE FROM access_policy_grants WHERE policy_id = (SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1) AND subject_kind = 'principal' AND identity_provider = 'test-idp' AND subject_id = 'policy-admin' AND action = 'write'")
        .bind(f.root_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_access_state SET access_revision = access_revision + 1 WHERE id = 1",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let (status, problem) = json_request(
        management.clone(),
        Method::PATCH,
        &format!("/v1/documents/{document_id}/metadata"),
        Some(&json!({
            "operationId": Uuid::now_v7(),
            "expectedDocumentRevision": document_revision,
            "set": {"category": "revoked"},
            "unset": [],
            "reason": "Test policy revalidation"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "FORBIDDEN");
    let (status, revoked) = json_request(read.clone(), Method::GET, &detail_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert_eq!(
        revoked["capabilities"]["updateMetadata"],
        json!({"status": "disabled", "reason": "permission"})
    );
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) SELECT policy_id,'principal','test-idp','policy-admin','write' FROM access_policy_bindings WHERE folder_id = $1")
        .bind(f.root_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_access_state SET access_revision = access_revision + 1 WHERE id = 1",
    )
    .execute(&f.pool)
    .await
    .unwrap();

    let (status, before_schedule) =
        json_request(read.clone(), Method::GET, &detail_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{before_schedule}");
    assert_eq!(
        before_schedule["capabilities"]["updateMetadata"]["status"],
        "available"
    );

    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'WORKING','Scheduled working','test-idp','policy-admin','{}',now())")
        .bind(working_id)
        .bind(document_id)
        .bind(published_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let scheduled_at = OffsetDateTime::now_utc() + Duration::days(1);
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,$4,$4,$5,$6,$7,'test-idp','policy-admin',$8,'PENDING',now())")
        .bind(publish_operation_id)
        .bind(document_id)
        .bind(working_id)
        .bind(published_id)
        .bind(document_revision)
        .bind(document_revision + 1)
        .bind(scheduled_at)
        .bind(vec![7_u8; 32])
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = $1 WHERE document_version_id = $2",
    )
    .bind(scheduled_at)
    .bind(working_id)
    .execute(&f.pool)
    .await
    .unwrap();

    let (status, problem) = json_request(
        management,
        Method::PATCH,
        &format!("/v1/documents/{document_id}/metadata"),
        Some(&json!({
            "operationId": Uuid::now_v7(),
            "expectedDocumentRevision": document_revision,
            "set": {"category": "changed"},
            "unset": [],
            "reason": "Test state revalidation"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "RESERVED_DOCUMENT");
    let (status, after) = json_request(read, Method::GET, &detail_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(
        after["capabilities"]["updateMetadata"],
        json!({"status": "disabled", "reason": "pendingSchedule"})
    );
}
