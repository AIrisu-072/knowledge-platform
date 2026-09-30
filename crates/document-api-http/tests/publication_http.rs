#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::publication::publication_router;
use document_application::{
    BootstrapRootPolicy, CreateVersionCommand, IdentityResolutionError, InvocationKind,
    VerifiedActorContext,
};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind};
use serde_json::{Value, json};
use support::{TestIds, actor, fixture, operation_id};
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

fn context() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        actor(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn full_grant() -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
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

fn id(lane: u16, value: u64) -> Uuid {
    Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-{lane:04x}-{value:012x}")).unwrap()
}

async fn send_json(router: Router, uri: &str, value: &Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(value).unwrap()))
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

async fn setup() -> (support::Fixture, Router) {
    let f = fixture().await;
    let bootstrap =
        document_repository_postgres::PostgresDocumentRepository::new_with_bootstrap_actor(
            f.pool.clone(),
            actor(),
        );
    bootstrap
        .initialize_root_policy(&context(), vec![full_grant()])
        .await
        .unwrap();
    let api = publication_router(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    (f, api)
}

#[tokio::test]
async fn lifecycle_actions_preserve_exact_results_and_operation_identity() {
    let (f, api) = setup().await;
    let document_id = f.document_id.as_uuid();
    let target = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(operation_id(141), f.document_id, target, 1, actor())
                .unwrap(),
            f.prepare("Replacement", 2).await,
        )
        .await
        .unwrap();

    let publish_uri = format!(
        "/v1/documents/{document_id}/versions/{}:publish",
        target.as_uuid()
    );
    let publish = json!({"operationId": id(0x8010, 1), "expectedRevision": 2});
    let (status, stale) = send_json(
        api.clone(),
        &publish_uri,
        &json!({"operationId": id(0x8010, 2), "expectedRevision": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["code"], "REVISION_CONFLICT");

    let (status, published) = send_json(api.clone(), &publish_uri, &publish).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["publishOperationId"], id(0x8010, 1).to_string());
    assert_eq!(published["documentId"], document_id.to_string());
    assert_eq!(published["documentVersionId"], target.as_uuid().to_string());
    assert_eq!(published["resultingDocumentRevision"], 3);
    assert_eq!(published["publishedAt"], "2023-11-14T22:13:20Z");
    let (status, replay) = send_json(api.clone(), &publish_uri, &publish).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, published);
    let (status, conflict) = send_json(
        api.clone(),
        &publish_uri,
        &json!({"operationId": id(0x8010, 1), "expectedRevision": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "OPERATION_CONFLICT");

    let withdraw_uri = format!(
        "/v1/documents/{document_id}/versions/{}:withdraw",
        target.as_uuid()
    );
    let withdraw = json!({
        "operationId": id(0x8020, 1),
        "expectedRevision": 3,
        "reason": "Superseded publication withdrawn"
    });
    let (status, withdrawn) = send_json(api.clone(), &withdraw_uri, &withdraw).await;
    assert_eq!(status, StatusCode::OK, "{withdrawn}");
    assert_eq!(withdrawn["operationId"], id(0x8020, 1).to_string());
    assert_eq!(
        withdrawn["formerCurrentVersionId"],
        target.as_uuid().to_string()
    );
    assert_eq!(
        withdrawn["resultingCurrentVersionId"],
        f.base_id.as_uuid().to_string()
    );
    assert_eq!(withdrawn["resultingRevision"], 4);
    let (status, replay) = send_json(api.clone(), &withdraw_uri, &withdraw).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, withdrawn);

    let scheduled_target = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(
                operation_id(142),
                f.document_id,
                scheduled_target,
                4,
                actor(),
            )
            .unwrap(),
            f.prepare("Scheduled replacement", 3).await,
        )
        .await
        .unwrap();
    let schedule_uri = format!(
        "/v1/documents/{document_id}/versions/{}:schedule-publication",
        scheduled_target.as_uuid()
    );
    let (status, non_utc) = send_json(
        api.clone(),
        &schedule_uri,
        &json!({
            "operationId": id(0x8030, 9),
            "expectedRevision": 5,
            "scheduledPublishAt": "2033-05-18T12:33:20+09:00"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{non_utc}");

    let schedule = json!({
        "operationId": id(0x8030, 1),
        "expectedRevision": 5,
        "scheduledPublishAt": "2033-05-18T03:33:20Z"
    });
    let (status, scheduled) = send_json(api.clone(), &schedule_uri, &schedule).await;
    assert_eq!(status, StatusCode::OK, "{scheduled}");
    assert_eq!(scheduled["publishOperationId"], id(0x8030, 1).to_string());
    assert_eq!(
        scheduled["targetVersionId"],
        scheduled_target.as_uuid().to_string()
    );
    assert_eq!(scheduled["acceptedRevision"], 6);
    assert_eq!(scheduled["scheduledPublishAt"], "2033-05-18T03:33:20Z");
    let (status, replay) = send_json(api.clone(), &schedule_uri, &schedule).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, scheduled);

    let cancel_uri = format!(
        "/v1/documents/{document_id}/versions/{}:cancel-publication-schedule",
        scheduled_target.as_uuid()
    );
    let cancel = json!({
        "operationId": id(0x8040, 1),
        "publishOperationId": id(0x8030, 1),
        "expectedRevision": 6
    });
    let (status, cancelled) = send_json(api.clone(), &cancel_uri, &cancel).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["operationId"], id(0x8040, 1).to_string());
    assert_eq!(cancelled["publishOperationId"], id(0x8030, 1).to_string());
    assert_eq!(cancelled["resultingRevision"], 7);
    let (status, replay) = send_json(api.clone(), &cancel_uri, &cancel).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, cancelled);

    let publish_scheduled_uri = format!(
        "/v1/documents/{document_id}/versions/{}:publish",
        scheduled_target.as_uuid()
    );
    let (status, published) = send_json(
        api.clone(),
        &publish_scheduled_uri,
        &json!({"operationId": id(0x8050, 1), "expectedRevision": 7}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["resultingDocumentRevision"], 8);

    let end_uri = format!("/v1/documents/{document_id}:end-publication");
    let (status, stale_current) = send_json(
        api.clone(),
        &end_uri,
        &json!({
            "operationId": id(0x8060, 9),
            "expectedRevision": 8,
            "expectedCurrentVersionId": f.base_id.as_uuid(),
            "reason": "wrong current"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale_current}");
    assert_eq!(stale_current["code"], "REVISION_CONFLICT");

    let end = json!({
        "operationId": id(0x8060, 1),
        "expectedRevision": 8,
        "expectedCurrentVersionId": scheduled_target.as_uuid(),
        "reason": "Publication period ended"
    });
    let (status, ended) = send_json(api.clone(), &end_uri, &end).await;
    assert_eq!(status, StatusCode::OK, "{ended}");
    assert_eq!(ended["operationId"], id(0x8060, 1).to_string());
    assert_eq!(
        ended["formerCurrentVersionId"],
        scheduled_target.as_uuid().to_string()
    );
    assert!(ended["resultingCurrentVersionId"].is_null());
    assert_eq!(ended["resultingDocumentRevision"], 9);
    assert_eq!(ended["endedAt"], "2023-11-14T22:13:20Z");
    let (status, replay) = send_json(api.clone(), &end_uri, &end).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, ended);
    let (status, conflict) = send_json(
        api,
        &end_uri,
        &json!({
            "operationId": id(0x8060, 1),
            "expectedRevision": 8,
            "expectedCurrentVersionId": scheduled_target.as_uuid(),
            "reason": "Different payload"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "OPERATION_CONFLICT");

    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
}

#[tokio::test]
async fn lifecycle_rejects_quality_failure_and_policy_revocation() {
    let (f, api) = setup().await;
    let target = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(operation_id(151), f.document_id, target, 1, actor())
                .unwrap(),
            f.prepare("Quality candidate", 4).await,
        )
        .await
        .unwrap();
    let file_id: Uuid = sqlx::query_scalar(
        "SELECT cr.file_id FROM content_items ci JOIN content_representations cr \
         ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE ci.document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE document_semantic_inspections SET editorial_provenance = $1 WHERE file_id = $2",
    )
    .bind(json!({
        "tracked_changes": [],
        "comments": [{
            "author_label": null,
            "timestamp": null,
            "resolved_state": "resolved",
            "source_locator": "body/1",
            "content": "comment"
        }],
        "document_author_labels": [],
        "last_modified_by": null,
        "modification_metadata": {}
    }))
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let uri = format!(
        "/v1/documents/{}/versions/{}:publish",
        f.document_id.as_uuid(),
        target.as_uuid()
    );
    let (status, quality) = send_json(
        api.clone(),
        &uri,
        &json!({"operationId": id(0x8070, 1), "expectedRevision": 2}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{quality}");
    assert_eq!(quality["code"], "PUBLISH_QUALITY_REJECTED");

    sqlx::query("DELETE FROM access_policy_grants WHERE action = 'publish'")
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, forbidden) = send_json(
        api,
        &uri,
        &json!({"operationId": id(0x8070, 2), "expectedRevision": 2}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{forbidden}");
    assert_eq!(forbidden["code"], "FORBIDDEN");
}
