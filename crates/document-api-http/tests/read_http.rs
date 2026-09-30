#[path = "../../document-repository-postgres/tests/support/management.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::read::read_router;
use document_api_http::validation::SchemaRegistry;
use document_application::{
    BootstrapRootPolicy, IdentityResolutionError, InvocationKind, VerifiedActorContext,
};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::{Value, json};
use support::{context, fixture};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

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

async fn get(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn context_for(principal_id: &str) -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", principal_id).unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal_id).unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn grant_for(principal_id: &str, actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal_id).unwrap(),
        actions,
    )
    .unwrap()
}

fn assert_schema(definition: &str, value: &Value) {
    let schema = match definition {
        "Folder" => json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["folderId", "name", "revision"],
            "properties": {
                "folderId": {"type": "string", "format": "uuid"},
                "parentFolderId": {"type": ["string", "null"], "format": "uuid"},
                "name": {"type": "string"},
                "revision": {"type": "integer", "minimum": 0}
            }
        }),
        "VersionList" => json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["items", "nextCursor"],
            "properties": {
                "items": {"type": "array", "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["versionId", "versionNo", "lifecycleState", "isCurrent", "createdAt"],
                    "properties": {
                        "versionId": {"type": "string", "format": "uuid"},
                        "versionNo": {"type": "integer", "minimum": 1},
                        "lifecycleState": {"enum": ["working", "published", "withdrawn"]},
                        "isCurrent": {"type": "boolean"},
                        "createdAt": {"type": "string", "format": "date-time"},
                        "publishedAt": {"type": ["string", "null"], "format": "date-time"},
                        "withdrawnAt": {"type": ["string", "null"], "format": "date-time"},
                        "firstReadAt": {"type": ["string", "null"], "format": "date-time"}
                    }
                }},
                "nextCursor": {"type": ["string", "null"]}
            }
        }),
        _ => panic!("unknown schema fixture"),
    };
    SchemaRegistry::compile([("response", schema)])
        .unwrap()
        .validate_response("response", value)
        .unwrap();
}

#[tokio::test]
async fn root_discovery_and_query_validation_cross_the_http_boundary() {
    let f = fixture().await;
    let subject =
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                PolicyGrant::new(
                    subject,
                    [
                        Action::Read,
                        Action::ReadHistory,
                        Action::Write,
                        Action::Administer,
                    ],
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, body) = get(router.clone(), "/v1/folders/root").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["folderId"], f.root_id.as_uuid().to_string());
    assert_schema("Folder", &body);
    let (status, body) = get(router, "/v1/documents?view=published&pageSize=201").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn version_list_requires_and_honors_the_authorization_purpose() {
    let f = fixture().await;
    let subject =
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                PolicyGrant::new(subject, [Action::Read, Action::ReadHistory, Action::Write])
                    .unwrap(),
            ],
        )
        .await
        .unwrap();
    let published = uuid::Uuid::now_v7();
    let working = uuid::Uuid::now_v7();
    for (id, number, state, title) in [
        (published, 1_i64, "PUBLISHED", "Published"),
        (working, 2_i64, "WORKING", "Working"),
    ] {
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,$5,CASE WHEN $4 = 'PUBLISHED' THEN now() ELSE NULL END,'test-idp','policy-admin','{}',now())")
            .bind(id)
            .bind(f.document_id.as_uuid())
            .bind(number)
            .bind(state)
            .bind(title)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(published)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let base = format!("/v1/documents/{}/versions", f.document_id.as_uuid());

    let (status, detail) = get(
        router.clone(),
        &format!("/v1/documents/{}?view=published", f.document_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["folderId"], f.root_id.as_uuid().to_string());
    assert_eq!(detail["unread"], true);

    let (status, body) = get(router.clone(), &base).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "VALIDATION_FAILED");

    let (status, body) = get(router.clone(), &format!("{base}?purpose=published")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["versionId"], published.to_string());
    assert_schema("VersionList", &body);

    let (status, body) = get(router, &format!("{base}?purpose=authoring")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["versionId"], working.to_string());
}

#[tokio::test]
async fn cursor_is_query_and_principal_bound_at_the_http_boundary() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                grant_for("policy-admin", [Action::Read]),
                grant_for("reader-two", [Action::Read]),
            ],
        )
        .await
        .unwrap();
    for (document_id, title) in [
        (f.document_id.as_uuid(), "Alpha"),
        (uuid::Uuid::now_v7(), "Beta"),
    ] {
        if document_id != f.document_id.as_uuid() {
            sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',now())")
                .bind(document_id)
                .bind(f.root_id.as_uuid())
                .execute(&f.pool)
                .await
                .unwrap();
        }
        let version_id = uuid::Uuid::now_v7();
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED',$3,now(),'test-idp','policy-admin','{}',now())")
            .bind(version_id)
            .bind(document_id)
            .bind(title)
            .execute(&f.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
            .bind(version_id)
            .bind(document_id)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    let first_router =
        read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    for page_size in [1_u16, 50, 200] {
        let (status, _) = get(
            first_router.clone(),
            &format!("/v1/documents?view=published&pageSize={page_size}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, first) = get(
        first_router.clone(),
        "/v1/documents?view=published&sort=titleAsc&pageSize=1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let cursor = first["nextCursor"]
        .as_str()
        .unwrap_or_else(|| panic!("two rows yield a cursor: {first}"));
    let (status, body) = get(first_router, "/v1/documents?view=published&cursor=not-hex").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "VALIDATION_FAILED");

    let second_router = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for("reader-two"))),
    )
    .unwrap();
    let (status, body) = get(
        second_router,
        &format!("/v1/documents?view=published&sort=titleAsc&pageSize=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "CURSOR_STALE");
}

#[tokio::test]
async fn publication_end_has_no_published_fallback_but_history_remains_visible() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [Action::Read, Action::ReadHistory, Action::Write],
            )],
        )
        .await
        .unwrap();
    let published = uuid::Uuid::now_v7();
    let withdrawn = uuid::Uuid::now_v7();
    for (id, number, state) in [
        (published, 1_i64, "PUBLISHED"),
        (withdrawn, 2_i64, "WITHDRAWN"),
    ] {
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,withdrawn_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,$4,CASE WHEN $4 = 'PUBLISHED' THEN now() ELSE NULL END,CASE WHEN $4 = 'WITHDRAWN' THEN now() ELSE NULL END,'test-idp','policy-admin','{}',now())")
            .bind(id)
            .bind(f.document_id.as_uuid())
            .bind(number)
            .bind(state)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(published)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,1,$4,'test-idp','policy-admin','end',$4,2,now())")
        .bind(uuid::Uuid::now_v7())
        .bind(f.document_id.as_uuid())
        .bind(vec![0_u8; 32])
        .bind(published)
        .execute(&f.pool)
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, published_list) = get(router.clone(), "/v1/documents?view=published").await;
    assert_eq!(status, StatusCode::OK);
    assert!(published_list["items"].as_array().unwrap().is_empty());
    let (status, body) = get(
        router.clone(),
        &format!("/v1/documents/{}?view=published", f.document_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "DOCUMENT_NOT_FOUND");
    let (status, history) = get(
        router,
        &format!(
            "/v1/documents/{}/versions?purpose=history",
            f.document_id.as_uuid()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    assert!(
        history["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["lifecycleState"] == "withdrawn")
    );
}

#[tokio::test]
async fn policy_read_requires_administer_permission() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant_for("policy-admin", [Action::Read])])
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, body) = get(
        router,
        &format!("/v1/folders/{}/access-policy", f.root_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "FORBIDDEN");
}
