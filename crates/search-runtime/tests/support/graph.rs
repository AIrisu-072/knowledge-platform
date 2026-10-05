//! Shared Graph fixtures for the P3 runtime tests (G06, G07).
#![allow(dead_code, unused_imports)]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use super::bundle::*;
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
use search_graph_memory::MemoryGraphRetriever;

pub struct Access {
    pub denied: BTreeSet<ResourceId>,
    pub unknown: BTreeSet<ResourceId>,
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
pub struct Verifier {
    pub calls: AtomicUsize,
    pub live_calls: usize,
}

impl Verifier {
    pub fn new(live_calls: usize) -> Self {
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

pub fn cites(id: u128, participants: &[(&str, u128)]) -> TypedRelationInstance {
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
pub fn graph(noisy: bool) -> (Vec<ResourceId>, Vec<TypedRelationInstance>) {
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

pub fn denied(noisy: bool) -> BTreeSet<ResourceId> {
    let mut denied = BTreeSet::from([rid(201)]);
    if noisy {
        denied.extend((0..6).map(|n| rid(211 + n)));
    }
    denied
}

pub fn attached(id: ResourceId, relations: &[TypedRelationInstance]) -> Vec<TypedRelationInstance> {
    let mut attached: Vec<TypedRelationInstance> = relations
        .iter()
        .filter(|relation| relation.participants.iter().any(|p| p.resource_ref == id))
        .cloned()
        .collect();
    attached.sort_by_key(|relation| relation.relation_id);
    attached
}

pub fn temporal(id: ResourceId) -> TemporalProjection {
    TemporalProjection {
        resource_ref: id,
        valid_from: None,
        valid_to: None,
        profile: TemporalDiscoveryProfile::default(),
    }
}

pub fn records(
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
pub async fn graph_ready(
    fixture: &Fixture,
    generation: u128,
    noisy: bool,
) -> ProjectionGenerationKey {
    let (resources, relations) = graph(noisy);
    graph_ready_with(fixture, generation, &resources, &relations).await
}

/// Registers a MANUAL target with these rows and settles only its Graph READY.
pub async fn graph_ready_with(
    fixture: &Fixture,
    generation: u128,
    resources: &[ResourceId],
    relations: &[TypedRelationInstance],
) -> ProjectionGenerationKey {
    let relations = relations.to_vec();
    let records = records(resources, &relations);
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
pub fn oracle(
    key: ProjectionGenerationKey,
    noisy: bool,
    access: Arc<Access>,
) -> MemoryGraphRetriever {
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

pub fn plan(
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

pub fn public(result: &GraphRetrievalResult) -> Vec<(Option<ResourceId>, String)> {
    result
        .hits
        .iter()
        .map(|hit| (hit.candidate.resource_ref, format!("{:?}", hit.paths)))
        .collect()
}

pub async fn actor(
    catalog: &SourceRegistrationCatalog,
    authority: &SyntheticAuthorityAdapter,
) -> (TrustedDiscoveryBinding, AuthorizedSourceScope) {
    let visibility = SyntheticVisibilityAdapter::new(catalog);
    visibility
        .grant(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            source_id(),
            super::registration::revision(1),
            super::registration::visibility(1),
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
