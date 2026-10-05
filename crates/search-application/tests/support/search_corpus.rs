//! In-memory durable Sources for the P5 Search contract tests: per-Source
//! pinned generations, a lexical retriever over titles and Unit bodies, and
//! current access with scripted revocation, outage and generation changes.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use search_application::SearchError;
use search_application::body_ports::{KnowledgeUnitHitRef, LexicalHit, LexicalRetrievalBatch};
use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, TemporalPolicy,
};
use search_application::materialization::ProbeBudget;
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentCandidateAccessEvaluatorPort, EvidenceResolverPort, LexicalQuery,
    LexicalRetrieverPort, ProjectionGenerationStore, ResolvedAssertionEvidence,
    SemanticRegistrySnapshot, SourceRegistryPort,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::RoutingConstraints;
use search_core::assertion::Assertion;
use search_core::discovery::{CandidateIdentityClass, DiscoveryRequest, FederatedCandidate};
use search_core::id::{ClaimId, ProjectionGenerationId, ResourceId, SourceId};
use search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    FormatSettings, NativeLocator, RawBinding, ResourceVersionRef, TextSpan, UnitId,
};
use search_core::observation::Coverage;
use search_core::predicate::{ConceptResolver, TruthValue};
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, RetentionMode};
use search_core::temporal::TemporalDiscoveryProfile;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Doc {
    pub id: ResourceId,
    pub title: String,
    pub body: Option<String>,
}

pub fn rid(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

pub fn doc(id: u128, title: &str, body: Option<&str>) -> Doc {
    Doc {
        id: rid(id),
        title: title.into(),
        body: body.map(str::to_owned),
    }
}

#[derive(Default)]
pub struct Corpus {
    pub docs: BTreeMap<SourceId, Vec<Doc>>,
    generations: Mutex<BTreeMap<SourceId, u128>>,
    pub denied: Mutex<BTreeSet<ResourceId>>,
    /// Allowed on the first access check, denied afterwards.
    pub revoke_after_first_check: Mutex<BTreeSet<ResourceId>>,
    checked: Mutex<BTreeSet<ResourceId>>,
    pub failing: Mutex<BTreeSet<SourceId>>,
    pub body_refused: Mutex<BTreeSet<SourceId>>,
    pub retrieved: Mutex<Vec<SourceId>>,
}

impl Corpus {
    pub fn new(docs: Vec<(SourceId, Vec<Doc>)>) -> Self {
        Self {
            docs: docs.into_iter().collect(),
            ..Self::default()
        }
    }

    fn generation(&self, source: SourceId) -> ProjectionGenerationKey {
        let seed = *self.generations.lock().unwrap().entry(source).or_insert(1);
        ProjectionGenerationKey {
            source_id: source,
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(
                (source.as_uuid().as_u128() << 8) + seed,
            )),
        }
    }

    /// Publishes a new generation of `source`.
    pub fn bump(&self, source: SourceId) {
        *self.generations.lock().unwrap().entry(source).or_insert(1) += 1;
    }

    fn manifest(&self, source: SourceId) -> ProjectionGenerationManifest {
        let key = self.generation(source);
        ProjectionGenerationManifest {
            source_id: source,
            generation_id: key.generation_id,
            projection_schema_version: "v1".into(),
            lens_version: 1,
            semantic_registry_version: "v1".into(),
            analyzer_version: None,
            embedding_model_version: None,
            graph_schema_version: None,
            source_snapshot: "snapshot".into(),
            resource_count: self.docs.get(&source).map_or(0, Vec::len) as u64,
            relation_count: Some(0),
            coverage: Coverage::CompleteEnumeration,
            digest: "digest".into(),
            built_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn find(&self, key: ProjectionGenerationKey, id: ResourceId) -> Option<&Doc> {
        (self.generation(key.source_id) == key)
            .then(|| self.docs.get(&key.source_id))
            .flatten()?
            .iter()
            .find(|doc| doc.id == id)
    }

    fn candidate(source: SourceId, id: ResourceId) -> FederatedCandidate {
        let mut candidate = FederatedCandidate::new(
            format!("{}:{}", source.as_uuid(), id.as_uuid()),
            CandidateIdentityClass::DurableResource,
            source,
            "lexical",
        );
        candidate.resource_ref = Some(id);
        candidate
    }

    pub fn service(&self) -> DiscoveryService<'_> {
        DiscoveryService::new(self.service_config(), self.ports()).unwrap()
    }

    pub fn ports(&self) -> DiscoveryPorts<'_> {
        DiscoveryPorts {
            sources: self,
            generations: self,
            concepts: self,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: Some(self),
                hypergraph: None,
                graph_resource_access: None,
                remote: None,
                access: self,
            },
            selectors: self,
            assertions: self,
            evidence: self,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        }
    }

    pub fn service_config(&self) -> DiscoveryConfig {
        {
            DiscoveryConfig {
                routing: RoutingConstraints {
                    required_source_ids: vec![],
                    preferred_source_ids: vec![],
                    max_initial_optional_sources: 0,
                },
                retriever_profile: RetrieverProfile::Knowledge,
                retriever_support: RetrieverSupport {
                    lexical: true,
                    ..RetrieverSupport::default()
                },
                retrieval_inputs: RetrievalInputs {
                    max_initial_retrievers_per_source: 2,
                    ..RetrievalInputs::default()
                },
                structured_filters: vec![],
                discriminators: vec![],
                lexical_query: None,
                temporal_policy: TemporalPolicy::default(),
                probe_budget: ProbeBudget {
                    max_content_bytes: 0,
                    max_latency_ms: 0,
                    max_remote_calls: 0,
                    max_monetary_cost_minor_units: 0,
                    currency: "USD".into(),
                },
                max_actions: 8,
                evaluation_currency: "USD".into(),
            }
        }
    }
}

fn profile() -> ExtractionProfileId {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 16);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1024);
    limits.insert(BudgetKey::WorkerOutputBytes, 65_536);
    ExtractionProfileId::for_definition(&ExtractionProfileDefinitionV1 {
        format: FormatId::Text,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [1; 32],
        native_binary_sha256: None,
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: FormatSettings::Text {
            charset: "utf-8".into(),
        },
        limits,
    })
    .unwrap()
}

fn unit_hit(key: ProjectionGenerationKey, parent: ResourceId, text: &str) -> KnowledgeUnitHitRef {
    let version = ResourceVersionRef {
        source_id: key.source_id,
        resource_id: parent,
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: "part-1".into(),
        logical_path: "body/primary".into(),
        ordinal: 0,
    };
    let locator = NativeLocator::Text {
        line_start: 0,
        line_end: 1,
    };
    let profile = profile();
    KnowledgeUnitHitRef {
        generation: key,
        parent_resource: parent,
        unit_id: UnitId::derive(&version, &part, &profile, &locator, 0).unwrap(),
        version,
        part,
        authoritative_representation_ref: "representation-1".into(),
        span: TextSpan::new(text, 0, text.len() as u32).unwrap(),
        text_sha256: [7; 32],
        raw: RawBinding {
            sha256: [9; 32],
            size_bytes: 64,
            media_type: "text/plain".into(),
        },
        profile,
        opaque_locator: "00".into(),
    }
}

impl SourceRegistryPort for Corpus {
    fn get_source<'a>(&'a self, _: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>> {
        Box::pin(async { Ok(None) })
    }
    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>> {
        Box::pin(async { Ok(vec![]) })
    }
}

fn no_write<'a>() -> BoxFuture<'a, ()> {
    Box::pin(async { Err(SearchError::OperationFailed("read-only corpus".into())) })
}

impl ProjectionGenerationStore for Corpus {
    fn begin_generation<'a>(&'a self, _: PersistableGenerationManifest) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn stage_resource<'a>(&'a self, _: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn pin_current<'a>(
        &'a self,
        source: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move {
            Ok(self
                .docs
                .contains_key(&source)
                .then(|| self.manifest(source)))
        })
    }
    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        Box::pin(async move {
            Ok(self.find(key, id).map(|doc| CompiledResourceProjection {
                manifest: self.manifest(key.source_id),
                retention_mode: RetentionMode::PersistentResource,
                directory: DirectoryProjection {
                    resource_ref: id,
                    resource_version: None,
                    kind: ResourceKind::Document,
                    canonical_name: doc.title.clone(),
                    title: Some(doc.title.clone()),
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
            }))
        })
    }
}

impl LexicalRetrieverPort for Corpus {
    fn retrieve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move {
            self.retrieved.lock().unwrap().push(key.source_id);
            if self.failing.lock().unwrap().contains(&key.source_id) {
                return Err(SearchError::SourceUnavailable("timeout".into()));
            }
            Ok(self
                .docs
                .get(&key.source_id)
                .into_iter()
                .flatten()
                .filter(|doc| doc.title.contains(&query.text))
                .map(|doc| Self::candidate(key.source_id, doc.id))
                .collect())
        })
    }

    fn retrieve_body<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, LexicalRetrievalBatch> {
        Box::pin(async move {
            self.retrieved.lock().unwrap().push(key.source_id);
            if self.body_refused.lock().unwrap().contains(&key.source_id)
                || self.failing.lock().unwrap().contains(&key.source_id)
            {
                return Err(SearchError::SourceUnavailable("no body bundle".into()));
            }
            Ok(LexicalRetrievalBatch {
                hits: self
                    .docs
                    .get(&key.source_id)
                    .into_iter()
                    .flatten()
                    .filter(|doc| {
                        doc.body
                            .as_deref()
                            .is_some_and(|body| body.contains(&query.text))
                    })
                    .map(|doc| LexicalHit {
                        candidate: Self::candidate(key.source_id, doc.id),
                        unit_hit: Some(unit_hit(key, doc.id, &query.text)),
                    })
                    .collect(),
                exhausted_matching_units: true,
            })
        })
    }
}

impl CurrentCandidateAccessEvaluatorPort for Corpus {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            let Some(id) = candidate.resource_ref else {
                return Ok(AccessDecision::Unknown);
            };
            if self.denied.lock().unwrap().contains(&id) {
                return Ok(AccessDecision::Denied);
            }
            if self.revoke_after_first_check.lock().unwrap().contains(&id)
                && !self.checked.lock().unwrap().insert(id)
            {
                return Ok(AccessDecision::Denied);
            }
            Ok(AccessDecision::Allowed)
        })
    }
}

struct Unknown;
impl ConceptResolver for Unknown {
    fn same_concept(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
    fn is_a(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
    fn descendant_of(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
}

impl ConceptRegistryPort for Corpus {
    fn pin_view<'a>(
        &'a self,
        _: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        Box::pin(async {
            let resolver: Arc<dyn ConceptResolver + Send + Sync> = Arc::new(Unknown);
            Ok(resolver)
        })
    }
    fn same_concept<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async { Ok(TruthValue::Unknown) })
    }
    fn is_a<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async { Ok(TruthValue::Unknown) })
    }
}

impl ClaimSelectorPort for Corpus {
    fn selector_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async { Ok(None) })
    }
}

impl AssertionStorePort for Corpus {
    fn assertions_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async { Ok(vec![]) })
    }
}

impl EvidenceResolverPort for Corpus {
    fn resolve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async { Ok(None) })
    }
}
