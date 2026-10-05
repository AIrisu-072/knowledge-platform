//! B7: the durable Graph read by many actors, and the public Discover Graph
//! input. The ternary `document_current_placement` relation joins a
//! Document, its current Version and its Folder placement; traversing it
//! from the Document to the Version passes the placement as the third
//! participant. Each request enters the loaded Graph with its own Document
//! access, so the path exists only for an actor who can currently read every
//! participant, and only while that request is bound.

mod support;

#[path = "support/api.rs"]
mod api;
#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/durable.rs"]
mod durable;
#[path = "support/registration.rs"]
mod registration;

use std::sync::Arc;

use document_application::{DocumentAccessCheckService, InvocationKind, VerifiedActorContext};
use document_domain::{DocumentId, PolicySubject, PolicySubjectKind, PrincipalRef};
use durable::{Durable, publish};
use search_application::indexing_service::IndexingOutcome;
use search_application::ports::{CurrentAccessEvaluatorPort, HyperGraphRetrieverPort};
use search_application::search_core::graph::{
    GraphTraversalPlan, RelationPathPattern, TraversalBudget,
};
use search_application::search_core::id::{DiscoveryEvaluationId, ResourceId};
use search_application::search_core::relation::RelationNamespace;
use search_application::search_core::temporal::TemporalEvaluationContext;
use search_runtime::durable_read::DurableDocumentReadModel;
use search_source_document::{DocumentCurrentAccessAdapter, document_resource_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn adapter(durable: &Durable, principal: &str, binding: &str) -> Arc<DocumentCurrentAccessAdapter> {
    let actor = VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test-idp", principal).unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal).unwrap()],
        OffsetDateTime::now_utc() + time::Duration::minutes(10),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    Arc::new(DocumentCurrentAccessAdapter::new(
        durable.source_id,
        durable.pool.clone(),
        DocumentAccessCheckService::new(durable.repository.clone()),
        actor,
        binding.into(),
    ))
}

fn placement_plan(seed: ResourceId, access_context: &str) -> GraphTraversalPlan {
    let now = OffsetDateTime::now_utc();
    GraphTraversalPlan {
        seed_nodes: vec![seed],
        path_patterns: vec![RelationPathPattern {
            namespace: RelationNamespace::Discovery,
            relation_type: "document_current_placement".into(),
            from_role: "document".into(),
            to_role: "current_version".into(),
            from_resource: None,
            to_resource: None,
            required_participants: vec![],
        }],
        allowed_relation_types: vec!["document_current_placement".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::now_v7()),
            now,
            now,
            "UTC",
        )),
        access_context: access_context.into(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 16,
            max_branching_per_node: 8,
            max_seed_nodes: 8,
            max_paths: 16,
        },
        stop_conditions: vec![],
    }
}

#[tokio::test]
async fn graph_nary_third_participant_revocation() {
    let durable = Durable::start_with(true).await;
    let document = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    let key = match durable
        .indexer()
        .await
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            document,
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published generation: {other:?}"),
    };
    let version: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(document)
            .fetch_one(&durable.pool)
            .await
            .unwrap();
    let model = DurableDocumentReadModel::new(
        durable.pool.clone(),
        &durable.lexical_root,
        durable.source(),
    );
    let loaded = model.current().await.unwrap().unwrap();
    let graph = loaded.graph();
    let reader = graph.reader();
    let seed = document_resource_id(durable.source_id, DocumentId::from_uuid(document));
    let reached = |hits: &[search_application::ports::GraphRetrievalHit]| -> Vec<ResourceId> {
        hits.iter()
            .filter_map(|hit| hit.candidate.resource_ref)
            .collect()
    };

    // The editor can read the Document, so every participant — the
    // Document, its Folder placement and the Version — is readable.
    let editor = graph
        .enter(
            "editor-binding".into(),
            adapter(&durable, "editor", "editor-binding"),
        )
        .unwrap();
    let result = reader
        .retrieve(key, &placement_plan(seed, "editor-binding"))
        .await
        .unwrap();
    assert_eq!(reached(&result.hits), vec![ResourceId::from_uuid(version)]);
    assert!(
        editor
            .evaluate(ResourceId::from_uuid(version), "editor-binding")
            .await
            .unwrap()
            == search_application::ports::AccessDecision::Allowed
    );

    // An actor without Read reaches nothing through the same relation.
    let _outsider = graph
        .enter(
            "outsider-binding".into(),
            adapter(&durable, "outsider", "outsider-binding"),
        )
        .unwrap();
    let denied = reader
        .retrieve(key, &placement_plan(seed, "outsider-binding"))
        .await
        .unwrap();
    assert!(reached(&denied.hits).is_empty());

    // Once the editor's request leaves, its binding opens no path again.
    drop(editor);
    let left = reader
        .retrieve(key, &placement_plan(seed, "editor-binding"))
        .await
        .unwrap();
    assert!(reached(&left.hits).is_empty());
}

#[tokio::test]
async fn public_discover_accepts_one_closed_graph_input() {
    use api::{Host, call, discovery_config, durable as api_durable};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use search_application::retrieval::{RetrievalInputs, RetrieverSupport};
    use search_application::source_registration::HostRegistrationSnapshotPort;
    use search_runtime::api::build_search_api_runtime;
    use search_runtime::durable_read::{
        DocumentActorAccess, DocumentActorAccessPort, DurableDocumentPorts,
    };
    use search_source_document::DocumentApiRead;
    use serde_json::json;

    struct Access(Arc<DocumentCurrentAccessAdapter>, Arc<DocumentApiRead>);
    impl DocumentActorAccessPort for Access {
        fn for_actor<'a>(
            &'a self,
            _actor: &'a search_application::scoped::TrustedSearchScope,
        ) -> search_api_http::router::ApiFuture<'a, DocumentActorAccess> {
            Box::pin(async move {
                Ok(DocumentActorAccess {
                    access: self.0.clone(),
                    resource_locator: self.1.clone(),
                    resource_reader: self.1.clone(),
                })
            })
        }
    }

    let durable = Durable::start_with(true).await;
    let document = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    durable
        .indexer()
        .await
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            document,
        ))
        .await
        .unwrap();
    let host = Host::new();
    host.grants.grant("tenant-a", "editor", durable.source_id);
    host.login("editor-token", "tenant-a", "editor");
    let access = adapter(&durable, "editor", "unused");
    let read = Arc::new(DocumentApiRead::new(
        durable.source_id,
        durable.pool.clone(),
        access.clone(),
        "unused".into(),
    ));
    let ports = Arc::new(DurableDocumentPorts::new(
        Arc::new(DurableDocumentReadModel::new(
            durable.pool.clone(),
            &durable.lexical_root,
            durable.source(),
        )),
        Arc::new(Access(access, read)),
    ));
    let snapshot: Arc<dyn HostRegistrationSnapshotPort> = durable.host.clone();
    let runtime = build_search_api_runtime(
        host.config(
            snapshot,
            None,
            discovery_config(RetrieverSupport::default(), RetrievalInputs::default()),
        ),
        api_durable(&durable.pool, ports),
        host.identity(),
    )
    .await
    .unwrap();
    let router = runtime.router();
    let seed = document_resource_id(durable.source_id, DocumentId::from_uuid(document));
    let discover = |graph: serde_json::Value| {
        Request::post("/v1/discover")
            .header(header::AUTHORIZATION, "Bearer editor-token")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "need": {
                        "purpose": "find the current rule",
                        "requiredResourceTypes": ["knowledge"],
                        "requiredClaimIds": [Uuid::now_v7().to_string()],
                    },
                    "coverage": "titleAndPermittedMetadata",
                    "graph": graph,
                })
                .to_string(),
            ))
            .unwrap()
    };
    let accepted = call(
        &router,
        discover(json!({
            "seedResourceIds": [seed.as_uuid().to_string()],
            "relationType": "document_current_placement",
            "fromRole": "document",
            "toRole": "current_version",
            "maxHops": 1
        })),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    // Out-of-range values are field errors.
    for (bad, pointer) in [
        (
            json!({"seedResourceIds": [], "relationType": "x", "fromRole": "a", "toRole": "b"}),
            "/graph/seedResourceIds",
        ),
        (
            json!({"seedResourceIds": [seed.as_uuid().to_string()], "relationType": "Bad Type",
                   "fromRole": "a", "toRole": "b"}),
            "/graph/relationType",
        ),
        (
            json!({"seedResourceIds": [seed.as_uuid().to_string()], "relationType": "x",
                   "fromRole": "a", "toRole": "b", "maxHops": 4}),
            "/graph/maxHops",
        ),
    ] {
        let refused = call(&router, discover(bad)).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.body
        );
        assert_eq!(
            refused.body["errors"][0]["pointer"], pointer,
            "{}",
            refused.body
        );
    }
    // A caller can never name the access context or any other field.
    let unknown = call(
        &router,
        discover(
            json!({"seedResourceIds": [seed.as_uuid().to_string()], "relationType": "x",
                        "fromRole": "a", "toRole": "b", "accessContext": "other"}),
        ),
    )
    .await;
    assert_eq!(
        unknown.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        unknown.body
    );
}
