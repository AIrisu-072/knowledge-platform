//! P2-06: the trusted Vector query, Source resolution and S1 contract.

#[path = "support/api.rs"]
mod api;
#[path = "support/vector.rs"]
mod vector;

use api::ApiWorld;
use search_application::candidate::{CandidateHardGates, HardGateEvaluation, RetrieverRankList};
use search_application::federation::{CandidateFederator, FusionStrategy};
use search_application::ports::AccessDecision;
use search_application::vector::{
    PinnedVectorGeneration, TrustedVectorQuery, VectorBuildOutcome, VectorLifecycle,
    VectorResolution, VectorRetriever, resolve_hit,
};
use search_core::applicability::{ApplicabilityEvaluation, ApplicabilityState};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate, GapReason};
use search_core::id::{ResourceId, SourceId};
use search_core::knowledge_unit::{ExtractionProfileId, KnowledgeUnit};
use search_core::vector::{RankedVectorHit, VectorActivationPolicy, VectorStorageKind};
use time::OffsetDateTime;
use uuid::Uuid;
use vector::*;

const SCOPE: &str = "tenant-a-scope";

/// One change to the Source's current Unit.
type Mutation = Box<dyn Fn(&mut KnowledgeUnit)>;

fn rid(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

struct World {
    api: ApiWorld,
    provider: Provider,
    index: Index,
    generations: Generations,
    resolver: Resolver,
}

impl World {
    async fn new() -> Self {
        Self {
            api: ApiWorld::new().await,
            provider: Provider::new(spec("commit-abc123")),
            index: Index::default(),
            generations: Generations::default(),
            resolver: Resolver::default(),
        }
    }

    fn source(&self) -> SourceId {
        self.api.document
    }

    fn lifecycle(&self) -> VectorLifecycle<'_> {
        VectorLifecycle {
            provider: &self.provider,
            index: &self.index,
            generations: &self.generations,
        }
    }

    fn retriever(&self) -> VectorRetriever<'_> {
        VectorRetriever {
            provider: &self.provider,
            index: &self.index,
            generations: &self.generations,
            resolver: &self.resolver,
        }
    }

    /// Publishes `units` as generation 1 and makes them the Source's view.
    async fn publish(&self, units: &[KnowledgeUnit]) {
        let key = generation(self.source(), 1);
        self.generations.set_p1(key);
        let input = input(units, key, SCOPE);
        self.resolver.current(&input);
        assert!(matches!(
            self.lifecycle()
                .build(
                    &input,
                    &[],
                    VectorStorageKind::Persistent,
                    OffsetDateTime::now_utc()
                )
                .await
                .unwrap(),
            VectorBuildOutcome::Published(_)
        ));
    }

    async fn query(&self, text: &str) -> TrustedVectorQuery {
        TrustedVectorQuery::compile(
            &scope(&self.api).await,
            VectorActivationPolicy::EligibleOptIn,
            &self.provider.spec,
            text,
            10,
        )
        .unwrap()
    }
}

fn parents(candidates: &[FederatedCandidate]) -> Vec<ResourceId> {
    candidates
        .iter()
        .map(|candidate| candidate.resource_ref.unwrap())
        .collect()
}

#[tokio::test]
async fn forged_query_or_model_rejected() {
    let world = World::new().await;
    let source = world.source();
    world.publish(&[unit(source, 1, 0, "alpha", 9)]).await;
    let scope = scope(&world.api).await;
    let model = &world.provider.spec;
    // Disabled by default; bounded text and window.
    for (policy, text, window) in [
        (VectorActivationPolicy::default(), "alpha", 10),
        (VectorActivationPolicy::EligibleOptIn, " ", 10),
        (
            VectorActivationPolicy::EligibleOptIn,
            &"x".repeat(2_049),
            10,
        ),
        (VectorActivationPolicy::EligibleOptIn, "alpha", 0),
        (VectorActivationPolicy::EligibleOptIn, "alpha", 101),
    ] {
        assert!(TrustedVectorQuery::compile(&scope, policy, model, text, window).is_err());
    }
    let query = world.query("alpha").await;
    let now = OffsetDateTime::now_utc();
    // The query is bound to its Source: another Source's P1 key is refused.
    let foreign = generation(world.api.second, 1);
    assert!(
        world
            .retriever()
            .retrieve(foreign, &query, now)
            .await
            .is_err()
    );
    // A provider answering with another model's query vector is refused.
    let mut forging = Provider::new(spec("commit-abc123"));
    forging.forge_query_model = Some(spec("commit-other"));
    let retriever = VectorRetriever {
        provider: &forging,
        ..world.retriever()
    };
    assert!(
        retriever
            .retrieve(generation(source, 1), &query, now)
            .await
            .is_err()
    );
    // A query compiled for one model cannot run on another model's provider.
    let other = Provider::new(spec("commit-other"));
    let retriever = VectorRetriever {
        provider: &other,
        ..world.retriever()
    };
    assert!(
        retriever
            .retrieve(generation(source, 1), &query, now)
            .await
            .is_err()
    );
    // The genuine path works.
    let batch = world
        .retriever()
        .retrieve(generation(source, 1), &query, now)
        .await
        .unwrap();
    assert_eq!(parents(batch.candidates()), vec![rid(1)]);
}

#[tokio::test]
async fn read_unknown_denied_id_free() {
    let world = World::new().await;
    let source = world.source();
    world
        .publish(&[
            unit(source, 1, 0, "alpha one", 9),
            unit(source, 2, 0, "alpha two", 9),
            unit(source, 3, 0, "alpha three", 9),
        ])
        .await;
    world.resolver.read(2, AccessDecision::Denied);
    let query = world.query("alpha").await;
    let now = OffsetDateTime::now_utc();
    let denied = world
        .retriever()
        .retrieve(generation(source, 1), &query, now)
        .await
        .unwrap();
    // Denied leaves no candidate, no trace entry and no gap.
    assert_eq!(parents(denied.candidates()), vec![rid(1), rid(3)]);
    assert_eq!(denied.trace().len(), 2);
    assert!(!denied.is_unavailable() && denied.gap().is_none());
    // Unknown is never absence: the batch is unavailable with an ID-free gap.
    world.resolver.read(3, AccessDecision::Unknown);
    let unknown = world
        .retriever()
        .retrieve(generation(source, 1), &query, now)
        .await
        .unwrap();
    assert_eq!(parents(unknown.candidates()), vec![rid(1)]);
    assert!(unknown.is_unavailable());
    let gap = unknown.gap().unwrap();
    assert!(gap.blocking);
    assert_eq!(gap.reason, GapReason::Availability);
    assert_eq!(gap.required_fact, "vector_unavailable");
}

#[tokio::test]
async fn stale_version_t10_part_representation_raw_profile_text_model_or_generation_rejected() {
    let world = World::new().await;
    let source = world.source();
    let original = unit(source, 1, 0, "alpha", 9);
    world.publish(std::slice::from_ref(&original)).await;
    let scope = scope(&world.api).await;
    let key = generation(source, 1);
    let pin = world
        .generations
        .pin_current_for_test(key, &world.provider.spec)
        .await;
    let hit = RankedVectorHit {
        hit: world.index.first_hit(&pin).await,
        model_id: pin.manifest().model_id.clone(),
        rank: 1,
        raw_similarity: 0.9,
    };
    let model = &world.provider.spec;
    let resolve = |hit: RankedVectorHit| {
        let world = &world;
        let scope = scope.clone();
        let pin = pin.clone();
        async move { resolve_hit(&world.resolver, model, &pin, &scope, &hit).await }
    };
    assert!(matches!(
        resolve(hit.clone()).await,
        VectorResolution::Visible(_)
    ));
    // The Source's current Unit differs in any bound field: suppressed.
    let mutations: Vec<Mutation> = vec![
        Box::new(|unit| unit.version.source_native_version = "version-2".into()),
        Box::new(|unit| unit.part.source_native_part_id = "part-other".into()),
        Box::new(|unit| {
            unit.provenance.authoritative_representation_ref = "representation-2".into()
        }),
        Box::new(|unit| unit.provenance.raw.sha256 = [8; 32]),
        Box::new(|unit| unit.provenance.profile = ExtractionProfileId::parse(&digest(19)).unwrap()),
        Box::new(|unit| {
            unit.text = "alpha changed".into();
            unit.text_sha256 = search_core::knowledge_unit::text_sha256("alpha changed");
        }),
    ];
    for mutate in mutations {
        let (mut current, authority) =
            world.resolver.units.lock().unwrap()[&original.unit_id].clone();
        mutate(&mut current);
        world
            .resolver
            .units
            .lock()
            .unwrap()
            .insert(original.unit_id, (current, authority));
        assert_eq!(resolve(hit.clone()).await, VectorResolution::Suppressed);
        let input = input(std::slice::from_ref(&original), key, SCOPE);
        world.resolver.current(&input);
    }
    // Another Version became current or publication ended (T10).
    world
        .resolver
        .not_current
        .lock()
        .unwrap()
        .push(original.unit_id);
    assert_eq!(resolve(hit.clone()).await, VectorResolution::Suppressed);
    world.resolver.not_current.lock().unwrap().clear();
    // A hit of another model or generation is an index fault: fail closed.
    let mut other_model = hit.clone();
    other_model.model_id = spec("commit-other").validate_and_id().unwrap();
    assert_eq!(resolve(other_model).await, VectorResolution::Unavailable);
    let mut other_generation = hit.clone();
    other_generation.hit.generation = generation(source, 2);
    assert_eq!(
        resolve(other_generation).await,
        VectorResolution::Unavailable
    );
    // And through the façade the batch is unavailable, not empty-and-final.
    *world.index.foreign_generation.lock().unwrap() = Some(generation(source, 2));
    let batch = world
        .retriever()
        .retrieve(key, &world.query("alpha").await, OffsetDateTime::now_utc())
        .await
        .unwrap();
    assert!(batch.candidates().is_empty() && batch.is_unavailable());
}

#[tokio::test]
async fn multi_unit_parent_once() {
    let world = World::new().await;
    let source = world.source();
    world
        .publish(&[
            unit(source, 1, 0, "alpha first part", 9),
            unit(source, 1, 1, "alpha second part", 9),
            unit(source, 2, 0, "beta", 9),
        ])
        .await;
    let batch = world
        .retriever()
        .retrieve(
            generation(source, 1),
            &world.query("alpha").await,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    // Two Unit hits of one Version: one parent candidate, both in the trace.
    assert_eq!(parents(batch.candidates()), vec![rid(1), rid(2)]);
    assert_eq!(batch.trace().len(), 3);
}

#[tokio::test]
async fn body_required_and_absence_not_minted() {
    let world = World::new().await;
    let source = world.source();
    world.publish(&[unit(source, 1, 0, "alpha", 9)]).await;
    let batch = world
        .retriever()
        .retrieve(
            generation(source, 1),
            &world.query("gamma").await,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    // A Vector candidate carries no locator, body signal or score: it can
    // never stand in for a verified body Unit hit or exact text.
    for candidate in batch.candidates() {
        assert!(candidate.locator.is_none());
        assert!(candidate.matched_signals.is_empty());
        assert_eq!(candidate.retrieval_method, "vector");
    }
    let list = batch.rank_list("source:Vector", |_| gates()).unwrap();
    assert!(
        list.hits
            .iter()
            .all(|hit| hit.raw_score.is_none() && hit.evidence_refs.is_empty())
    );
    // No hit is not absence either: there is nothing absence-shaped to read.
    let empty = Index::default();
    let retriever = VectorRetriever {
        index: &empty,
        ..world.retriever()
    };
    let none = retriever
        .retrieve(
            generation(source, 1),
            &world.query("alpha").await,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    assert!(none.candidates().is_empty() && none.gap().is_none());
}

fn gates() -> CandidateHardGates {
    let open = HardGateEvaluation {
        state: ApplicabilityState::Applicable,
        reasons: vec![],
        gaps: vec![],
    };
    CandidateHardGates {
        applicability: ApplicabilityEvaluation {
            state: ApplicabilityState::Applicable,
            matched_conditions: vec![],
            resolved_discriminators: vec![],
            remaining_nonblocking_unknowns: vec![],
            gaps: vec![],
            reasons: vec![],
        },
        structured: open.clone(),
        access: open.clone(),
        temporal: open,
    }
}

fn list(source: SourceId, retriever: &str, resources: &[u128]) -> RetrieverRankList {
    RetrieverRankList {
        retriever_id: format!("{}:{retriever}", source.as_uuid()),
        generation: generation(source, 1),
        hits: resources
            .iter()
            .map(|resource| {
                let mut candidate = FederatedCandidate::new(
                    format!("{}:{resource}", source.as_uuid()),
                    CandidateIdentityClass::DurableResource,
                    source,
                    retriever,
                );
                candidate.resource_ref = Some(rid(*resource));
                search_application::candidate::RankedCandidateHit {
                    candidate,
                    hard_gates: gates(),
                    identity_evidence: vec![],
                    raw_score: None,
                    evidence_refs: vec![],
                }
            })
            .collect(),
    }
}

#[tokio::test]
async fn lexical_graph_order_unchanged() {
    let world = World::new().await;
    let source = world.source();
    world
        .publish(&[
            unit(source, 3, 0, "alpha", 9),
            unit(source, 1, 0, "alpha too", 9),
        ])
        .await;
    let batch = world
        .retriever()
        .retrieve(
            generation(source, 1),
            &world.query("alpha").await,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    let vector = batch
        .rank_list(&format!("{}:Vector", source.as_uuid()), |_| gates())
        .unwrap();
    let order = |lists: Vec<RetrieverRankList>| {
        CandidateFederator::merge(&lists, FusionStrategy::PriorityConcat)
            .unwrap()
            .ranked
            .iter()
            .map(|group| group.hits[0].candidate.resource_ref.unwrap())
            .collect::<Vec<_>>()
    };
    let lexical = list(source, "Lexical", &[1, 2]);
    let graph = list(source, "HyperGraph", &[4]);
    // S1 Lexical → Vector → HyperGraph: Lexical keeps its order first, the
    // Graph result keeps its place after, Vector only appends new parents.
    let without = order(vec![lexical.clone(), graph.clone()]);
    let with = order(vec![lexical, vector, graph]);
    assert_eq!(without, vec![rid(1), rid(2), rid(4)]);
    assert_eq!(with, vec![rid(1), rid(2), rid(3), rid(4)]);
}

#[tokio::test]
async fn unavailable_does_not_assert_absence() {
    let world = World::new().await;
    let source = world.source();
    let query = world.query("alpha").await;
    let now = OffsetDateTime::now_utc();
    // No READY Vector generation for the pinned P1 key.
    let missing = world
        .retriever()
        .retrieve(generation(source, 1), &query, now)
        .await
        .unwrap();
    assert!(missing.is_unavailable() && missing.candidates().is_empty());
    assert!(missing.gap().unwrap().blocking);
    assert!(missing.rank_list("source:Vector", |_| gates()).is_none());
    // A failing Source resolver.
    world.publish(&[unit(source, 1, 0, "alpha", 9)]).await;
    *world.resolver.failing.lock().unwrap() = true;
    let failing = world
        .retriever()
        .retrieve(generation(source, 1), &query, now)
        .await
        .unwrap();
    assert!(failing.is_unavailable() && failing.candidates().is_empty());
    assert_eq!(failing.gap().unwrap().reason, GapReason::Availability);
}

trait TestPins {
    async fn pin_current_for_test(
        &self,
        key: search_core::projection::ProjectionGenerationKey,
        spec: &search_core::vector::EmbeddingModelSpec,
    ) -> PinnedVectorGeneration;
}

impl TestPins for Generations {
    async fn pin_current_for_test(
        &self,
        key: search_core::projection::ProjectionGenerationKey,
        spec: &search_core::vector::EmbeddingModelSpec,
    ) -> PinnedVectorGeneration {
        search_application::vector::VectorGenerationPort::pin_current(
            self,
            key,
            &spec.validate_and_id().unwrap(),
        )
        .await
        .unwrap()
        .unwrap()
    }
}

trait TestHits {
    async fn first_hit(
        &self,
        pin: &PinnedVectorGeneration,
    ) -> search_core::knowledge_unit::VectorHitRef;
}

impl TestHits for Index {
    async fn first_hit(
        &self,
        pin: &PinnedVectorGeneration,
    ) -> search_core::knowledge_unit::VectorHitRef {
        search_application::vector::VectorIndexPort::staged_entries(self, &pin.manifest().index)
            .await
            .unwrap()
            .remove(0)
            .hit
    }
}
