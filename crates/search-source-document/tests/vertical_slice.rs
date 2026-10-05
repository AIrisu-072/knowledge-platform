#[path = "support/document_discovery.rs"]
mod discovery_support;
#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod versioning_support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use document_application::{
    AccessPolicyService, BootstrapRootPolicy, DocumentAccessCheckService,
    DocumentPublicationEndService, EndDocumentPublicationCommand, InvocationKind,
    ManagementCommand, ManagementOperationId, PublicationEndOperationId, VerifiedActorContext,
};
use document_domain::{
    Action, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use search_application::discovery_service::{DiscoveryPorts, DiscoveryService};
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelectorPort, CurrentAccessEvaluatorPort,
    DirectoryRetrieverPort, EvidenceResolverPort, HyperGraphRetrieverPort, LexicalQuery,
    LexicalRetrieverPort, StructuredFacetFilter, StructuredRetrieverPort,
};
use search_application::retrieval::{RetrieverProfile, RetrieverSupport};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate, GapReason};
use search_core::evidence::{ClaimState, EvidenceRole, EvidenceSufficiency};
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_source_document::{
    DocumentCoveragePreflight, DocumentCoverageRequirement, DocumentCurrentAccessAdapter,
    DocumentEvidenceCatalog, DocumentEvidenceField, DocumentHistoricalLookup, DocumentIndexRuntime,
    DocumentLexicalReader, DocumentOutboxIndexer, DocumentSourceTranslation,
    DocumentSourceTranslator, MemoryDocumentIndexRuntime, PostgresDocumentSnapshotReader,
    document_resource_id, folder_resource_id,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        versioning_support::actor(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

async fn grant_actions(
    fixture: &versioning_support::Fixture,
    actions: impl IntoIterator<Item = Action>,
) -> Arc<PostgresDocumentRepository> {
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        fixture.pool.clone(),
        versioning_support::actor(),
    ));
    repository
        .initialize_root_policy(
            &actor(),
            vec![
                PolicyGrant::new(
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
                    actions,
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    repository
}

fn plan(
    request: &search_core::discovery::DiscoveryRequest,
    source: SourceId,
    document: ResourceId,
    folder: ResourceId,
) -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: vec![document],
        path_patterns: vec![
            RelationPathPattern::new(
                RelationNamespace::Discovery,
                "document_current_placement",
                "document",
                "current_version",
            )
            .with_participant("folder", folder),
        ],
        allowed_relation_types: vec!["document_current_placement".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some(source.as_uuid().to_string()),
        temporal_context: Some(request.temporal_context.clone()),
        access_context: request.access_context.clone(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 2,
            max_branching_per_node: 2,
            max_seed_nodes: 1,
            max_paths: 2,
        },
        stop_conditions: vec![],
    }
}

struct CountedLexical {
    inner: DocumentLexicalReader,
    calls: AtomicUsize,
}

impl LexicalRetrieverPort for CountedLexical {
    fn retrieve<'a>(
        &'a self,
        generation: search_core::projection::ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.retrieve(generation, request, query)
    }
}

#[tokio::test]
async fn graph_only_current_version_qualifies_with_source_title_evidence_and_hard_gates() {
    let fixture = versioning_support::fixture().await;
    sqlx::query("UPDATE documents SET metadata = '{\"document_type\":\"policy\",\"category\":\"guide\"}' WHERE document_id = $1")
        .bind(fixture.document_id.as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    let repository = grant_actions(&fixture, [Action::Read, Action::Administer]).await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let access = Arc::new(DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    ));
    let runtime = MemoryDocumentIndexRuntime::with_current_access(access.clone());
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    let generation = match indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let reader = runtime.projection_reader();
    let lexical = runtime.lexical_reader();
    let graph = runtime.graph_reader();
    let graph_access = runtime.graph_access_reader();
    let request = discovery_support::request();
    let version = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let document = document_resource_id(source_id, fixture.document_id);
    let folder = folder_resource_id(
        source_id,
        fixture.document_id,
        document_domain::FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
    );
    let graph_plan = plan(&request, source_id, document, folder);
    assert_eq!(
        graph_access
            .evaluate(document, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    assert_eq!(
        graph_access
            .evaluate(folder, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    assert!(
        lexical
            .retrieve(generation, &request, &LexicalQuery::new("missingword", 10))
            .await
            .unwrap()
            .is_empty()
    );
    let paths = graph.retrieve(generation, &graph_plan).await.unwrap();
    assert_eq!(paths.hits.len(), 1, "{paths:?}");
    assert_eq!(paths.hits[0].candidate.resource_ref, Some(version));
    let step = &paths.hits[0].paths[0].steps[0];
    assert_eq!(step.from_role, "document");
    assert_eq!(step.to_role, "current_version");
    assert_eq!(step.participants.len(), 3);
    assert_eq!(
        step.provenance.as_deref(),
        Some(fixture.document_id.as_uuid().to_string().as_str())
    );

    let wrong_generation = search_core::projection::ProjectionGenerationKey {
        source_id,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
    };
    assert!(graph.retrieve(wrong_generation, &graph_plan).await.is_err());
    let mut role_swapped = graph_plan.clone();
    role_swapped.path_patterns[0].to_role = "version".into();
    assert!(
        graph
            .retrieve(generation, &role_swapped)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut false_composite = graph_plan.clone();
    false_composite.path_patterns[0].required_participants[0].resource_ref =
        ResourceId::from_uuid(Uuid::now_v7());
    assert!(
        graph
            .retrieve(generation, &false_composite)
            .await
            .unwrap()
            .hits
            .is_empty()
    );

    let mut evidence = DocumentEvidenceCatalog::new(reader.clone());
    let claim_id = request.need.required_claims[0];
    evidence.bind_claim(
        claim_id,
        version,
        DocumentEvidenceField::Title,
        Some("Base".into()),
    );
    let assertion = reader
        .assertions_for(generation, version, "document.title")
        .await
        .unwrap();
    assert_eq!(assertion.len(), 1);
    let locator = &assertion[0].evidence_refs[0];
    assert!(locator.contains(&generation.generation_id.as_uuid().to_string()));
    assert_eq!(
        evidence
            .resolve(generation, version, locator)
            .await
            .unwrap()
            .unwrap()
            .role,
        EvidenceRole::Primary
    );
    assert!(
        evidence
            .resolve(generation, version, "dsi:opaque")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        evidence
            .resolve(wrong_generation, version, locator)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        evidence
            .selector_for(generation, claim_id)
            .await
            .unwrap()
            .is_some()
    );

    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let mut config = discovery_support::discovery_config(source_id, "missingword");
    config.retriever_profile = RetrieverProfile::Exploratory;
    config.retriever_support = RetrieverSupport {
        lexical: true,
        hypergraph: true,
        structured: true,
        ..RetrieverSupport::default()
    };
    config.retrieval_inputs.graph_plans = BTreeMap::from([(source_id, graph_plan)]);
    config.retrieval_inputs.max_initial_retrievers_per_source = 3;
    config.structured_filters = vec![StructuredFacetFilter::eq(
        "document_type",
        TypedValue::String("policy".into()),
    )];
    config.max_actions = 3;
    let service = DiscoveryService::new(
        config.clone(),
        DiscoveryPorts {
            sources: &sources,
            generations: &reader,
            concepts: &reader,
            retrieval: RetrievalExecutionPorts {
                remote: None,
                directory: Some(&reader),
                structured: Some(&reader),
                lexical: Some(&lexical),
                hypergraph: Some(&graph),
                graph_resource_access: Some(&graph_access),
                access: access.as_ref(),
            },
            selectors: &evidence,
            assertions: &reader,
            evidence: &evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let found = service.discover(request.clone()).await.unwrap();
    assert_eq!(
        found.evidence_sufficiency,
        EvidenceSufficiency::Sufficient,
        "{found:?}"
    );
    assert_eq!(found.qualified_resources.len(), 1, "{found:?}");
    assert_eq!(found.qualified_resources[0].resource_ref, version);
    assert!(
        found
            .evidence_set
            .iter()
            .any(|claim| claim.state == ClaimState::Supported)
    );
    assert!(found.qualified_resources[0].evidence_refs.contains(locator));
    assert!(
        found
            .retrieval_trace
            .iter()
            .any(|trace| trace.contains("HyperGraph"))
    );

    config.structured_filters[0] =
        StructuredFacetFilter::eq("document_type", TypedValue::String("wrong".into()));
    let mismatch = DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: &sources,
            generations: &reader,
            concepts: &reader,
            retrieval: RetrievalExecutionPorts {
                remote: None,
                directory: Some(&reader),
                structured: Some(&reader),
                lexical: Some(&lexical),
                hypergraph: Some(&graph),
                graph_resource_access: Some(&graph_access),
                access: access.as_ref(),
            },
            selectors: &evidence,
            assertions: &reader,
            evidence: &evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap()
    .discover(request.clone())
    .await
    .unwrap();
    assert!(mismatch.qualified_resources.is_empty(), "{mismatch:?}");

    AccessPolicyService::new(repository)
        .set_access_policy(
            &actor(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(fixture.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![
                    PolicyGrant::new(
                        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                            .unwrap(),
                        [Action::Administer],
                    )
                    .unwrap(),
                ]),
                reason: "revoke current read".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reader.pin_current(source_id).await.unwrap().unwrap().key(),
        generation
    );
    let denied = service.discover(request).await.unwrap();
    assert!(denied.qualified_resources.is_empty(), "{denied:?}");
    assert!(
        denied
            .evidence_set
            .iter()
            .all(|claim| { claim.subject.is_none() && claim.evidence_refs.is_empty() })
    );
    let encoded = format!("{denied:?}");
    for secret in [
        version.as_uuid().to_string(),
        document.as_uuid().to_string(),
        folder.as_uuid().to_string(),
        locator.clone(),
    ] {
        assert!(
            !encoded.contains(&secret),
            "revoked identity leaked: {encoded}"
        );
    }
}

#[tokio::test]
async fn explicit_t10_removes_live_and_targeted_history_requires_both_permissions() {
    let fixture = versioning_support::fixture().await;
    let repository = grant_actions(
        &fixture,
        [Action::Read, Action::ReadHistory, Action::Administer],
    )
    .await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let config = discovery_support::index_config(source_id);
    let translator = DocumentSourceTranslator::new(
        config.source.clone(),
        config.lens.clone(),
        config.projection_schema_version.clone(),
        config.semantic_registry.version.clone(),
    );
    let runtime = MemoryDocumentIndexRuntime::new();
    let reader = PostgresDocumentSnapshotReader::new(fixture.pool.clone());
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        reader.clone(),
        config,
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap();
    DocumentPublicationEndService::new(
        Arc::new(versioning_support::TestIds),
        fixture.clock.clone(),
        fixture.repository.clone(),
    )
    .end_document_publication(
        EndDocumentPublicationCommand::new(
            PublicationEndOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
            fixture.document_id,
            1,
            fixture.base_id,
            versioning_support::actor(),
            "publication period ended".into(),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let generation = match indexer
        .handle(discovery_support::event(
            "DocumentPublicationEnded",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let manifest = runtime
        .projection_reader()
        .pin_current(source_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manifest.key(), generation);
    assert_eq!(manifest.resource_count, 0);
    assert_eq!(manifest.relation_count, Some(0));
    let lookup = DocumentHistoricalLookup::new(reader, repository.clone(), actor(), translator);
    let historical = lookup
        .lookup(fixture.document_id, fixture.base_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        historical,
        DocumentSourceTranslation::Historical { .. }
    ));
    assert!(
        lookup
            .lookup(
                fixture.document_id,
                document_domain::DocumentVersionId::from_uuid(Uuid::now_v7()),
            )
            .await
            .unwrap()
            .is_none()
    );

    for (revision, actions) in [
        (0, vec![Action::Read, Action::Administer]),
        (1, vec![Action::ReadHistory, Action::Administer]),
    ] {
        AccessPolicyService::new(repository.clone())
            .set_access_policy(
                &actor(),
                ManagementCommand::SetAccessPolicy {
                    operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                    target: PolicyTarget::Document(fixture.document_id),
                    expected_policy_revision: revision,
                    mode: PolicyMode::Explicit(vec![
                        PolicyGrant::new(
                            PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                                .unwrap(),
                            actions,
                        )
                        .unwrap(),
                    ]),
                    reason: "history access boundary".into(),
                },
            )
            .await
            .unwrap();
        assert!(
            lookup
                .lookup(fixture.document_id, fixture.base_id)
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn body_required_coverage_returns_gap_before_real_title_lexical_port_is_called() {
    let fixture = versioning_support::fixture().await;
    let repository = grant_actions(&fixture, [Action::Read, Action::Administer]).await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let access = DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap();
    let reader = runtime.projection_reader();
    let lexical = CountedLexical {
        inner: runtime.lexical_reader(),
        calls: AtomicUsize::new(0),
    };
    let evidence = DocumentEvidenceCatalog::new(reader.clone());
    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let service = DiscoveryService::new(
        discovery_support::discovery_config(source_id, "Base"),
        DiscoveryPorts {
            sources: &sources,
            generations: &reader,
            concepts: &reader,
            retrieval: RetrievalExecutionPorts {
                remote: None,
                directory: Some(&reader),
                structured: Some(&reader),
                lexical: Some(&lexical),
                hypergraph: None,
                graph_resource_access: None,
                access: &access,
            },
            selectors: &evidence,
            assertions: &reader,
            evidence: &evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let result = DocumentCoveragePreflight::discover(
        &service,
        discovery_support::request(),
        DocumentCoverageRequirement::BodyRequired,
        None,
    )
    .await
    .unwrap();
    assert_eq!(lexical.calls.load(Ordering::SeqCst), 0);
    assert!(result.qualified_resources.is_empty());
    assert!(result.rejected_candidates.is_empty());
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.reason == GapReason::UnsupportedCoverage && gap.blocking)
    );
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
}

#[tokio::test]
async fn shared_folder_placements_inherit_only_their_own_documents_read_policy() {
    let fixture = versioning_support::fixture().await;
    let second_document = document_domain::DocumentId::from_uuid(Uuid::now_v7());
    let second_version = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
         VALUES ($1,$2,NULL,1,'{}',to_timestamp(0))",
    )
    .bind(second_document.as_uuid())
    .bind(SYSTEM_ROOT_FOLDER_ID)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
         VALUES ($1,$2,1,'PUBLISHED','Other',to_timestamp(0),'test-idp','editor','{}',to_timestamp(0))",
    )
    .bind(second_version.as_uuid())
    .bind(second_document.as_uuid())
    .execute(&fixture.pool)
    .await
    .unwrap();
    let second_file = Uuid::now_v7();
    let second_item = Uuid::now_v7();
    let second_representation = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
         VALUES ($1,$2,'text/plain',8,$3,now())",
    )
    .bind(second_file)
    .bind(vec![7_u8; 32])
    .bind(format!("objects/{second_file}"))
    .execute(&fixture.pool)
    .await
    .unwrap();
    let mut item_tx = fixture.pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
         VALUES ($1,$2,'primary',0,$3)",
    )
    .bind(second_item)
    .bind(second_version.as_uuid())
    .bind(second_representation)
    .execute(&mut *item_tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
         VALUES ($1,$2,$3,'AUTHORITATIVE','source.txt')",
    )
    .bind(second_representation)
    .bind(second_item)
    .bind(second_file)
    .execute(&mut *item_tx)
    .await
    .unwrap();
    item_tx.commit().await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(second_version.as_uuid())
        .bind(second_document.as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    let repository = grant_actions(&fixture, [Action::Read, Action::Administer]).await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let access = Arc::new(DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    ));
    let runtime = MemoryDocumentIndexRuntime::with_current_access(access.clone());
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    let key = match indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let first_folder = folder_resource_id(
        source_id,
        fixture.document_id,
        document_domain::FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
    );
    let second_folder = folder_resource_id(
        source_id,
        second_document,
        document_domain::FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
    );
    assert_ne!(first_folder, second_folder);
    let manifest = runtime
        .projection_reader()
        .pin_current(source_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manifest.resource_count, 6);
    assert_eq!(manifest.relation_count, Some(4));
    let graph_access = runtime.graph_access_reader();
    let context = discovery_support::ACCESS_CONTEXT;
    assert_eq!(
        graph_access.evaluate(first_folder, context).await.unwrap(),
        AccessDecision::Allowed
    );
    assert_eq!(
        graph_access.evaluate(second_folder, context).await.unwrap(),
        AccessDecision::Allowed
    );
    let request = discovery_support::request();
    let first_doc = document_resource_id(source_id, fixture.document_id);
    let second_doc = document_resource_id(source_id, second_document);
    let first_version = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let second_version_ref = ResourceId::from_uuid(second_version.as_uuid());
    let graph_plan = plan(&request, source_id, first_doc, first_folder);
    let mut crossed = graph_plan.clone();
    crossed.path_patterns[0].required_participants[0].resource_ref = second_folder;
    assert!(
        runtime
            .graph_reader()
            .retrieve(key, &crossed)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let reader = runtime.projection_reader();
    let lexical = runtime.lexical_reader();
    let graph = runtime.graph_reader();
    let mut evidence = DocumentEvidenceCatalog::new(reader.clone());
    evidence.bind_claim(
        request.need.required_claims[0],
        first_version,
        DocumentEvidenceField::Title,
        Some("Base".into()),
    );
    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let mut config = discovery_support::discovery_config(source_id, "Other");
    config.retriever_profile = RetrieverProfile::Exploratory;
    config.retriever_support = RetrieverSupport {
        lexical: true,
        hypergraph: true,
        ..RetrieverSupport::default()
    };
    config.retrieval_inputs.graph_plans = BTreeMap::from([(source_id, graph_plan)]);
    config.retrieval_inputs.max_initial_retrievers_per_source = 2;
    config.max_actions = 2;
    let discovery = DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: &sources,
            generations: &reader,
            concepts: &reader,
            retrieval: RetrievalExecutionPorts {
                remote: None,
                directory: Some(&reader),
                structured: Some(&reader),
                lexical: Some(&lexical),
                hypergraph: Some(&graph),
                graph_resource_access: Some(&graph_access),
                access: access.as_ref(),
            },
            selectors: &evidence,
            assertions: &reader,
            evidence: &evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let ranked = discovery.discover(request.clone()).await.unwrap();
    assert_eq!(ranked.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(
        ranked
            .evidence_set
            .iter()
            .any(|claim| claim.state == ClaimState::Supported)
    );
    assert_eq!(
        ranked
            .qualified_resources
            .iter()
            .map(|item| item.resource_ref)
            .collect::<Vec<_>>(),
        vec![second_version_ref, first_version],
        "S1 must retain retriever order after hard gates: {ranked:?}",
    );
    AccessPolicyService::new(repository)
        .set_access_policy(
            &actor(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(fixture.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![
                    PolicyGrant::new(
                        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                            .unwrap(),
                        [Action::Administer],
                    )
                    .unwrap(),
                ]),
                reason: "deny only first document".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        key
    );
    assert_eq!(
        graph_access.evaluate(first_folder, context).await.unwrap(),
        AccessDecision::Denied
    );
    assert_eq!(
        graph_access.evaluate(second_folder, context).await.unwrap(),
        AccessDecision::Allowed
    );
    assert!(
        runtime
            .graph_reader()
            .retrieve(key, &plan(&request, source_id, first_doc, first_folder))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let allowed = runtime
        .graph_reader()
        .retrieve(key, &plan(&request, source_id, second_doc, second_folder))
        .await
        .unwrap();
    assert_eq!(allowed.hits.len(), 1);
    assert_eq!(
        allowed.hits[0].candidate.resource_ref,
        Some(ResourceId::from_uuid(second_version.as_uuid()))
    );
}

#[tokio::test]
async fn current_authoritative_snapshot_publishes_typed_graph_participants_without_lexical_auxiliaries()
 {
    let fixture = versioning_support::fixture().await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    let generation = match indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published generation: {other:?}"),
    };
    let reader = runtime.projection_reader();
    let manifest = reader.pin_current(source_id).await.unwrap().unwrap();
    assert_eq!(manifest.key(), generation);
    assert_eq!(
        manifest.graph_schema_version.as_deref(),
        Some("typed-nary-v1")
    );
    assert_eq!(manifest.resource_count, 3);
    assert_eq!(manifest.relation_count, Some(2));
    let version = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let document = document_resource_id(source_id, fixture.document_id);
    let folder = folder_resource_id(
        source_id,
        fixture.document_id,
        document_domain::FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
    );
    let mut canonical_relations = BTreeMap::new();
    for (resource, expected_kind) in [
        (version, ResourceKind::Knowledge),
        (document, ResourceKind::Document),
        (folder, ResourceKind::FolderPlacement),
    ] {
        let projection = reader
            .resource_at(generation, resource)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(projection.directory.kind, expected_kind);
        assert!(projection.relations.iter().all(|relation| {
            relation
                .participants
                .iter()
                .any(|participant| participant.resource_ref == resource)
        }));
        for relation in projection.relations {
            if let Some(previous) =
                canonical_relations.insert(relation.relation_id, relation.clone())
            {
                assert_eq!(
                    previous, relation,
                    "participants must carry one canonical definition"
                );
            }
        }
    }
    assert_eq!(canonical_relations.len(), 2);
    let request = discovery_support::request();
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(generation, &request, &LexicalQuery::new("Base", 10))
            .await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        DirectoryRetrieverPort::retrieve(&reader, generation, &request)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        StructuredRetrieverPort::retrieve(&reader, generation, &request, &[])
            .await
            .unwrap()
            .len(),
        1
    );

    let mut incomplete_manifest = manifest.clone();
    incomplete_manifest.generation_id = ProjectionGenerationId::from_uuid(Uuid::now_v7());
    incomplete_manifest.resource_count = 1;
    let mut orphan = reader
        .resource_at(generation, version)
        .await
        .unwrap()
        .unwrap();
    orphan.manifest = incomplete_manifest.clone();
    let error = DocumentIndexRuntime::build_graph_generation(
        &runtime,
        incomplete_manifest.clone(),
        &discovery_support::index_config(source_id).source,
        vec![orphan],
        vec![],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("missing generation participant"),
        "{error}"
    );
    assert_eq!(
        reader.pin_current(source_id).await.unwrap().unwrap().key(),
        generation
    );
    let graph_plan = plan(&request, source_id, document, folder);
    assert!(
        runtime
            .graph_reader()
            .retrieve(generation, &graph_plan)
            .await
            .is_ok()
    );
    assert!(
        runtime
            .graph_reader()
            .retrieve(incomplete_manifest.key(), &graph_plan)
            .await
            .is_err()
    );
}
