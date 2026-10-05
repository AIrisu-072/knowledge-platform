#[path = "../../document-repository-postgres/tests/support/management.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::read::{read_router, read_router_with_identity_presentation};
use document_api_http::validation::SchemaRegistry;
use document_application::{
    BootstrapRootPolicy, IdentityPresentation, IdentityPresentationResolution,
    IdentityPresentationResolutionError, IdentityPresentationResolver, IdentityRef,
    IdentityResolutionError, InvocationKind, VerifiedActorContext,
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

#[derive(Clone)]
struct TestPresentationResolver {
    calls: Arc<AtomicUsize>,
    requested: Arc<Mutex<Vec<Vec<IdentityRef>>>>,
    unavailable: bool,
}

impl TestPresentationResolver {
    fn new(unavailable: bool) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(Mutex::new(Vec::new())),
            unavailable,
        }
    }
}

impl IdentityPresentationResolver for TestPresentationResolver {
    fn resolve_batch<'a>(
        &'a self,
        refs: &'a [IdentityRef],
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>,
                > + Send
                + 'a,
        >,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requested.lock().unwrap().push(refs.to_vec());
        let unavailable = self.unavailable;
        let presentations = refs
            .iter()
            .cloned()
            .map(|reference| IdentityPresentation {
                display_name: Some(format!(
                    "{} {}",
                    reference.kind.as_str(),
                    reference.subject_id
                )),
                secondary_text: Some(reference.provider.clone()),
                reference,
                resolution: IdentityPresentationResolution::Resolved,
            })
            .collect();
        Box::pin(async move {
            if unavailable {
                Err(IdentityPresentationResolutionError::Unavailable)
            } else {
                Ok(presentations)
            }
        })
    }
}

async fn get(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

async fn seed_gui_document(f: &support::Fixture) -> (uuid::Uuid, uuid::Uuid) {
    let version_id = uuid::Uuid::now_v7();
    let primary_file_id = uuid::Uuid::now_v7();
    let content_item_id = uuid::Uuid::now_v7();
    let representation_id = uuid::Uuid::now_v7();
    let initial_operation_id = uuid::Uuid::now_v7();
    let metadata_operation_id = uuid::Uuid::now_v7();
    let initial_revision_id = uuid::Uuid::now_v7();
    let latest_revision_id = uuid::Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let older = now - Duration::hours(2);
    let newer = now - Duration::hours(1);

    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Operations policy',$3,'test-idp','policy-admin','{}',$3)")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .bind(older)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'application/pdf',7,$3,$4)")
        .bind(primary_file_id)
        .bind(vec![9_u8; 32])
        .bind(format!("objects/{primary_file_id}"))
        .bind(older)
        .execute(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(content_item_id)
        .bind(version_id)
        .bind(representation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','../../Policy.pdf')")
        .bind(representation_id)
        .bind(content_item_id)
        .bind(primary_file_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let complete_snapshot = json!({
        "document_type": "policy",
        "owning_department": null,
        "category": "operations",
        "extensions": null
    });
    sqlx::query("INSERT INTO document_revisions (revision_id,document_id,document_version_id,major_no,minor_no,metadata_snapshot,metadata_snapshot_status,source_kind,operation_id,created_at,actor_identity_provider,actor_principal_id,reason) VALUES ($1,$2,$3,1,0,$4,'complete','initialPublication',$5,$6,'test-idp','policy-admin',NULL)")
        .bind(initial_revision_id)
        .bind(f.document_id.as_uuid())
        .bind(version_id)
        .bind(&complete_snapshot)
        .bind(initial_operation_id)
        .bind(older)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_revisions (revision_id,document_id,document_version_id,major_no,minor_no,metadata_snapshot,metadata_snapshot_status,source_kind,operation_id,created_at,actor_identity_provider,actor_principal_id,reason) VALUES ($1,$2,$3,1,1,$4,'complete','metadataRevision',$5,$6,'test-idp','policy-admin','classify')")
        .bind(latest_revision_id)
        .bind(f.document_id.as_uuid())
        .bind(version_id)
        .bind(complete_snapshot)
        .bind(metadata_operation_id)
        .bind(newer)
        .execute(&f.pool)
        .await
        .unwrap();

    (version_id, latest_revision_id)
}

// A genuinely never-published version: no revision rows are created or removed.
async fn seed_initial_gui_document(f: &support::Fixture) -> uuid::Uuid {
    let version_id = uuid::Uuid::now_v7();
    let file_id = uuid::Uuid::now_v7();
    let item_id = uuid::Uuid::now_v7();
    let representation_id = uuid::Uuid::now_v7();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Initial policy','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',7,$3,now())")
        .bind(file_id).bind(vec![9_u8;32]).bind(format!("objects/{file_id}")).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item_id).bind(version_id).bind(representation_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','source.txt')")
        .bind(representation_id).bind(item_id).bind(file_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    version_id
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

fn context_for_kind(principal_id: &str, invocation_kind: InvocationKind) -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", principal_id).unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal_id).unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        invocation_kind,
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
            "required": ["folderId", "name", "revision", "capabilities"],
            "properties": {
                "folderId": {"type": "string", "format": "uuid"},
                "parentFolderId": {"type": ["string", "null"], "format": "uuid"},
                "name": {"type": "string"},
                "revision": {"type": "integer", "minimum": 0},
                "capabilities": {"type": "object"}
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
                    "required": ["versionId", "versionNo", "baseVersionId", "lifecycleState", "isCurrent", "createdAt", "approvedAt", "scheduledPublishAt", "publishedAt", "withdrawnAt", "updatedAt", "fileSummary", "firstReadAt"],
                    "properties": {
                        "versionId": {"type": "string", "format": "uuid"},
                        "versionNo": {"type": "integer", "minimum": 1},
                        "baseVersionId": {"type": ["string", "null"], "format": "uuid"},
                        "lifecycleState": {"enum": ["working", "published", "withdrawn"]},
                        "isCurrent": {"type": "boolean"},
                        "createdAt": {"type": "string", "format": "date-time"},
                        "approvedAt": {"type": ["string", "null"], "format": "date-time"},
                        "scheduledPublishAt": {"type": ["string", "null"], "format": "date-time"},
                        "publishedAt": {"type": ["string", "null"], "format": "date-time"},
                        "withdrawnAt": {"type": ["string", "null"], "format": "date-time"},
                        "updatedAt": {"type": "string", "format": "date-time"},
                        "fileSummary": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["authoritativeItemCount", "totalSizeBytes", "primary"],
                            "properties": {
                                "authoritativeItemCount": {"type": "integer", "minimum": 0},
                                "totalSizeBytes": {"type": "integer", "minimum": 0},
                                "primary": {"type": ["object", "null"]}
                            }
                        },
                        "firstReadAt": {"type": ["string", "null"], "format": "date-time"}
                    }
                }},
                "nextCursor": {"type": ["string", "null"]}
            }
        }),
        "Version" => json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["versionId", "versionNo", "baseVersionId", "lifecycleState", "isCurrent", "createdAt", "approvedAt", "scheduledPublishAt", "publishedAt", "withdrawnAt", "updatedAt", "fileSummary", "firstReadAt", "title", "metadata", "capabilities"],
            "properties": {
                "versionId": {"type": "string", "format": "uuid"},
                "versionNo": {"type": "integer", "minimum": 1},
                "baseVersionId": {"type": ["string", "null"], "format": "uuid"},
                "lifecycleState": {"enum": ["working", "published", "withdrawn"]},
                "isCurrent": {"type": "boolean"},
                "createdAt": {"type": "string", "format": "date-time"},
                "approvedAt": {"type": ["string", "null"], "format": "date-time"},
                "scheduledPublishAt": {"type": ["string", "null"], "format": "date-time"},
                "publishedAt": {"type": ["string", "null"], "format": "date-time"},
                "withdrawnAt": {"type": ["string", "null"], "format": "date-time"},
                "updatedAt": {"type": "string", "format": "date-time"},
                "fileSummary": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["authoritativeItemCount", "totalSizeBytes", "primary"],
                    "properties": {
                        "authoritativeItemCount": {"type": "integer", "minimum": 0},
                        "totalSizeBytes": {"type": "integer", "minimum": 0},
                        "primary": {"type": ["object", "null"]}
                    }
                },
                "firstReadAt": {"type": ["string", "null"], "format": "date-time"},
                "title": {"type": "string"},
                "metadata": {"type": "object"},
                "capabilities": {"type": "object"}
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
async fn gui_document_list_projects_version_revision_read_state_and_file_summary() {
    let f = fixture().await;
    let (_, latest_revision_id) = seed_gui_document(&f).await;
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
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();

    let (status, body) = get(router, "/v1/documents?view=published").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let item = &body["items"][0];
    assert_eq!(item["documentId"], f.document_id.as_uuid().to_string());
    assert_eq!(item["displayVersion"]["versionNo"], 1);
    assert_eq!(item["displayVersion"]["baseVersionId"], Value::Null);
    assert_eq!(item["displayVersion"]["lifecycleState"], "PUBLISHED");
    assert_eq!(item["displayVersion"]["isCurrent"], true);
    assert!(item["displayVersion"]["updatedAt"].as_str().is_some());
    assert_eq!(
        item["displayVersion"]["fileSummary"]["authoritativeItemCount"],
        1
    );
    assert_eq!(item["displayVersion"]["fileSummary"]["totalSizeBytes"], 7);
    assert_eq!(
        item["displayVersion"]["fileSummary"]["primary"]["displayName"],
        "Policy.pdf"
    );
    assert_eq!(
        item["displayVersion"]["fileSummary"]["primary"]["sizeBytes"],
        7
    );
    assert_eq!(
        item["displayRevision"]["revisionId"],
        latest_revision_id.to_string()
    );
    assert_eq!(item["displayRevision"]["major"], 1);
    assert_eq!(item["displayRevision"]["minor"], 1);
    assert_eq!(item["readState"]["isRead"], false);
    assert_eq!(item["readState"]["firstReadAt"], Value::Null);
    assert_eq!(item["displayTimestamp"]["kind"], "revisionCreatedAt");
    assert_eq!(
        item["displayTimestamp"]["value"],
        item["displayRevision"]["createdAt"]
    );
}

#[tokio::test]
async fn revision_history_is_keyset_paginated_authorized_and_has_detail_snapshot() {
    let f = fixture().await;
    let (_, latest_revision_id) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                grant_for("policy-admin", [Action::Read, Action::ReadHistory]),
                grant_for("reader-two", [Action::Read]),
            ],
        )
        .await
        .unwrap();
    let path = format!("/v1/documents/{}/revisions", f.document_id.as_uuid());
    let unauthorized = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for("reader-two"))),
    )
    .unwrap();
    let (status, body) = get(unauthorized, &path).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, first) = get(router.clone(), &format!("{path}?pageSize=1")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        first["items"][0]["revisionId"],
        latest_revision_id.to_string()
    );
    assert_eq!(first["items"][0]["major"], 1);
    assert_eq!(first["items"][0]["minor"], 1);
    let cursor = first["nextCursor"]
        .as_str()
        .expect("two revisions have a next page");

    let (status, second) = get(
        router.clone(),
        &format!("{path}?pageSize=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["major"], 1);
    assert_eq!(second["items"][0]["minor"], 0);
    assert_eq!(second["nextCursor"], Value::Null);

    let (status, detail) = get(router, &format!("{path}/{latest_revision_id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["revisionId"], latest_revision_id.to_string());
    assert_eq!(detail["sourceKind"], "metadataRevision");
    assert_eq!(detail["reason"], "classify");
    assert_eq!(detail["metadataSnapshot"]["document_type"], "policy");
    assert_eq!(detail["metadataSnapshotStatus"], "complete");
}

#[tokio::test]
async fn detail_capabilities_reflect_permission_lifecycle_and_human_context() {
    let f = fixture().await;
    let (published, _) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                grant_for(
                    "policy-admin",
                    [
                        Action::Read,
                        Action::ReadHistory,
                        Action::Write,
                        Action::Publish,
                        Action::Administer,
                    ],
                ),
                grant_for("reader-two", [Action::Read]),
                grant_for(
                    "service-actor",
                    [
                        Action::Read,
                        Action::ReadHistory,
                        Action::Write,
                        Action::Publish,
                        Action::Administer,
                    ],
                ),
            ],
        )
        .await
        .unwrap();
    let document_path = format!("/v1/documents/{}?view=published", f.document_id.as_uuid());
    let version_path = format!(
        "/v1/documents/{}/versions/{}?purpose=published",
        f.document_id.as_uuid(),
        published
    );

    let admin = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, document) = get(admin.clone(), &document_path).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    assert_eq!(
        document["capabilities"]["createVersion"]["status"],
        "available"
    );
    assert_eq!(
        document["capabilities"]["manageAccess"]["status"],
        "available"
    );

    let (status, version) = get(admin.clone(), &version_path).await;
    assert_eq!(status, StatusCode::OK, "{version}");
    assert_eq!(version["capabilities"]["withdraw"]["status"], "available");
    assert_eq!(version["capabilities"]["download"]["status"], "available");
    assert_eq!(
        version["capabilities"]["edit"],
        json!({"status": "disabled", "reason": "lifecycle"})
    );

    let (status, root) = get(admin.clone(), "/v1/folders/root").await;
    assert_eq!(status, StatusCode::OK, "{root}");
    assert_eq!(
        root["capabilities"]["createDocument"]["status"],
        "available"
    );
    assert_eq!(root["capabilities"]["manageAccess"]["status"], "available");

    let children_path = format!("/v1/folders/{}/children", f.root_id.as_uuid());
    let (status, children) = get(admin, &children_path).await;
    assert_eq!(status, StatusCode::OK, "{children}");
    assert_eq!(
        children["capabilities"]["createFolder"]["status"],
        "available"
    );

    let reader = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for("reader-two"))),
    )
    .unwrap();
    let (status, document) = get(reader, &document_path).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    assert_eq!(
        document["capabilities"]["createVersion"],
        json!({"status": "disabled", "reason": "permission"})
    );

    let service = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for_kind(
            "service-actor",
            InvocationKind::Service,
        ))),
    )
    .unwrap();
    let (status, document) = get(service, &document_path).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    assert_eq!(
        document["capabilities"]["createVersion"],
        json!({"status": "disabled", "reason": "notHumanInteractive"})
    );
}

#[tokio::test]
async fn working_version_capabilities_disable_publish_and_enable_rebase_for_stale_base() {
    let f = fixture().await;
    let (base_version_id, _) = seed_gui_document(&f).await;
    let newer_current_id = uuid::Uuid::now_v7();
    let stale_working_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'PUBLISHED','New current',now(),'test-idp','policy-admin','{}',now())")
        .bind(newer_current_id)
        .bind(f.document_id.as_uuid())
        .bind(base_version_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,3,$3,'WORKING','Stale working','test-idp','policy-admin','{}',now())")
        .bind(stale_working_id)
        .bind(f.document_id.as_uuid())
        .bind(base_version_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(newer_current_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [
                    Action::Read,
                    Action::ReadHistory,
                    Action::Write,
                    Action::Publish,
                ],
            )],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{}?purpose=authoring",
        f.document_id.as_uuid(),
        stale_working_id
    );
    let (status, version) = get(router, &path).await;
    assert_eq!(status, StatusCode::OK, "{version}");
    assert_eq!(version["capabilities"]["rebase"]["status"], "available");
    assert_eq!(
        version["capabilities"]["edit"],
        json!({"status": "disabled", "reason": "staleBase"})
    );
    assert_eq!(
        version["capabilities"]["publish"],
        json!({"status": "disabled", "reason": "staleBase"})
    );
}

#[tokio::test]
async fn document_capabilities_distinguish_missing_current_from_ended_publication() {
    let f = fixture().await;
    let (published_id, _) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [
                    Action::Read,
                    Action::ReadHistory,
                    Action::Write,
                    Action::Publish,
                    Action::Administer,
                ],
            )],
        )
        .await
        .unwrap();
    let working_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'WORKING','Working after withdrawal','test-idp','policy-admin','{}',now())")
        .bind(working_id)
        .bind(f.document_id.as_uuid())
        .bind(published_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();

    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!("/v1/documents/{}?view=authoring", f.document_id.as_uuid());
    let (status, document) = get(router.clone(), &path).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    assert_eq!(
        document["capabilities"]["createVersion"]["status"],
        "disabled"
    );
    assert_eq!(
        document["capabilities"]["endPublication"],
        json!({"status": "disabled", "reason": "notCurrent"})
    );
    let version_path = format!(
        "/v1/documents/{}/versions/{}?purpose=authoring",
        f.document_id.as_uuid(),
        working_id
    );
    let (status, working) = get(router, &version_path).await;
    assert_eq!(status, StatusCode::OK, "{working}");
    assert_eq!(
        working["capabilities"]["edit"],
        json!({"status": "disabled", "reason": "staleBase"})
    );
    assert_eq!(working["capabilities"]["rebase"]["status"], "disabled");
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
    assert_eq!(body["items"][0]["versionNo"], 1);
    assert_eq!(body["items"][0]["baseVersionId"], serde_json::Value::Null);
    assert!(body["items"][0]["updatedAt"].as_str().is_some());
    assert_eq!(body["items"][0]["fileSummary"]["authoritativeItemCount"], 0);
    assert_eq!(body["items"][0]["fileSummary"]["totalSizeBytes"], 0);
    assert_eq!(
        body["items"][0]["fileSummary"]["primary"],
        serde_json::Value::Null
    );
    assert_schema("VersionList", &body);

    let (status, detail) = get(
        router.clone(),
        &format!(
            "/v1/documents/{}/versions/{}?purpose=published",
            f.document_id.as_uuid(),
            published
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["baseVersionId"], serde_json::Value::Null);
    assert!(detail["updatedAt"].as_str().is_some());
    assert_eq!(detail["fileSummary"]["authoritativeItemCount"], 0);
    assert_schema("Version", &detail);

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

#[tokio::test]
async fn history_and_policy_resolve_identity_presentations_once_per_response() {
    let f = fixture().await;
    seed_gui_document(&f).await;
    let group = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Group, "directory", "reviewers").unwrap(),
        [Action::Read],
    )
    .unwrap();
    let role = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Role, "directory", "auditor").unwrap(),
        [Action::Read],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                grant_for(
                    "policy-admin",
                    [Action::Read, Action::ReadHistory, Action::Administer],
                ),
                group,
                role,
            ],
        )
        .await
        .unwrap();
    let resolver = TestPresentationResolver::new(false);
    let router = read_router_with_identity_presentation(
        f.repository.clone(),
        Arc::new(FixedIdentity(context())),
        Arc::new(resolver.clone()),
    )
    .unwrap();

    let (status, history) = get(
        router.clone(),
        &format!("/v1/documents/{}/history", f.document_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let history_actor = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("actor").filter(|actor| !actor.is_null()))
        .expect("version history includes an actor");
    assert_eq!(
        history_actor["presentation"]["ref"]["subjectId"],
        "policy-admin"
    );
    assert_eq!(
        history_actor["presentation"]["displayName"],
        "principal policy-admin"
    );

    let (status, policy) = get(
        router,
        &format!("/v1/folders/{}/access-policy", f.root_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    let grants = policy["effectiveGrants"].as_array().unwrap();
    assert_eq!(grants.len(), 3);
    assert!(grants.iter().all(|grant| {
        grant["presentation"]["resolution"] == "resolved"
            && grant["presentation"]["ref"]["subjectId"] == grant["subjectId"]
    }));
    let requested = resolver.requested.lock().unwrap();
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    assert_eq!(requested.len(), 2);
    assert_eq!(
        requested[0].len(),
        1,
        "repeated history actors are deduplicated"
    );
    assert_eq!(
        requested[1].len(),
        3,
        "policy subjects resolve in one batch"
    );
}

#[tokio::test]
async fn unavailable_identity_presentation_does_not_fail_history_or_policy_reads() {
    let f = fixture().await;
    seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [Action::Read, Action::ReadHistory, Action::Administer],
            )],
        )
        .await
        .unwrap();
    let router = read_router_with_identity_presentation(
        f.repository.clone(),
        Arc::new(FixedIdentity(context())),
        Arc::new(TestPresentationResolver::new(true)),
    )
    .unwrap();

    let (status, history) = get(
        router.clone(),
        &format!("/v1/documents/{}/history", f.document_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let history_actor = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("actor").filter(|actor| !actor.is_null()))
        .expect("version history includes an actor");
    assert_eq!(history_actor["presentation"]["resolution"], "unavailable");
    assert_eq!(history_actor["presentation"]["displayName"], Value::Null);
    assert_eq!(
        history_actor["presentation"]["ref"]["subjectId"],
        "policy-admin"
    );

    let (status, policy) = get(
        router,
        &format!("/v1/folders/{}/access-policy", f.root_id.as_uuid()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    assert_eq!(
        policy["effectiveGrants"][0]["presentation"]["resolution"],
        "unavailable"
    );
    assert_eq!(
        policy["effectiveGrants"][0]["presentation"]["ref"]["subjectId"],
        "policy-admin"
    );
}

#[tokio::test]
async fn edit_manifest_reads_exact_metadata_and_order_with_current_write_authorization() {
    let f = fixture().await;
    let (version_id, _) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![
                grant_for("policy-admin", [Action::Read, Action::Write]),
                grant_for("reader-two", [Action::Read, Action::ReadHistory]),
            ],
        )
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let existing_item: uuid::Uuid = sqlx::query_scalar(
        "SELECT content_item_id FROM content_items WHERE document_version_id = $1",
    )
    .bind(version_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let existing_file: uuid::Uuid = sqlx::query_scalar(
        "SELECT file_id FROM content_representations WHERE content_item_id = $1",
    )
    .bind(existing_item)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let extra_item = uuid::Uuid::from_u128(90);
    let authoritative = uuid::Uuid::from_u128(93);
    let rendition = uuid::Uuid::from_u128(91);
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'appendix',0,$3)")
        .bind(extra_item).bind(version_id).bind(authoritative).execute(&mut *tx).await.unwrap();
    for (id, role, name) in [
        (rendition, "RENDITION", "preview\\exact.pdf"),
        (authoritative, "AUTHORITATIVE", "source/../exact\nname.txt"),
    ] {
        sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,$4,$5)")
            .bind(id).bind(extra_item).bind(existing_file).bind(role).bind(name).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE documents SET revision=17 WHERE document_id=$1")
        .bind(f.document_id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{version_id}/edit-manifest",
        f.document_id.as_uuid()
    );
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, body) = get(router.clone(), &format!("{path}?purpose=published")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sourceVersionId"], version_id.to_string());
    assert_eq!(body["documentRevision"], 17);
    assert_eq!(body["purpose"], "published");
    assert_eq!(body["title"], "Operations policy");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["logicalPath"], "appendix");
    assert_eq!(items[1]["logicalPath"], "primary");
    assert_eq!(
        items[1]["representations"][0]["originalFilename"],
        "../../Policy.pdf"
    );
    let reps = items[0]["representations"].as_array().unwrap();
    assert_eq!(reps[0]["representationId"], authoritative.to_string());
    assert_eq!(reps[0]["role"], "authoritative");
    assert_eq!(reps[0]["originalFilename"], "source/../exact\nname.txt");
    assert_eq!(reps[0]["fileId"], existing_file.to_string());
    assert_eq!(reps[0]["mediaType"], "application/pdf");
    assert_eq!(reps[0]["sizeBytes"], 7);
    assert_eq!(reps[1]["role"], "rendition");
    assert_eq!(reps[1]["originalFilename"], "preview\\exact.pdf");
    let reader = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for("reader-two"))),
    )
    .unwrap();
    assert_eq!(
        get(reader, &format!("{path}?purpose=published")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(router.clone(), &format!("{path}?purpose=authoring"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(router.clone(), &format!("{path}?purpose=history"))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // Revoking Write after a successful read must invalidate a new read.
    sqlx::query(
        "DELETE FROM access_policy_grants WHERE subject_id='policy-admin' AND action='write'",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        get(router, &format!("{path}?purpose=published")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn initial_working_edit_capability_rejects_existing_publication_history() {
    let f = fixture().await;
    let (version_id, _) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [Action::Read, Action::Write, Action::ReadHistory],
            )],
        )
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(version_id).execute(&f.pool).await.unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{version_id}?purpose=authoring",
        f.document_id.as_uuid()
    );
    let (_, prior) = get(router.clone(), &path).await;
    assert_eq!(
        prior["capabilities"]["edit"]["status"], "disabled",
        "issued revisions prove this is not an initial draft"
    );
    assert_eq!(prior["capabilities"]["rebase"]["status"], "disabled");
}

#[tokio::test]
async fn initial_working_edit_capability_requires_no_publication_history_or_pending_schedule() {
    let f = fixture().await;
    let version_id = seed_initial_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [Action::Read, Action::Write, Action::ReadHistory],
            )],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{version_id}?purpose=authoring",
        f.document_id.as_uuid()
    );
    let (status, initial) = get(router.clone(), &path).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert_eq!(initial["capabilities"]["edit"]["status"], "available");
    assert_eq!(initial["capabilities"]["rebase"]["status"], "disabled");
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,0,1,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(uuid::Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8; 32]).execute(&f.pool).await.unwrap();
    let (_, pending) = get(router, &path).await;
    assert_eq!(
        pending["capabilities"]["edit"],
        json!({"status":"disabled","reason":"pendingSchedule"})
    );
}

#[tokio::test]
async fn edit_manifest_snapshot_keeps_revision_title_and_original_filename_together() {
    let f = fixture().await;
    let version_id = seed_initial_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for("policy-admin", [Action::Read, Action::Write])],
        )
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE document_versions SET title='snapshot-1' WHERE document_version_id=$1")
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content_representations SET original_filename='snapshot-1' WHERE content_item_id IN (SELECT content_item_id FROM content_items WHERE document_version_id=$1)")
        .bind(version_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    // Agent contexts remain eligible under the existing authoring policy.
    let router = read_router(
        f.repository.clone(),
        Arc::new(FixedIdentity(context_for_kind(
            "policy-admin",
            InvocationKind::Agent,
        ))),
    )
    .unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{version_id}/edit-manifest?purpose=authoring",
        f.document_id.as_uuid()
    );
    let pool = f.pool.clone();
    let document_id = f.document_id;
    let writer = tokio::spawn(async move {
        for revision in 2..=30_i64 {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("UPDATE documents SET revision=$2 WHERE document_id=$1")
                .bind(document_id.as_uuid())
                .bind(revision)
                .execute(&mut *tx)
                .await
                .unwrap();
            let name = format!("snapshot-{revision}");
            sqlx::query("UPDATE document_versions SET title=$2 WHERE document_version_id=$1")
                .bind(version_id)
                .bind(&name)
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE content_representations SET original_filename=$2 WHERE content_item_id IN (SELECT content_item_id FROM content_items WHERE document_version_id=$1)")
                .bind(version_id).bind(&name).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            tokio::task::yield_now().await;
        }
    });
    for _ in 0..30 {
        let (status, body) = get(router.clone(), &path).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let revision = body["documentRevision"].as_i64().unwrap();
        assert_eq!(body["title"], format!("snapshot-{revision}"));
        assert_eq!(
            body["items"][0]["representations"][0]["originalFilename"],
            format!("snapshot-{revision}")
        );
        assert_eq!(body["sourceVersionId"], version_id.to_string());
    }
    writer.await.unwrap();
}

#[tokio::test]
async fn edit_manifest_never_falls_back_to_history_after_publication_changes_or_ends() {
    let f = fixture().await;
    let (old_version, _) = seed_gui_document(&f).await;
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
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{old_version}/edit-manifest?purpose=published",
        f.document_id.as_uuid()
    );
    assert_eq!(get(router.clone(), &path).await.0, StatusCode::OK);
    let current = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,'PUBLISHED','Replacement',now(),'test-idp','policy-admin','{}',now())")
        .bind(current).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$2 WHERE document_id=$1")
        .bind(f.document_id.as_uuid())
        .bind(current)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(get(router.clone(), &path).await.0, StatusCode::NOT_FOUND);
    sqlx::query("UPDATE document_versions SET lifecycle_state='WITHDRAWN',withdrawn_at=now() WHERE document_version_id=$1")
        .bind(old_version).execute(&f.pool).await.unwrap();
    assert_eq!(get(router.clone(), &path).await.0, StatusCode::NOT_FOUND);
    // The purpose gate also rejects ended publication, even if an inconsistent
    // fixture still has a current pointer. No history permission broadens it.
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,1,$4,'test-idp','policy-admin','end', $4,2,now())")
        .bind(uuid::Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![1u8;32]).bind(current).execute(&f.pool).await.unwrap();
    let current_path = format!(
        "/v1/documents/{}/versions/{current}/edit-manifest?purpose=published",
        f.document_id.as_uuid()
    );
    assert_eq!(get(router, &current_path).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_version_capability_requires_current_published_and_no_working_or_schedule() {
    use document_application::{
        ActionAvailability, ActionCapabilityReadRepository, CapabilityDisabledReason,
    };
    let f = fixture().await;
    let (version_id, _) = seed_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for("policy-admin", [Action::Read, Action::Write])],
        )
        .await
        .unwrap();
    let available = f
        .repository
        .read_document_action_capabilities(&context(), f.document_id)
        .await
        .unwrap();
    assert_eq!(available.create_version, ActionAvailability::available());
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let absent = f
        .repository
        .read_document_action_capabilities(&context(), f.document_id)
        .await
        .unwrap();
    assert_eq!(
        absent.create_version,
        ActionAvailability::disabled(CapabilityDisabledReason::NotCurrent)
    );
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let working_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'WORKING','Working','test-idp','policy-admin','{}',now())")
        .bind(working_id).bind(f.document_id.as_uuid()).bind(version_id).execute(&f.pool).await.unwrap();
    let working = f
        .repository
        .read_document_action_capabilities(&context(), f.document_id)
        .await
        .unwrap();
    assert_eq!(
        working.create_version,
        ActionAvailability::disabled(CapabilityDisabledReason::Lifecycle)
    );
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,$4,$4,0,1,now() + interval '1 day','test-idp','policy-admin',$5,'PENDING',now())")
        .bind(uuid::Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(working_id).bind(version_id).bind(vec![1_u8; 32]).execute(&f.pool).await.unwrap();
    let pending = f
        .repository
        .read_document_action_capabilities(&context(), f.document_id)
        .await
        .unwrap();
    assert_eq!(
        pending.create_version,
        ActionAvailability::disabled(CapabilityDisabledReason::PendingSchedule)
    );
}

#[tokio::test]
async fn edit_manifest_refuses_unclassified_legacy_or_empty_content() {
    let f = fixture().await;
    let version_id = seed_initial_gui_document(&f).await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for("policy-admin", [Action::Read, Action::Write])],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let path = format!(
        "/v1/documents/{}/versions/{version_id}/edit-manifest?purpose=authoring",
        f.document_id.as_uuid()
    );
    sqlx::query("UPDATE document_versions SET requires_content_classification=TRUE WHERE document_version_id=$1")
        .bind(version_id).execute(&f.pool).await.unwrap();
    let (status, body) = get(router.clone(), &path).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "BUSINESS_RULE_REJECTED");
    sqlx::query("UPDATE document_versions SET requires_content_classification=FALSE WHERE document_version_id=$1")
        .bind(version_id).execute(&f.pool).await.unwrap();
    sqlx::query("DELETE FROM content_items WHERE document_version_id=$1")
        .bind(version_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, body) = get(router, &path).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "BUSINESS_RULE_REJECTED");
}

#[tokio::test]
async fn unclassified_current_can_end_publication_while_versioning_stays_disabled() {
    use document_application::{
        EndDocumentPublicationCommand, EndPublicationRecord, PublicationEndOperationId,
        PublicationEndRepository,
    };
    use document_domain::{AuditEventId, DocumentVersionId, EventId};

    let f = fixture().await;
    let (published_id, _) = seed_gui_document(&f).await;
    sqlx::query("UPDATE document_versions SET requires_content_classification=TRUE WHERE document_version_id=$1")
        .bind(published_id).execute(&f.pool).await.unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant_for(
                "policy-admin",
                [Action::Read, Action::Write, Action::Publish],
            )],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let document_path = format!("/v1/documents/{}?view=published", f.document_id.as_uuid());
    let (status, document) = get(router.clone(), &document_path).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    assert_eq!(
        document["capabilities"]["createVersion"],
        json!({"status": "disabled", "reason": "notCurrent"}),
    );
    assert_eq!(
        document["capabilities"]["endPublication"]["status"],
        "available"
    );

    let published_path = format!(
        "/v1/documents/{}/versions/{published_id}?purpose=published",
        f.document_id.as_uuid(),
    );
    let (status, published) = get(router.clone(), &published_path).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["capabilities"]["withdraw"]["status"], "available");

    let working_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'WORKING','Working','test-idp','policy-admin','{}',now())")
        .bind(working_id).bind(f.document_id.as_uuid()).bind(published_id).execute(&f.pool).await.unwrap();
    let working_path = format!(
        "/v1/documents/{}/versions/{working_id}?purpose=authoring",
        f.document_id.as_uuid(),
    );
    let (status, working) = get(router, &working_path).await;
    assert_eq!(status, StatusCode::OK, "{working}");
    for operation in ["edit", "rebase"] {
        assert_eq!(
            working["capabilities"][operation],
            json!({"status": "disabled", "reason": "lifecycle"}),
        );
    }

    let command = EndDocumentPublicationCommand::new(
        PublicationEndOperationId::try_from_uuid(uuid::Uuid::now_v7()).unwrap(),
        f.document_id,
        1,
        DocumentVersionId::from_uuid(published_id),
        support::actor(),
        "Retire the legacy publication".into(),
    )
    .unwrap();
    let result = f
        .repository
        .with_verified_actor(context())
        .end_document_publication(EndPublicationRecord::new(
            command,
            OffsetDateTime::now_utc(),
            EventId::from_uuid(uuid::Uuid::now_v7()),
            AuditEventId::from_uuid(uuid::Uuid::now_v7()),
        ))
        .await
        .unwrap();
    assert_eq!(result.former_current_version_id().as_uuid(), published_id);
    assert_eq!(result.resulting_document_revision(), 2);
    let current: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
    let original: (String, bool) = sqlx::query_as(
        "SELECT lifecycle_state, requires_content_classification FROM document_versions WHERE document_version_id=$1",
    )
    .bind(published_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(original, ("PUBLISHED".into(), true));
}
