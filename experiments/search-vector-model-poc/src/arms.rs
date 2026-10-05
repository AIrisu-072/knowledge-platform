//! Exact dense retrieval (D) and its S1 PriorityConcat compositions with the
//! frozen baseline's actual Lexical and HyperGraph stages (P2-03).
//!
//! Every Unit of every Source Part is embedded as a passage. A query scans
//! all Units exactly, keeps the top `window` Unit hits by `(score desc,
//! UnitId)`, applies Source-owned current Read/Version, and folds Units to
//! their parent Version once. Raw scores never leave the dense stage; the
//! compositions concatenate stage lists in planner order.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use search_application::retrieval::{
    RetrievalInputs, RetrieverPlanner, RetrieverProfile, RetrieverSupport,
};
use search_application::routing::{RoutingConstraints, SourceRouter};
use search_core::discovery::DiscoveryNeed;
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{DiscoveryEvaluationId, NeedId, ResourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalEvaluationContext;
use search_vector_poc::run::{Harness, QueryRun, StageTrace};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::embed::{Embedder, TextKind};

pub struct DenseUnit {
    pub parent: usize,
    pub unit_id: String,
    pub vector: Vec<f32>,
}

pub struct DenseIndex {
    pub units: Vec<DenseUnit>,
    pub build: Duration,
}

pub fn build_index(
    harness: &Harness,
    embedder: &Embedder,
    batch: usize,
) -> Result<DenseIndex, String> {
    let units: Vec<(usize, &str, &str)> = harness
        .corpus
        .records
        .iter()
        .flat_map(|record| {
            std::iter::once(&record.unit)
                .chain(record.additional_units.iter())
                .map(move |unit| (record.index, unit.unit_id.as_str(), unit.text.as_str()))
        })
        .collect();
    let start = Instant::now();
    let mut out = Vec::with_capacity(units.len());
    for chunk in units.chunks(batch) {
        let texts: Vec<&str> = chunk.iter().map(|(_, _, text)| *text).collect();
        let embedded = embedder.embed(&texts, TextKind::Passage)?;
        for ((parent, unit_id, _), vector) in chunk.iter().zip(embedded.normalized) {
            out.push(DenseUnit {
                parent: *parent,
                unit_id: (*unit_id).to_owned(),
                vector,
            });
        }
    }
    Ok(DenseIndex {
        units: out,
        build: start.elapsed(),
    })
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

pub fn dense_stage(
    index: &DenseIndex,
    query: &[f32],
    visible: &BTreeSet<usize>,
    window: usize,
) -> Vec<usize> {
    let mut scored: Vec<(f32, &str, usize)> = index
        .units
        .iter()
        .map(|unit| (dot(&unit.vector, query), unit.unit_id.as_str(), unit.parent))
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0).then_with(|| left.1.cmp(right.1)));
    let mut parents = Vec::new();
    let mut seen = BTreeSet::new();
    for (_, _, parent) in scored.into_iter().take(window) {
        if visible.contains(&parent) && seen.insert(parent) {
            parents.push(parent);
        }
    }
    parents
}

/// S1 PriorityConcat: stages in planner order, each eligible parent once.
pub fn fuse(stages: &[&[usize]], visible: &BTreeSet<usize>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for stage in stages {
        for parent in *stage {
            if visible.contains(parent) && seen.insert(*parent) {
                out.push(*parent);
            }
        }
    }
    out
}

/// The actual planner's S1 order for Lexical + Vector + HyperGraph.
pub fn planned_ldg_order(harness: &Harness) -> Vec<String> {
    let need = DiscoveryNeed {
        need_id: NeedId::from_uuid(Uuid::from_u128(1)),
        intent_signature: IntentSignature::new(IntentFact::new(
            "planner order".into(),
            IntentFactOrigin::Explicit,
        )),
        required_resource_types: vec![ResourceKind::Knowledge],
        required_claims: vec![],
        authority_requirements: vec![],
        freshness_requirements: vec![],
        constraints: vec![],
        completion_requirement: EvidenceRequirement::new(vec![]),
    };
    let source = harness.source.clone();
    let routes = SourceRouter::plan(
        &need,
        std::slice::from_ref(&source),
        &RoutingConstraints {
            required_source_ids: vec![source.source_id],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
    );
    let seed = ResourceId::from_uuid(harness.corpus.records[0].unit.resource_id);
    let context = ResourceId::from_uuid(harness.corpus.records[1].unit.resource_id);
    // The same plan shape as the baseline's LG arm; only its validity matters.
    let plan = GraphTraversalPlan {
        seed_nodes: vec![seed],
        path_patterns: vec![
            RelationPathPattern::new(
                RelationNamespace::Discovery,
                "supports-context",
                "seed",
                "result",
            )
            .with_participant("context", context),
        ],
        allowed_relation_types: vec!["supports-context".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(2)),
            OffsetDateTime::UNIX_EPOCH,
            OffsetDateTime::UNIX_EPOCH,
            "UTC",
        )),
        access_context: search_vector_poc::run::ACTOR.into(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 1,
            max_branching_per_node: 1,
            max_seed_nodes: 1,
            max_paths: 1,
        },
        stop_conditions: vec![],
    };
    RetrieverPlanner::plan(
        RetrieverProfile::Exploratory,
        &routes,
        &RetrieverSupport {
            lexical: true,
            vector: true,
            hypergraph: true,
            ..RetrieverSupport::default()
        },
        &RetrievalInputs {
            lexical_query: Some("planner".into()),
            vector_query_available: true,
            graph_plans: BTreeMap::from([(source.source_id, plan)]),
            max_initial_retrievers_per_source: 3,
            ..RetrievalInputs::default()
        },
    )
    .s1_retriever_order
}

pub fn gold(harness: &Harness, query_id: &str) -> BTreeSet<usize> {
    harness
        .corpus
        .qrels
        .iter()
        .filter(|qrel| qrel.query_id == query_id && qrel.grade > 0)
        .map(|qrel| qrel.resource_index)
        .collect()
}

pub fn run(
    harness: &Harness,
    query_id: &str,
    stages: Vec<(String, Vec<usize>)>,
    visible: &BTreeSet<usize>,
    query_time: Duration,
) -> QueryRun {
    let fusion = Instant::now();
    let lists: Vec<&[usize]> = stages.iter().map(|(_, ranks)| ranks.as_slice()).collect();
    let parent_ranks = fuse(&lists, visible);
    let fusion_time = fusion.elapsed();
    let gold = gold(harness, query_id);
    QueryRun {
        query_id: query_id.to_owned(),
        parent_ranks,
        stages: stages
            .into_iter()
            .map(|(retriever_id, raw_parent_ranks)| StageTrace {
                eligible_gold_misses: gold
                    .difference(&raw_parent_ranks.iter().copied().collect())
                    .copied()
                    .collect(),
                retriever_id,
                raw_parent_ranks,
            })
            .collect(),
        query_time,
        fusion_time,
    }
}
