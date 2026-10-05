//! Actual Search lexical/graph adapters, application retrieval and S1 federation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use search_application::candidate::{
    CandidateHardGates, HardGateEvaluation, RankedCandidateHit, RetrieverRankList,
};
use search_application::federation::{CandidateFederator, FusionStrategy};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    LexicalQuery,
};
use search_application::retrieval::{
    RetrievalInputs, RetrievalPlan, RetrieverKind, RetrieverPlanner, RetrieverProfile,
    RetrieverSupport,
};
use search_application::retrieval_execution::{
    RetrievalExecutionInput, RetrievalExecutionPorts, RetrievalExecutor,
};
use search_application::routing::{RoutingConstraints, SourceRouter};
use search_core::applicability::{ApplicabilityEvaluation, ApplicabilityState};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest, FederatedCandidate};
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::observation::Coverage;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
use search_graph_memory::MemoryGraphRetriever;
use search_tantivy::{LexicalBuildInput, LexicalDocument, SourceSuppliedBody, TantivyLexicalIndex};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::corpus::{Corpus, CorpusRecord, Query, ReadState};
use crate::validate::validate_source_owned_unit;

pub const ACTOR: &str = "synthetic-principal";
const SNAPSHOT: &str = "synthetic-snapshot-v1";
const AUTHORITY: &str = "synthetic-authority";

fn sid(id: Uuid) -> SourceId {
    SourceId::from_uuid(id)
}
fn rid(id: Uuid) -> ResourceId {
    ResourceId::from_uuid(id)
}
fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    Lexical,
    LexicalGraph,
}

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Self::Lexical => "L",
            Self::LexicalGraph => "LG",
        }
    }
}

pub struct CurrentSource {
    source_id: SourceId,
    states: RwLock<BTreeMap<ResourceId, (AccessDecision, bool)>>,
}

impl CurrentSource {
    fn from_corpus(corpus: &Corpus) -> Self {
        let states = corpus
            .records
            .iter()
            .map(|record| {
                let decision = match record.current_read {
                    ReadState::Allowed => AccessDecision::Allowed,
                    ReadState::Denied => AccessDecision::Denied,
                    ReadState::Unknown => AccessDecision::Unknown,
                };
                (
                    rid(record.unit.resource_id),
                    (decision, record.current_version),
                )
            })
            .collect();
        Self {
            source_id: sid(corpus.source_id),
            states: RwLock::new(states),
        }
    }

    pub fn set_read(&self, resource: ResourceId, decision: AccessDecision) {
        if let Some(state) = self.states.write().unwrap().get_mut(&resource) {
            state.0 = decision;
        }
    }

    pub fn set_current_version(&self, resource: ResourceId, current: bool) {
        if let Some(state) = self.states.write().unwrap().get_mut(&resource) {
            state.1 = current;
        }
    }

    fn decision(&self, resource: ResourceId, actor: &str) -> AccessDecision {
        if actor != ACTOR {
            return AccessDecision::Denied;
        }
        self.states.read().unwrap().get(&resource).map_or(
            AccessDecision::Unknown,
            |(read, current)| {
                if *current {
                    *read
                } else {
                    AccessDecision::Denied
                }
            },
        )
    }
}

impl CurrentAccessEvaluatorPort for CurrentSource {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(self.decision(resource_ref, access_context)) })
    }
}

impl CurrentCandidateAccessEvaluatorPort for CurrentSource {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            Ok(if candidate.source_ref == self.source_id {
                candidate
                    .resource_ref
                    .map_or(AccessDecision::Unknown, |id| {
                        self.decision(id, access_context)
                    })
            } else {
                AccessDecision::Denied
            })
        })
    }
}

pub struct BuildTiming {
    pub lexical: Duration,
    pub graph: Duration,
}

pub struct Harness {
    pub corpus: Corpus,
    pub source: DiscoverableSource,
    pub manifest: ProjectionGenerationManifest,
    pub access: Arc<CurrentSource>,
    pub lexical: TantivyLexicalIndex,
    pub graph: MemoryGraphRetriever,
    pub build: BuildTiming,
}

fn source(corpus: &Corpus) -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        sid(corpus.source_id),
        "synthetic",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentResource,
    );
    source.resource_types = vec![ResourceKind::Knowledge];
    source.discovery_modes = vec![
        DiscoveryMode::LocalDirectory,
        DiscoveryMode::LocalContentSearch,
    ];
    source.provenance = Some("synthetic-trusted-projection-v1".into());
    source
}

fn manifest(corpus: &Corpus, generation_id: Uuid) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: sid(corpus.source_id),
        generation_id: ProjectionGenerationId::from_uuid(generation_id),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: Some("typed-nary-v1".into()),
        source_snapshot: SNAPSHOT.into(),
        resource_count: corpus.records.len() as u64,
        relation_count: Some(12),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("synthetic-fixture-{}-{}", corpus.seed, corpus.records.len()),
        built_at: at(100),
    }
}

fn relations(corpus: &Corpus, index: usize) -> Vec<TypedRelationInstance> {
    if index >= 30 || index % 10 != 4 {
        return vec![];
    }
    let base = index - 4;
    (0..4)
        .map(|branch| {
            let target_index = match branch {
                0 | 1 => base + 2 + branch,
                2 => base + 7,
                3 => base,
                _ => unreachable!(),
            };
            let mut relation = TypedRelationInstance::new(
                RelationId::from_uuid(Uuid::from_u128(50 + (base / 10 * 4 + branch) as u128)),
                RelationNamespace::Discovery,
                "supports-context",
                vec![
                    RelationParticipant::new("seed", rid(corpus.records[index].unit.resource_id)),
                    RelationParticipant::new(
                        "result",
                        rid(corpus.records[target_index].unit.resource_id),
                    ),
                    RelationParticipant::new(
                        "context",
                        rid(corpus.records[base + 5].unit.resource_id),
                    ),
                ],
            );
            relation.authority = Some(
                if branch == 2 {
                    "other-authority"
                } else {
                    AUTHORITY
                }
                .into(),
            );
            relation.provenance = Some("synthetic-source-relation".into());
            relation.evidence_refs = vec![format!("synthetic-relation-{base}-{branch}")];
            relation
        })
        .collect()
}

fn projections(
    corpus: &Corpus,
    manifest: &ProjectionGenerationManifest,
    source: &DiscoverableSource,
) -> Vec<CompiledResourceProjection> {
    corpus
        .records
        .iter()
        .map(|record| {
            let id = rid(record.unit.resource_id);
            CompiledResourceProjection {
                manifest: manifest.clone(),
                retention_mode: source.retention_mode,
                directory: DirectoryProjection {
                    resource_ref: id,
                    resource_version: None,
                    kind: ResourceKind::Knowledge,
                    canonical_name: record.canonical_name.clone(),
                    title: None,
                    aliases: record.aliases.clone(),
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
                    valid_to: (!record.temporal_valid).then(|| at(50)),
                    profile: TemporalDiscoveryProfile::default(),
                },
                access: AccessProjection {
                    resource_ref: id,
                    access_scope: None,
                    source_access_model: Some("synthetic-current-read".into()),
                },
                relations: relations(corpus, record.index),
            }
        })
        .collect()
}

fn lexical_input(corpus: &Corpus) -> LexicalBuildInput {
    let documents = corpus
        .records
        .iter()
        .map(|record| LexicalDocument {
            resource_ref: rid(record.unit.resource_id),
            kind: ResourceKind::Knowledge,
            canonical_name: record.canonical_name.clone(),
            title: None,
            aliases: record.aliases.clone(),
            high_signal_text: None,
            body: Some(SourceSuppliedBody::new(
                sid(corpus.source_id),
                source_bound_body(record),
            )),
            locator: Some(format!("synthetic://parent/{}", record.unit.resource_id)),
        })
        .collect();
    LexicalBuildInput::new(sid(corpus.source_id), SNAPSHOT, "schema-1", 1, documents)
}

fn source_bound_body(record: &CorpusRecord) -> String {
    std::iter::once(&record.unit)
        .chain(record.additional_units.iter())
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Harness {
    pub fn build(corpus: Corpus) -> Result<Self, String> {
        corpus.validate()?;
        let source = source(&corpus);
        let manifest = manifest(&corpus, corpus.generation_id);
        let access = Arc::new(CurrentSource::from_corpus(&corpus));
        let lexical = TantivyLexicalIndex::new();
        let graph = MemoryGraphRetriever::new(access.clone());
        let start = Instant::now();
        lexical
            .build_generation(manifest.clone(), &source, lexical_input(&corpus))
            .map_err(|e| e.to_string())?;
        let lexical_time = start.elapsed();
        let start = Instant::now();
        graph
            .build_generation(
                manifest.clone(),
                &source,
                projections(&corpus, &manifest, &source),
            )
            .map_err(|e| e.to_string())?;
        let graph_time = start.elapsed();
        Ok(Self {
            corpus,
            source,
            manifest,
            access,
            lexical,
            graph,
            build: BuildTiming {
                lexical: lexical_time,
                graph: graph_time,
            },
        })
    }

    pub fn measure_update(&self) -> Result<BuildTiming, String> {
        let mut next = manifest(&self.corpus, Uuid::from_u128(3));
        next.digest.push_str("-update-one");
        // Rebuild an immutable next generation with one Source-supplied field change.
        let docs = self
            .corpus
            .records
            .iter()
            .map(|record| LexicalDocument {
                resource_ref: rid(record.unit.resource_id),
                kind: ResourceKind::Knowledge,
                canonical_name: if record.index == 0 {
                    format!("{} 改訂", record.canonical_name)
                } else {
                    record.canonical_name.clone()
                },
                title: None,
                aliases: record.aliases.clone(),
                high_signal_text: None,
                body: Some(SourceSuppliedBody::new(
                    sid(self.corpus.source_id),
                    source_bound_body(record),
                )),
                locator: Some(format!("synthetic://parent/{}", record.unit.resource_id)),
            })
            .collect::<Vec<_>>();
        let input =
            LexicalBuildInput::new(sid(self.corpus.source_id), SNAPSHOT, "schema-1", 1, docs);
        let start = Instant::now();
        self.lexical
            .build_generation(next.clone(), &self.source, input)
            .map_err(|e| e.to_string())?;
        let lexical = start.elapsed();
        let start = Instant::now();
        self.graph
            .build_generation(
                next.clone(),
                &self.source,
                projections(&self.corpus, &next, &self.source),
            )
            .map_err(|e| e.to_string())?;
        let graph = start.elapsed();
        Ok(BuildTiming { lexical, graph })
    }
}

impl Harness {
    pub fn source_visible_parents(&self, actor: &str) -> BTreeSet<usize> {
        self.corpus
            .records
            .iter()
            .filter(|record| {
                record.eligible
                    && record.temporal_valid
                    && self.access.decision(rid(record.unit.resource_id), actor)
                        == AccessDecision::Allowed
            })
            .map(|record| record.index)
            .collect()
    }

    fn resolve_source_parent(
        &self,
        index: usize,
        actor: &str,
        pin: ProjectionGenerationKey,
    ) -> Result<bool, String> {
        if pin != self.manifest.key() || self.corpus.generation_id != pin.generation_id.as_uuid() {
            return Err("Source generation pin changed".into());
        }
        let record = self
            .corpus
            .records
            .get(index)
            .ok_or("Source parent missing")?;
        if record.source_parts.len() != 1 + record.additional_units.len() {
            return Err("Source Part enumeration changed".into());
        }
        if self.access.decision(rid(record.unit.resource_id), actor) != AccessDecision::Allowed {
            return Ok(false);
        }
        let version = Uuid::from_u128(1_000_000 + index as u128).to_string();
        for (part, unit) in record
            .source_parts
            .iter()
            .zip(std::iter::once(&record.unit).chain(record.additional_units.iter()))
        {
            validate_source_owned_unit(
                self.corpus.source_id,
                record.unit.resource_id,
                self.corpus.generation_id,
                &version,
                part,
                unit,
            )?;
        }
        Ok(true)
    }
}

pub fn pinned(harness: &Harness) -> ProjectionGenerationKey {
    harness.manifest.key()
}

fn request(query: &Query, actor: &str) -> DiscoveryRequest {
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(100)),
            intent_signature: IntentSignature::new(IntentFact::new(
                format!("find {}", query.query_id),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: vec![],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(101)),
            at(100),
            at(100),
            "Asia/Tokyo",
        ),
        access_context: actor.into(),
    }
}

fn graph_plan(
    corpus: &Corpus,
    query: &Query,
    request: &DiscoveryRequest,
) -> Option<GraphTraversalPlan> {
    let seed_index = query.seed_index?;
    let context_index = seed_index + 1;
    Some(GraphTraversalPlan {
        seed_nodes: vec![rid(corpus.records[seed_index].unit.resource_id)],
        path_patterns: vec![
            RelationPathPattern::new(
                RelationNamespace::Discovery,
                "supports-context",
                "seed",
                "result",
            )
            .with_participant(
                "context",
                rid(corpus.records[context_index].unit.resource_id),
            ),
        ],
        allowed_relation_types: vec!["supports-context".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some(AUTHORITY.into()),
        temporal_context: Some(request.temporal_context.clone()),
        access_context: request.access_context.clone(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 3,
            max_branching_per_node: 3,
            max_seed_nodes: 1,
            max_paths: 3,
        },
        stop_conditions: vec![],
    })
}

/// This is the single route/planner call used by both actual L/LG execution
/// and the pre-run input export. Vector arms have no baseline execution port.
fn planned_baseline_query(
    corpus: &Corpus,
    source: &DiscoverableSource,
    query: &Query,
    actor: &str,
    arm: Arm,
) -> (DiscoveryRequest, Option<GraphTraversalPlan>, RetrievalPlan) {
    let request = request(query, actor);
    let graph_plan = graph_plan(corpus, query, &request);
    let routes = SourceRouter::plan(
        &request.need,
        std::slice::from_ref(source),
        &RoutingConstraints {
            required_source_ids: vec![source.source_id],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
    );
    let inputs = RetrievalInputs {
        lexical_query: Some(query.text.clone()),
        graph_plans: graph_plan
            .clone()
            .map(|plan| BTreeMap::from([(source.source_id, plan)]))
            .unwrap_or_default(),
        vector_query_available: false,
        max_initial_retrievers_per_source: if arm == Arm::LexicalGraph { 2 } else { 1 },
        ..RetrievalInputs::default()
    };
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Exploratory,
        &routes,
        &RetrieverSupport {
            lexical: true,
            hypergraph: arm == Arm::LexicalGraph,
            ..RetrieverSupport::default()
        },
        &inputs,
    );
    (request, graph_plan, plan)
}

fn retriever_name(kind: RetrieverKind) -> Result<&'static str, String> {
    match kind {
        RetrieverKind::Lexical => Ok("Lexical"),
        RetrieverKind::HyperGraph => Ok("HyperGraph"),
        _ => Err("unexpected retriever in baseline planner export".into()),
    }
}

/// Capture the concrete in-process synthetic Source, Unit, Read and planner
/// inputs before any arm. The CLI and L/LG execution use this same crate.
pub fn export_synthetic_input(corpus: &Corpus, window: usize) -> Result<Value, String> {
    corpus.validate()?;
    if window == 0 || window > 256 {
        return Err("invalid baseline export window".into());
    }
    let source = source(corpus);
    let mut rows = Vec::new();
    let mut bindings = Vec::new();
    let mut parents = Vec::new();
    let mut allowed = Vec::new();
    let mut denied = Vec::new();
    let mut unknown = Vec::new();
    let mut included_unit_ids = Vec::new();
    for record in &corpus.records {
        let parent = json!({"source_id": corpus.source_id.to_string(),
                            "version_id": record.unit.source_native_version});
        parents.push(json!({"source_id": corpus.source_id.to_string(),
                            "version_id": record.unit.source_native_version,
                            "current": record.current_version}));
        match record.current_read {
            ReadState::Allowed => allowed.push(parent),
            ReadState::Denied => denied.push(parent),
            ReadState::Unknown => unknown.push(parent),
        }
        for unit in std::iter::once(&record.unit).chain(record.additional_units.iter()) {
            let row = json!({
                "docid": format!("{}#{}", record.index, unit.part_ordinal),
                "resource_index": record.index,
                "source_id": unit.source_id.to_string(),
                "source_generation": unit.generation_id.to_string(),
                "source_snapshot": unit.source_snapshot,
                "version_id": unit.source_native_version,
                "part_id": unit.source_native_part_id,
                "logical_path": unit.logical_path,
                "part_ordinal": unit.part_ordinal,
                "unit_ordinal": unit.unit_ordinal,
                "unit_id": unit.unit_id,
                "locator": format!("text-lines:{}:{}", unit.line_start, unit.line_end),
                "profile": unit.profile,
                "parser_build_id": unit.parser_build_id,
                "representation_ref": unit.authoritative_representation_ref,
                "raw_sha256": unit.raw_sha256,
                "raw_size_bytes": unit.raw_size_bytes,
                "text_sha256": unit.text_sha256,
                "text": unit.text,
            });
            let mut binding = row.clone();
            binding.as_object_mut().unwrap().remove("text");
            binding.as_object_mut().unwrap().remove("resource_index");
            bindings.push(binding);
            rows.push(row);
            if record.eligible && record.temporal_valid {
                included_unit_ids.push(unit.unit_id.clone());
            }
        }
    }
    let mut judgments = Vec::new();
    let mut eligible_by_query = BTreeMap::new();
    for query in &corpus.queries {
        let mut grades: BTreeMap<String, u8> = BTreeMap::new();
        for qrel in corpus.qrels.iter().filter(|q| q.query_id == query.query_id) {
            let record = &corpus.records[qrel.resource_index];
            let part_ordinal = if query.query_id == "qpart" { 1 } else { 0 };
            judgments.push(json!({"query_id": query.query_id,
                                  "docid": format!("{}#{part_ordinal}", qrel.resource_index),
                                  "grade": qrel.grade}));
            if record.eligible
                && record.temporal_valid
                && record.current_version
                && record.current_read == ReadState::Allowed
            {
                grades.insert(record.unit.source_native_version.clone(), qrel.grade);
            }
        }
        let parents = grades.iter().map(|(version_id, grade)| json!({
            "source_id": corpus.source_id.to_string(), "version_id": version_id, "grade": grade,
        })).collect::<Vec<_>>();
        eligible_by_query.insert(query.query_id.clone(), parents);
    }
    let mut baseline_plans = Vec::new();
    for arm in [Arm::Lexical, Arm::LexicalGraph] {
        for query in &corpus.queries {
            let (_, _, plan) = planned_baseline_query(corpus, &source, query, ACTOR, arm);
            let retriever_kinds = plan
                .initial_actions
                .iter()
                .map(|action| retriever_name(action.retriever).map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?;
            let retriever_ids = plan
                .initial_actions
                .iter()
                .map(|action| action.retriever_id.clone())
                .collect::<Vec<_>>();
            baseline_plans.push(json!({"arm": arm.name(), "query_id": query.query_id,
                                       "retriever_kinds": retriever_kinds,
                                       "retriever_ids": retriever_ids,
                                       "s1_retriever_order": plan.s1_retriever_order}));
        }
    }
    let query_rows = corpus.queries.iter().map(|q| json!({
        "query_id": q.query_id, "text": q.text, "split": "development", "family": q.query_id,
    })).collect::<Vec<_>>();
    let query_ids = corpus
        .queries
        .iter()
        .map(|q| q.query_id.clone())
        .collect::<Vec<_>>();
    let eligible_queries = eligible_by_query
        .into_iter()
        .map(|(query_id, parents)| json!({"query_id": query_id, "parents": parents}))
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": "p2-synthetic-executed-input-v1",
        "seed": corpus.seed,
        "scale": corpus.records.len(),
        "window": window,
        "actor": ACTOR,
        "artifacts": {
            "corpus_text_sha256": {"schema": "p2-corpus-rows-v1", "rows": rows},
            "query_text_sha256": {"schema": "p2-query-rows-v1", "queries": query_rows},
            "label_split_sha256": {"schema": "p2-label-split-v1", "development": query_ids,
                                    "untouched_holdout": []},
            "source_snapshot_sha256": {"schema": "p2-source-snapshot-v1",
                "source_generation": corpus.generation_id.to_string(), "parents": parents},
            "current_read_sha256": {"schema": "p2-current-read-v1", "actor": ACTOR,
                "allowed": allowed, "denied": denied, "unknown": unknown},
            "filter_sha256": {"schema": "p2-filter-v1", "included_unit_ids": included_unit_ids},
            "eligible_parent_set_sha256": {"schema": "p2-eligible-qrels-v1", "queries": eligible_queries},
            "unit_binding_sha256": {"schema": "p2-unit-bindings-v1", "bindings": bindings},
            "judgments_sha256": {"schema": "p2-judgments-v1", "judgments": judgments},
            "planner_output": {"schema": "p2-planner-output-v1", "seed": corpus.seed,
                               "window": window, "baseline_plans": baseline_plans},
        }
    }))
}

fn state_gate(state: ApplicabilityState, why: &str) -> HardGateEvaluation {
    HardGateEvaluation {
        state,
        reasons: if state == ApplicabilityState::Applicable {
            vec![]
        } else {
            vec![why.into()]
        },
        gaps: vec![],
    }
}

fn gates(eligible: bool, temporal_valid: bool) -> CandidateHardGates {
    CandidateHardGates {
        applicability: ApplicabilityEvaluation {
            state: if eligible {
                ApplicabilityState::Applicable
            } else {
                ApplicabilityState::Excluded
            },
            matched_conditions: vec![],
            resolved_discriminators: vec![],
            remaining_nonblocking_unknowns: vec![],
            gaps: vec![],
            reasons: if eligible {
                vec![]
            } else {
                vec!["Source eligibility=false".into()]
            },
        },
        structured: state_gate(ApplicabilityState::Applicable, ""),
        access: state_gate(ApplicabilityState::Applicable, ""),
        temporal: state_gate(
            if temporal_valid {
                ApplicabilityState::Applicable
            } else {
                ApplicabilityState::Excluded
            },
            "Source effective interval ended",
        ),
    }
}

#[derive(Clone, Debug)]
pub struct StageTrace {
    pub retriever_id: String,
    pub raw_parent_ranks: Vec<usize>,
    pub eligible_gold_misses: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct QueryRun {
    pub query_id: String,
    pub parent_ranks: Vec<usize>,
    pub stages: Vec<StageTrace>,
    pub query_time: Duration,
    pub fusion_time: Duration,
}

fn index_by_id(corpus: &Corpus) -> BTreeMap<ResourceId, usize> {
    corpus
        .records
        .iter()
        .map(|record| (rid(record.unit.resource_id), record.index))
        .collect()
}

pub async fn run_arm(
    harness: &Harness,
    arm: Arm,
    pinned_generation: ProjectionGenerationKey,
    actor: &str,
    window: usize,
) -> Result<Vec<QueryRun>, String> {
    if pinned_generation != harness.manifest.key() || actor != ACTOR || window == 0 || window > 256
    {
        return Err("pin, actor, or bounded lexical window mismatch".into());
    }
    let by_id = index_by_id(&harness.corpus);
    let ports = RetrievalExecutionPorts {
        directory: None,
        structured: None,
        lexical: Some(&harness.lexical),
        hypergraph: Some(&harness.graph),
        graph_resource_access: Some(harness.access.as_ref()),
        remote: None,
        access: harness.access.as_ref(),
    };
    let mut runs = Vec::new();
    for query in &harness.corpus.queries {
        let (request, graph_plan, plan) =
            planned_baseline_query(&harness.corpus, &harness.source, query, actor, arm);
        let lexical_query = LexicalQuery::new(&query.text, window);
        let mut lists = Vec::new();
        let mut stages = Vec::new();
        let query_start = Instant::now();
        for action in &plan.initial_actions {
            if !matches!(
                action.retriever,
                RetrieverKind::Lexical | RetrieverKind::HyperGraph
            ) {
                return Err("unexpected retriever in baseline".into());
            }
            let result = RetrievalExecutor::execute(
                &ports,
                RetrievalExecutionInput {
                    action,
                    generation: pinned_generation,
                    request: &request,
                    structured_filters: &[],
                    lexical_query: Some(&lexical_query),
                    body_query: None,
                    graph_plan: graph_plan.as_ref(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;
            let mut raw_parent_ranks = Vec::new();
            let mut hits = Vec::new();
            for hit in result.hits {
                if hit.generation != pinned_generation {
                    return Err("retriever generation mismatch".into());
                }
                let parent = hit
                    .candidate
                    .resource_ref
                    .ok_or("candidate parent missing")?;
                let index = *by_id
                    .get(&parent)
                    .ok_or("candidate parent not in Source snapshot")?;
                if !harness.resolve_source_parent(index, actor, pinned_generation)? {
                    continue;
                }
                raw_parent_ranks.push(index);
                let record = &harness.corpus.records[index];
                hits.push(RankedCandidateHit {
                    candidate: hit.candidate,
                    hard_gates: gates(record.eligible, record.temporal_valid),
                    identity_evidence: vec![],
                    raw_score: None,
                    evidence_refs: vec![],
                });
            }
            let eligible_gold: BTreeSet<_> = harness
                .corpus
                .qrels
                .iter()
                .filter(|q| q.query_id == query.query_id && q.grade > 0)
                .map(|q| q.resource_index)
                .collect();
            stages.push(StageTrace {
                retriever_id: action.retriever_id.clone(),
                eligible_gold_misses: eligible_gold
                    .difference(&raw_parent_ranks.iter().copied().collect())
                    .copied()
                    .collect(),
                raw_parent_ranks,
            });
            lists.push(RetrieverRankList {
                retriever_id: action.retriever_id.clone(),
                generation: pinned_generation,
                hits,
            });
        }
        let query_time = query_start.elapsed();
        if lists
            .iter()
            .map(|list| &list.retriever_id)
            .collect::<Vec<_>>()
            != plan.s1_retriever_order.iter().collect::<Vec<_>>()
        {
            return Err("S1 action/list order mismatch".into());
        }
        let fusion_start = Instant::now();
        let fused = CandidateFederator::merge(&lists, FusionStrategy::PriorityConcat)
            .map_err(|e| e.to_string())?;
        let mut parent_ranks = Vec::new();
        let mut seen = BTreeSet::new();
        for candidate in fused.ranked {
            let hit = candidate.hits.first().ok_or("empty fused candidate")?;
            let parent = hit.candidate.resource_ref.ok_or("fused parent missing")?;
            let index = *by_id.get(&parent).ok_or("fused parent not in snapshot")?;
            let record = &harness.corpus.records[index];
            // Source-owned current Read/current Version is checked again at publication.
            if !harness.resolve_source_parent(index, actor, pinned_generation)?
                || !record.temporal_valid
                || !record.eligible
            {
                continue;
            }
            if seen.insert(index) {
                parent_ranks.push(index);
            }
        }
        runs.push(QueryRun {
            query_id: query.query_id.clone(),
            parent_ranks,
            stages,
            query_time,
            fusion_time: fusion_start.elapsed(),
        });
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use search_application::ports::LexicalRetrieverPort;

    use super::*;
    use crate::corpus::SEED;

    #[tokio::test]
    async fn restricted_only_query_reaches_tantivy_but_not_authorized_trace() {
        let harness = Harness::build(Corpus::synthetic(32, SEED).unwrap()).unwrap();
        let query = harness
            .corpus
            .queries
            .iter()
            .find(|q| q.query_id == "qaccess")
            .unwrap();
        let request = request(query, ACTOR);
        let raw = harness
            .lexical
            .retrieve(
                pinned(&harness),
                &request,
                &LexicalQuery::new(&query.text, 20),
            )
            .await
            .unwrap();
        let by_id = index_by_id(&harness.corpus);
        let matches = raw
            .iter()
            .map(|candidate| by_id[&candidate.resource_ref.unwrap()])
            .collect::<BTreeSet<_>>();
        assert_eq!(matches, BTreeSet::from([6, 8, 16, 18, 26, 28]));
        let run = run_arm(&harness, Arm::LexicalGraph, pinned(&harness), ACTOR, 20)
            .await
            .unwrap();
        let restricted = run.iter().find(|r| r.query_id == "qaccess").unwrap();
        assert!(restricted.parent_ranks.is_empty());
        assert!(
            restricted
                .stages
                .iter()
                .all(|stage| stage.raw_parent_ranks.is_empty())
        );
    }
}
