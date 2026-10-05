use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, HyperGraphRetrieverPort,
};
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{
    DiscoveryEvaluationId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::observation::Coverage;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
use search_graph_memory::{GraphIndexError, MemoryGraphRetriever};
use time::OffsetDateTime;
use uuid::Uuid;

fn source(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}
fn resource(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
fn relation(n: u128) -> RelationId {
    RelationId::from_uuid(Uuid::from_u128(n))
}
fn at(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(n).unwrap()
}

fn manifest(
    owner: SourceId,
    generation: u128,
    count: usize,
    relation_count: usize,
) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: owner,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: Some("typed-nary-v1".into()),
        source_snapshot: format!("snapshot-{generation}"),
        resource_count: count as u64,
        relation_count: Some(relation_count as u64),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("digest-{generation}"),
        built_at: at(100),
    }
}

fn owner(id: SourceId, retention: RetentionMode) -> DiscoverableSource {
    DiscoverableSource::new(id, "test", EnumerationSemantics::Complete, retention)
}

fn projection(
    manifest: &ProjectionGenerationManifest,
    id: ResourceId,
) -> CompiledResourceProjection {
    CompiledResourceProjection {
        manifest: manifest.clone(),
        retention_mode: RetentionMode::PersistentDiscoveryMetadata,
        directory: DirectoryProjection {
            resource_ref: id,
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: id.as_uuid().to_string(),
            title: None,
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: id,
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets: BTreeMap::new(),
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: id,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: id,
            access_scope: None,
            source_access_model: None,
        },
        relations: vec![],
    }
}

fn loan(
    id: u128,
    borrower: ResourceId,
    product: ResourceId,
    collateral: ResourceId,
) -> TypedRelationInstance {
    let mut result = TypedRelationInstance::new(
        relation(id),
        RelationNamespace::Discovery,
        "loan",
        vec![
            RelationParticipant::new("borrower", borrower),
            RelationParticipant::new("product", product),
            RelationParticipant::new("collateral", collateral),
        ],
    );
    result.authority = Some("finance:authoritative".into());
    result.evidence_refs = vec![format!("evidence-{id}")];
    result
}

fn batch(
    manifest: &ProjectionGenerationManifest,
    relations: Vec<TypedRelationInstance>,
) -> Vec<CompiledResourceProjection> {
    let ids: BTreeSet<_> = relations
        .iter()
        .flat_map(|r| r.participants.iter().map(|p| p.resource_ref))
        .collect();
    let mut result: Vec<_> = ids.into_iter().map(|id| projection(manifest, id)).collect();
    for relation in relations {
        let anchor = relation.participants[0].resource_ref;
        result
            .iter_mut()
            .find(|p| p.directory.resource_ref == anchor)
            .unwrap()
            .relations
            .push(relation);
    }
    result
}

#[derive(Default)]
struct Access {
    decisions: BTreeMap<ResourceId, AccessDecision>,
}
impl CurrentAccessEvaluatorPort for Access {
    fn evaluate<'a>(&'a self, id: ResourceId, _context: &'a str) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(*self.decisions.get(&id).unwrap_or(&AccessDecision::Allowed)) })
    }
}

struct CountingAccess(Arc<AtomicUsize>);
impl CurrentAccessEvaluatorPort for CountingAccess {
    fn evaluate<'a>(&'a self, _id: ResourceId, _context: &'a str) -> BoxFuture<'a, AccessDecision> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

struct FailingAccess;
impl CurrentAccessEvaluatorPort for FailingAccess {
    fn evaluate<'a>(&'a self, _id: ResourceId, _context: &'a str) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Err(SearchError::SourceUnavailable("secret marker".into())) })
    }
}

fn plan(seed: ResourceId, steps: Vec<RelationPathPattern>) -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: vec![seed],
        path_patterns: steps,
        allowed_relation_types: vec!["loan".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some("finance:authoritative".into()),
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(999)),
            at(100),
            at(100),
            "UTC",
        )),
        access_context: "tenant:example".into(),
        expansion_budget: TraversalBudget {
            max_hops: 2,
            max_relations: 10,
            max_branching_per_node: 3,
            max_seed_nodes: 10,
            max_paths: 10,
        },
        stop_conditions: vec![],
    }
}

fn step(from: &str, to: &str) -> RelationPathPattern {
    RelationPathPattern::new(RelationNamespace::Discovery, "loan", from, to)
}

#[tokio::test]
async fn raw_seed_count_is_rejected_before_access_even_if_seeds_repeat() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let graph = MemoryGraphRetriever::new(Arc::new(CountingAccess(calls.clone())));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![loan(101, resource(1), resource(2), resource(10))],
            ),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.seed_nodes.push(resource(1));
    query.expansion_budget.max_seed_nodes = 1;
    assert!(matches!(
        graph.retrieve(manifest.key(), &query).await,
        Err(search_application::SearchError::InvalidRequest(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn visible_seed_frontier_obeys_path_budget() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![loan(101, resource(1), resource(2), resource(10))],
            ),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.seed_nodes.push(resource(2));
    query.expansion_budget.max_paths = 1;
    assert!(matches!(
        graph.retrieve(manifest.key(), &query).await,
        Err(search_application::SearchError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn relation_targets_obey_path_budget_before_expansion() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 4, 1);
    let mut multi = loan(101, resource(1), resource(2), resource(10));
    multi
        .participants
        .push(RelationParticipant::new("product", resource(3)));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, vec![multi]),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.expansion_budget.max_paths = 1;
    assert!(matches!(
        graph.retrieve(manifest.key(), &query).await,
        Err(search_application::SearchError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn undefined_stop_condition_is_rejected_before_access() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let graph = MemoryGraphRetriever::new(Arc::new(CountingAccess(calls.clone())));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![loan(101, resource(1), resource(2), resource(10))],
            ),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.stop_conditions.push("sufficient-evidence".into());
    assert!(matches!(
        graph.retrieve(manifest.key(), &query).await,
        Err(search_application::SearchError::InvalidRequest(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn false_composite_and_role_swap_do_not_escape_one_relation() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 3);
    let relations = vec![
        loan(101, resource(1), resource(2), resource(10)),
        loan(102, resource(1), resource(3), resource(11)),
        loan(103, resource(2), resource(1), resource(10)),
    ];
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, relations),
        )
        .unwrap();
    let exact = step("borrower", "product").with_participant("collateral", resource(10));
    let found = graph
        .retrieve(manifest.key(), &plan(resource(1), vec![exact]))
        .await
        .unwrap();
    assert_eq!(found.generation, manifest.key());
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].candidate.resource_ref, Some(resource(2)));
    assert_eq!(
        found.hits[0].candidate.candidate_id,
        format!("{}:{}", owner_id.as_uuid(), resource(2).as_uuid())
    );
    assert_eq!(found.hits[0].paths[0].steps[0].relation_id, relation(101));
    assert_eq!(found.hits[0].paths[0].steps[0].participants.len(), 3);
    assert_eq!(
        found.hits[0].paths[0].steps[0].evidence_refs,
        vec!["evidence-101"]
    );
    let composite = step("borrower", "product")
        .with_endpoints(resource(1), resource(2))
        .with_participant("collateral", resource(11));
    assert!(
        graph
            .retrieve(manifest.key(), &plan(resource(1), vec![composite]))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}

#[tokio::test]
async fn endpoint_constraint_applies_to_each_emitted_target() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 4, 1);
    let mut multi = loan(101, resource(1), resource(2), resource(10));
    multi
        .participants
        .push(RelationParticipant::new("product", resource(3)));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, vec![multi]),
        )
        .unwrap();
    let exact = step("borrower", "product").with_endpoints(resource(1), resource(2));
    let found = graph
        .retrieve(manifest.key(), &plan(resource(1), vec![exact]))
        .await
        .unwrap();
    assert_eq!(
        found
            .hits
            .iter()
            .map(|hit| hit.candidate.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(resource(2))]
    );
}

#[tokio::test]
async fn from_endpoint_constraint_applies_to_current_seed_not_another_participant() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 4, 1);
    let mut multi = loan(101, resource(1), resource(3), resource(10));
    multi
        .participants
        .push(RelationParticipant::new("borrower", resource(2)));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, vec![multi]),
        )
        .unwrap();
    let exact = step("borrower", "product").with_endpoints(resource(1), resource(3));
    assert!(
        graph
            .retrieve(manifest.key(), &plan(resource(2), vec![exact]))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}

#[tokio::test]
async fn relation_budget_counts_relation_expansions_not_same_relation_targets() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 4, 1);
    let mut multi = loan(101, resource(1), resource(2), resource(10));
    multi
        .participants
        .push(RelationParticipant::new("product", resource(3)));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, vec![multi]),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.expansion_budget.max_relations = 1;
    let found = graph.retrieve(manifest.key(), &query).await.unwrap();
    assert_eq!(found.hits.len(), 2);
}

#[tokio::test]
async fn ordered_two_step_path_keeps_each_relation_roles_and_evidence() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 2);
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![
                    loan(101, resource(1), resource(2), resource(10)),
                    loan(102, resource(1), resource(3), resource(11)),
                ],
            ),
        )
        .unwrap();
    let found = graph
        .retrieve(
            manifest.key(),
            &plan(
                resource(2),
                vec![step("product", "borrower"), step("borrower", "collateral")],
            ),
        )
        .await
        .unwrap();
    let path = found
        .hits
        .iter()
        .find(|hit| hit.candidate.resource_ref == Some(resource(11)))
        .unwrap()
        .paths
        .first()
        .unwrap();
    assert_eq!(
        path.resource_path,
        vec![resource(2), resource(1), resource(11)]
    );
    assert_eq!(
        path.steps.iter().map(|s| s.relation_id).collect::<Vec<_>>(),
        vec![relation(101), relation(102)]
    );
    assert_eq!(path.steps[0].from_role, "product");
    assert_eq!(path.steps[1].to_role, "collateral");
    assert_eq!(path.steps[1].evidence_refs, vec!["evidence-102"]);
}

#[tokio::test]
async fn ordered_steps_can_reuse_one_n_ary_relation_without_false_composite() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![loan(101, resource(1), resource(2), resource(10))],
            ),
        )
        .unwrap();
    let query = plan(
        resource(1),
        vec![step("borrower", "product"), step("product", "collateral")],
    );
    let result = graph.retrieve(manifest.key(), &query).await.unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].candidate.resource_ref, Some(resource(10)));
    assert_eq!(
        result.hits[0].paths[0]
            .steps
            .iter()
            .map(|step| step.relation_id)
            .collect::<Vec<_>>(),
        vec![relation(101), relation(101)]
    );
}

#[tokio::test]
async fn denied_or_unknown_seed_target_and_any_participant_are_invisible() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let build = |access: Access| {
        let graph = MemoryGraphRetriever::new(Arc::new(access));
        graph
            .build_generation(
                manifest.clone(),
                &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
                batch(
                    &manifest,
                    vec![loan(101, resource(1), resource(2), resource(10))],
                ),
            )
            .unwrap();
        graph
    };
    let query = plan(resource(1), vec![step("borrower", "product")]);
    for (id, decision) in [
        (resource(1), AccessDecision::Denied),
        (resource(2), AccessDecision::Unknown),
        (resource(10), AccessDecision::Denied),
    ] {
        let graph = build(Access {
            decisions: BTreeMap::from([(id, decision)]),
        });
        assert!(
            graph
                .retrieve(manifest.key(), &query)
                .await
                .unwrap()
                .hits
                .is_empty()
        );
    }
    let mut metadata = batch(
        &manifest,
        vec![loan(101, resource(1), resource(2), resource(10))],
    );
    metadata
        .iter_mut()
        .find(|p| p.directory.resource_ref == resource(10))
        .unwrap()
        .temporal
        .valid_to = Some(at(90));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            metadata,
        )
        .unwrap();
    assert!(
        graph
            .retrieve(manifest.key(), &query)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}

#[tokio::test]
async fn access_error_for_indexed_seed_is_indistinguishable_from_absent_seed() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 3, 1);
    let graph = MemoryGraphRetriever::new(Arc::new(FailingAccess));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![loan(101, resource(1), resource(2), resource(10))],
            ),
        )
        .unwrap();
    let existing = graph
        .retrieve(
            manifest.key(),
            &plan(resource(1), vec![step("borrower", "product")]),
        )
        .await;
    let absent = graph
        .retrieve(
            manifest.key(),
            &plan(resource(999), vec![step("borrower", "product")]),
        )
        .await
        .unwrap();
    assert_eq!(existing.unwrap(), absent);
}

#[tokio::test]
async fn authority_and_temporal_filters_precede_branch_budget() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 2);
    let mut invalid = loan(101, resource(1), resource(2), resource(10));
    invalid.temporal_scope.valid_to = Some(at(90));
    let valid = loan(102, resource(1), resource(3), resource(11));
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, vec![invalid, valid]),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.expansion_budget.max_branching_per_node = 1;
    let found = graph.retrieve(manifest.key(), &query).await.unwrap();
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].candidate.resource_ref, Some(resource(3)));
    query.authority_requirement = Some("other".into());
    assert!(
        graph
            .retrieve(manifest.key(), &query)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}

#[tokio::test]
async fn inaccessible_relation_is_removed_before_branch_budget() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 2);
    let access = Access {
        decisions: BTreeMap::from([(resource(10), AccessDecision::Denied)]),
    };
    let graph = MemoryGraphRetriever::new(Arc::new(access));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![
                    loan(101, resource(1), resource(2), resource(10)),
                    loan(102, resource(1), resource(3), resource(11)),
                ],
            ),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.expansion_budget.max_branching_per_node = 1;
    let found = graph.retrieve(manifest.key(), &query).await.unwrap();
    assert_eq!(
        found
            .hits
            .iter()
            .map(|hit| hit.candidate.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(resource(3))]
    );
}

#[tokio::test]
async fn generations_are_immutable_source_local_and_retention_gated() {
    let first = source(1);
    let second = source(2);
    let m1 = manifest(first, 1, 3, 1);
    let m2 = manifest(first, 2, 3, 1);
    let m3 = manifest(second, 1, 3, 1);
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            m1.clone(),
            &owner(first, RetentionMode::PersistentDiscoveryMetadata),
            batch(&m1, vec![loan(101, resource(1), resource(2), resource(10))]),
        )
        .unwrap();
    graph
        .build_generation(
            m2.clone(),
            &owner(first, RetentionMode::PersistentDiscoveryMetadata),
            batch(&m2, vec![loan(102, resource(1), resource(3), resource(11))]),
        )
        .unwrap();
    graph
        .build_generation(
            m3.clone(),
            &owner(second, RetentionMode::PersistentDiscoveryMetadata),
            batch(&m3, vec![loan(103, resource(1), resource(4), resource(12))]),
        )
        .unwrap();
    let query = plan(resource(1), vec![step("borrower", "product")]);
    let targets = |result: search_application::ports::GraphRetrievalResult| {
        result
            .hits
            .iter()
            .map(|h| h.candidate.resource_ref.unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        targets(graph.retrieve(m1.key(), &query).await.unwrap()),
        vec![resource(2)]
    );
    assert_eq!(
        targets(graph.retrieve(m2.key(), &query).await.unwrap()),
        vec![resource(3)]
    );
    assert_eq!(
        targets(graph.retrieve(m3.key(), &query).await.unwrap()),
        vec![resource(4)]
    );
    assert!(matches!(
        graph.build_generation(
            m1.clone(),
            &owner(first, RetentionMode::PersistentDiscoveryMetadata),
            batch(&m1, vec![loan(104, resource(1), resource(5), resource(13))])
        ),
        Err(GraphIndexError::DuplicateGeneration)
    ));
    assert_eq!(
        targets(graph.retrieve(m1.key(), &query).await.unwrap()),
        vec![resource(2)]
    );
    assert!(matches!(
        graph.build_generation(
            m1.clone(),
            &owner(first, RetentionMode::NoRetention),
            batch(&m1, vec![loan(101, resource(1), resource(2), resource(10))])
        ),
        Err(GraphIndexError::PersistenceDenied)
    ));
    assert!(matches!(
        graph.build_generation(
            m1.clone(),
            &owner(second, RetentionMode::PersistentDiscoveryMetadata),
            batch(&m1, vec![loan(101, resource(1), resource(2), resource(10))])
        ),
        Err(GraphIndexError::SourceMismatch)
    ));
}

#[tokio::test]
async fn high_degree_and_relation_budgets_fail_instead_of_returning_partial_paths() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 2);
    let graph = MemoryGraphRetriever::new(Arc::new(Access::default()));
    graph
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(
                &manifest,
                vec![
                    loan(101, resource(1), resource(2), resource(10)),
                    loan(102, resource(1), resource(3), resource(11)),
                ],
            ),
        )
        .unwrap();
    let mut query = plan(resource(1), vec![step("borrower", "product")]);
    query.expansion_budget.max_branching_per_node = 1;
    assert!(graph.retrieve(manifest.key(), &query).await.is_err());
    query.expansion_budget.max_branching_per_node = 3;
    query.expansion_budget.max_relations = 1;
    assert!(graph.retrieve(manifest.key(), &query).await.is_err());
}

#[tokio::test]
async fn relation_participant_and_batch_permutations_preserve_deterministic_results() {
    let owner_id = source(1);
    let manifest = manifest(owner_id, 1, 5, 2);
    let relations = vec![
        loan(101, resource(1), resource(2), resource(10)),
        loan(102, resource(1), resource(3), resource(11)),
    ];
    let first = MemoryGraphRetriever::new(Arc::new(Access::default()));
    first
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            batch(&manifest, relations.clone()),
        )
        .unwrap();
    let second = MemoryGraphRetriever::new(Arc::new(Access::default()));
    let mut permuted = relations;
    permuted.reverse();
    for relation in &mut permuted {
        relation.participants.reverse();
    }
    let mut projections = batch(&manifest, permuted);
    projections.reverse();
    second
        .build_generation(
            manifest.clone(),
            &owner(owner_id, RetentionMode::PersistentDiscoveryMetadata),
            projections,
        )
        .unwrap();
    let query = plan(resource(1), vec![step("borrower", "product")]);
    assert_eq!(
        first.retrieve(manifest.key(), &query).await.unwrap(),
        second.retrieve(manifest.key(), &query).await.unwrap()
    );
}
