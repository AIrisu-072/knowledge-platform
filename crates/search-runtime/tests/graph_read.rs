//! P3-G07: pinned PostgreSQL Graph read. Same paths as the memory oracle,
//! hidden relations never change a public result, access `Unknown` fails the
//! whole read, and a pin that lapses before return discards every hit.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use bundle::*;
use search_application::SearchError;
use search_application::graph_generation::{
    GenerationScopedGraphAccessPort, GraphBuildRef, GraphLeaseVerifierPort, GraphReadLease,
};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, GraphRetrievalResult,
    HyperGraphRetrieverPort,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, AuthorizedSourceScope, CurrentSourceVisibilityPort,
    PrincipalRef, SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
    TrustedDiscoveryBinding,
};
use search_application::search_core::graph::{
    GraphTraversalPlan, RelationPathPattern, TraversalBudget,
};
use search_application::search_core::id::{DiscoveryEvaluationId, SessionId};
use search_application::search_core::temporal::TemporalEvaluationContext;
use search_graph::PostgresGraphReader;
use search_graph_memory::MemoryGraphRetriever;
use search_runtime::pin::{PgEvaluationPins, PinTtl};

struct Access {
    denied: BTreeSet<ResourceId>,
    unknown: BTreeSet<ResourceId>,
}

impl Access {
    fn decide(&self, id: ResourceId) -> AccessDecision {
        if self.unknown.contains(&id) {
            AccessDecision::Unknown
        } else if self.denied.contains(&id) {
            AccessDecision::Denied
        } else {
            AccessDecision::Allowed
        }
    }
}

impl GenerationScopedGraphAccessPort for Access {
    fn evaluate<'a>(
        &'a self,
        _key: &'a ProjectionGenerationKey,
        resource_ref: ResourceId,
        _binding: &'a TrustedDiscoveryBinding,
        _scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(self.decide(resource_ref)) })
    }
}

impl CurrentAccessEvaluatorPort for Access {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(self.decide(resource_ref)) })
    }
}

/// Test lease verifier: succeeds for the first `live_calls` checks.
struct Verifier {
    calls: AtomicUsize,
    live_calls: usize,
}

impl Verifier {
    fn new(live_calls: usize) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            live_calls,
        }
    }
}

impl GraphLeaseVerifierPort for Verifier {
    fn verify<'a>(
        &'a self,
        _lease: &'a GraphReadLease,
        _binding: &'a TrustedDiscoveryBinding,
        _scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.live_calls {
                Ok(())
            } else {
                Err(SearchError::SourceUnavailable("pin expired".into()))
            }
        })
    }
}

fn cites(id: u128, participants: &[(&str, u128)]) -> TypedRelationInstance {
    TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(id)),
        RelationNamespace::Discovery,
        "cites",
        participants
            .iter()
            .map(|(role, resource)| RelationParticipant::new(*role, rid(*resource)))
            .collect(),
    )
}

/// R1..R6 (101..106) and a denied H1 (201). Relation 305 has the denied
/// witness H1; `noisy` adds six denied targets of R1 (relations 310..315).
fn graph(noisy: bool) -> (Vec<ResourceId>, Vec<TypedRelationInstance>) {
    let mut relations = vec![
        cites(301, &[("source", 101), ("target", 102), ("witness", 103)]),
        cites(302, &[("source", 102), ("target", 104)]),
        cites(303, &[("source", 101), ("target", 105)]),
        cites(304, &[("source", 102), ("target", 106), ("witness", 105)]),
        cites(305, &[("source", 102), ("target", 104), ("witness", 201)]),
    ];
    let mut resources: Vec<ResourceId> = [101, 102, 103, 104, 105, 106, 201]
        .into_iter()
        .map(rid)
        .collect();
    if noisy {
        for n in 0..6 {
            resources.push(rid(211 + n));
            relations.push(cites(310 + n, &[("source", 101), ("target", 211 + n)]));
        }
    }
    (resources, relations)
}

fn denied(noisy: bool) -> BTreeSet<ResourceId> {
    let mut denied = BTreeSet::from([rid(201)]);
    if noisy {
        denied.extend((0..6).map(|n| rid(211 + n)));
    }
    denied
}

fn attached(id: ResourceId, relations: &[TypedRelationInstance]) -> Vec<TypedRelationInstance> {
    let mut attached: Vec<TypedRelationInstance> = relations
        .iter()
        .filter(|relation| relation.participants.iter().any(|p| p.resource_ref == id))
        .cloned()
        .collect();
    attached.sort_by_key(|relation| relation.relation_id);
    attached
}

fn temporal(id: ResourceId) -> TemporalProjection {
    TemporalProjection {
        resource_ref: id,
        valid_from: None,
        valid_to: None,
        profile: TemporalDiscoveryProfile::default(),
    }
}

fn records(
    resources: &[ResourceId],
    relations: &[TypedRelationInstance],
) -> Vec<GraphResourceRecord> {
    resources
        .iter()
        .map(|id| GraphResourceRecord {
            resource_ref: *id,
            kind: ResourceKind::Knowledge,
            resource_version_ref: None,
            temporal: temporal(*id),
            mapping: GraphSourceMapping::Registered {
                adapter_id: "fixture".into(),
                native_id: id.as_uuid().to_string(),
            },
            attached_relations: attached(*id, relations),
        })
        .collect()
}

/// Registers a MANUAL target and settles only its Graph READY.
async fn graph_ready(fixture: &Fixture, generation: u128, noisy: bool) -> ProjectionGenerationKey {
    let (resources, relations) = graph(noisy);
    let records = records(&resources, &relations);
    let handle = fixture
        .registrar
        .register_manual_with_graph(
            &FullBuildRequest {
                manifest: manifest(generation),
                expected_snapshot: SNAPSHOT.into(),
            },
            &canonical_mapping_digest(source_id(), SNAPSHOT, &records).unwrap(),
            FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap();
    let target = handle.graph_target().unwrap();
    let store = PostgresGraphStore::new(fixture.admin.clone());
    store
        .stage_full(&target, &records, &relations)
        .await
        .unwrap();
    let report = store.validate(&GraphBuildRef::Full(target)).await.unwrap();
    let mut connection = fixture.admin.acquire().await.unwrap();
    search_graph::store::settle_ready_on(&mut connection, &report)
        .await
        .unwrap();
    handle.key()
}

/// The same graph in the memory oracle under the same generation key.
fn oracle(key: ProjectionGenerationKey, noisy: bool, access: Arc<Access>) -> MemoryGraphRetriever {
    let (resources, relations) = graph(noisy);
    let mut manifest = base_manifest(0);
    manifest.generation_id = key.generation_id;
    manifest.graph_schema_version = Some("typed-nary-v1".into());
    manifest.resource_count = resources.len() as u64;
    manifest.relation_count = Some(relations.len() as u64);
    let projections = resources
        .iter()
        .map(|id| CompiledResourceProjection {
            manifest: manifest.clone(),
            retention_mode: RetentionMode::PersistentResource,
            directory: DirectoryProjection {
                resource_ref: *id,
                resource_version: None,
                kind: ResourceKind::Knowledge,
                canonical_name: "r".into(),
                title: None,
                aliases: vec![],
            },
            structured: StructuredProjection {
                resource_ref: *id,
                concept_refs: vec![],
                high_signal_facets: BTreeMap::new(),
                typed_facets: BTreeMap::new(),
                assertions: vec![],
                authority_resolutions: BTreeMap::new(),
            },
            temporal: temporal(*id),
            access: AccessProjection {
                resource_ref: *id,
                access_scope: None,
                source_access_model: None,
            },
            relations: attached(*id, &relations),
        })
        .collect();
    let memory = MemoryGraphRetriever::new(access);
    memory
        .build_generation(manifest, &source(), projections)
        .unwrap();
    memory
}

fn plan(
    seeds: &[u128],
    relation_type: &str,
    roles: (&str, &str),
    hops: usize,
) -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: seeds.iter().copied().map(rid).collect(),
        path_patterns: vec![
            RelationPathPattern::new(
                RelationNamespace::Discovery,
                relation_type,
                roles.0,
                roles.1
            );
            hops
        ],
        allowed_relation_types: vec![relation_type.into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(TemporalEvaluationContext {
            evaluation_id: DiscoveryEvaluationId::from_uuid(Uuid::from_u128(1)),
            evaluated_at: OffsetDateTime::UNIX_EPOCH,
            temporal_target: OffsetDateTime::UNIX_EPOCH,
            business_timezone: "UTC".into(),
        }),
        access_context: "fixture-actor".into(),
        expansion_budget: TraversalBudget {
            max_hops: 2,
            max_relations: 4,
            max_branching_per_node: 2,
            max_seed_nodes: 1,
            max_paths: 4,
        },
        stop_conditions: vec![],
    }
}

fn public(result: &GraphRetrievalResult) -> Vec<(Option<ResourceId>, String)> {
    result
        .hits
        .iter()
        .map(|hit| (hit.candidate.resource_ref, format!("{:?}", hit.paths)))
        .collect()
}

async fn actor(
    catalog: &SourceRegistrationCatalog,
    authority: &SyntheticAuthorityAdapter,
) -> (TrustedDiscoveryBinding, AuthorizedSourceScope) {
    let visibility = SyntheticVisibilityAdapter::new(catalog);
    visibility
        .grant(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            source_id(),
            registration::revision(1),
            registration::visibility(1),
        )
        .unwrap();
    let handle = authority
        .issue_verified_identity(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            Some(SessionId::from_uuid(Uuid::now_v7())),
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(600),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let binding = authority
        .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::from_u128(1)))
        .await
        .unwrap()
        .unwrap();
    let scope = visibility
        .bind_source(&actor, source_id())
        .await
        .unwrap()
        .unwrap();
    (binding, scope)
}

#[tokio::test]
async fn pg_nary_path_matches_memory_oracle_and_hidden_degree_matches_absent() {
    let fixture = fixture().await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let base = graph_ready(&fixture, 8_001, false).await;
    let noisy = graph_ready(&fixture, 8_002, true).await;
    let two_hops = plan(&[101], "cites", ("source", "target"), 2);

    let access = Access {
        denied: denied(true),
        unknown: BTreeSet::new(),
    };
    let verifier = Verifier::new(usize::MAX);
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &verifier);
    let lease = |key| GraphReadLease::from_identifiers(key, binding.evaluation(), Uuid::nil());
    let pg = reader
        .retrieve(&lease(base), &two_hops, &binding, &scope)
        .await
        .unwrap();
    let memory = oracle(
        base,
        false,
        Arc::new(Access {
            denied: denied(false),
            unknown: BTreeSet::new(),
        }),
    )
    .retrieve(base, &two_hops)
    .await
    .unwrap();
    assert_eq!(pg, memory);
    let targets: Vec<_> = pg
        .hits
        .iter()
        .map(|hit| hit.candidate.resource_ref)
        .collect();
    assert_eq!(targets, vec![Some(rid(104)), Some(rid(106))]);
    // The ternary relation 301 keeps its witness in the path evidence.
    assert_eq!(pg.hits[0].paths[0].steps[0].participants.len(), 3);

    // Six hidden branches of R1 consume no visible budget and leave no trace.
    let with_hidden = reader
        .retrieve(&lease(noisy), &two_hops, &binding, &scope)
        .await
        .unwrap();
    assert_eq!(public(&with_hidden), public(&pg));

    // A missing seed and a denied seed share the same empty response.
    let missing = reader
        .retrieve(
            &lease(base),
            &plan(&[999], "cites", ("source", "target"), 2),
            &binding,
            &scope,
        )
        .await
        .unwrap();
    let hidden_seed = reader
        .retrieve(
            &lease(base),
            &plan(&[201], "cites", ("source", "target"), 2),
            &binding,
            &scope,
        )
        .await
        .unwrap();
    assert!(missing.hits.is_empty());
    assert_eq!(public(&missing), public(&hidden_seed));
}

#[tokio::test]
async fn access_unknown_unsupported_stop_budget_and_lapsed_pin_return_no_hits() {
    let fixture = fixture().await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let key = graph_ready(&fixture, 8_011, false).await;
    let lease = GraphReadLease::from_identifiers(key, binding.evaluation(), Uuid::nil());
    let two_hops = plan(&[101], "cites", ("source", "target"), 2);
    let live = Verifier::new(usize::MAX);

    // Unknown access for a reachable participant fails the whole read.
    let unknown = Access {
        denied: denied(false),
        unknown: BTreeSet::from([rid(106)]),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &unknown, &live);
    assert!(matches!(
        reader.retrieve(&lease, &two_hops, &binding, &scope).await,
        Err(SearchError::SourceUnavailable(_))
    ));

    let access = Access {
        denied: denied(false),
        unknown: BTreeSet::new(),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &live);
    let mut stop = two_hops.clone();
    stop.stop_conditions = vec!["first-hit".into()];
    let mut narrow = two_hops.clone();
    narrow.expansion_budget.max_branching_per_node = 1;
    for refused in [stop, narrow] {
        assert!(matches!(
            reader.retrieve(&lease, &refused, &binding, &scope).await,
            Err(SearchError::InvalidRequest(_))
        ));
    }

    // The pin lapses between the snapshot and the before-return check.
    let lapsing = Verifier::new(1);
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &lapsing);
    assert!(
        reader
            .retrieve(&lease, &two_hops, &binding, &scope)
            .await
            .is_err()
    );
    assert_eq!(lapsing.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn real_pin_reads_current_graph_until_the_pin_expires() {
    let fixture = fixture().await;
    let key = fixture.publish_current(8_021).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            source_id(),
            registration::revision(1),
            registration::visibility(1),
        )
        .unwrap();
    let handle = authority
        .issue_verified_identity(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            Some(SessionId::from_uuid(Uuid::now_v7())),
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(600),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let binding = authority
        .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::from_u128(5)))
        .await
        .unwrap()
        .unwrap();
    let scope = visibility
        .bind_source(&actor, source_id())
        .await
        .unwrap()
        .unwrap();
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );
    let pin = pins
        .pin_current(
            &binding,
            &scope,
            PinTtl::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pin.lease.key(), key);

    let access = Access {
        denied: BTreeSet::new(),
        unknown: BTreeSet::new(),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &pins);
    let mut placement = plan(
        &[10],
        "document_current_placement",
        ("document", "folder"),
        1,
    );
    placement.expansion_budget.max_hops = 1;
    let result = reader
        .retrieve(&pin.lease, &placement, &binding, &scope)
        .await
        .unwrap();
    assert_eq!(result.generation, key);
    assert_eq!(
        result
            .hits
            .iter()
            .map(|hit| hit.candidate.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(rid(11))]
    );

    sqlx::query(
        "UPDATE search_evaluation_lease SET expires_at = clock_timestamp() - interval '1 second' \
         WHERE lease_id=$1",
    )
    .bind(pin.lease.lease_id())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert!(matches!(
        reader
            .retrieve(&pin.lease, &placement, &binding, &scope)
            .await,
        Err(SearchError::SourceUnavailable(_))
    ));
}

#[tokio::test]
async fn missing_mapping_commitment_fails_recovery() {
    let fixture = fixture().await;
    let key = graph_ready(&fixture, 8_031, false).await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let manifest_digest = manifest(8_031).digest;
    let receipt = store.recover(key, &manifest_digest).await.unwrap();
    assert_eq!(receipt.resource_count, 7);
    let stored = store.ready_resource(key, rid(101)).await.unwrap().unwrap();
    assert!(matches!(
        stored.mapping,
        GraphSourceMapping::Registered { .. }
    ));

    // Admin-only corruption below the triggers: one owner mapping changes
    // after READY, so the committed mapping digest no longer holds.
    let mut tx = fixture.admin.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE search_graph.resource SET native_id='borrowed' \
         WHERE source_id=$1 AND generation_id=$2 AND resource_id=$3",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(rid(101).as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        store.recover(key, &manifest_digest).await,
        Err(search_graph::GraphError::Integrity(_))
    ));
}
