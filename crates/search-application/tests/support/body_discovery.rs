//! Shared Discovery fixture for P1 body-scope contracts (A02/A03/A04).
#![allow(dead_code, unused_imports)]

pub use std::collections::{BTreeMap, BTreeSet};
pub use std::sync::{Arc, Mutex};

pub use search_application::SearchError;
pub use search_application::body_ports::{
    BodyCoverageGapPort, CONTAINS_EXACT_PREDICATE, ExactTextEvidencePort, ExactTextSelector,
    KnowledgeUnitHitRef, LexicalHit, LexicalRetrievalBatch, VerifiedExtractedTextEvidence,
};
pub use search_application::content_scope::{BodySearchSpec, DiscoveryScope};
pub use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, TemporalPolicy,
};
pub use search_application::materialization::ProbeBudget;
pub use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    DirectoryRetrieverPort, EvidenceResolverPort, LexicalFieldScope, LexicalQuery,
    LexicalRetrieverPort, ProjectionGenerationStore, ResolvedAssertionEvidence, SourceRegistryPort,
    StructuredFacetFilter, StructuredRetrievalHit, StructuredRetrieverPort,
};
pub use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
pub use search_application::retrieval_execution::RetrievalExecutionPorts;
pub use search_application::routing::RoutingConstraints;
pub use search_core::assertion::{Assertion, AssertionOrigin};
pub use search_core::discovery::{
    CandidateIdentityClass, DiscoveryNeed, DiscoveryRequest, DiscoveryResult, FederatedCandidate,
    GapReason, InformationGap,
};
pub use search_core::evidence::{
    ClaimState, EvidenceRequirement, EvidenceRole, EvidenceSufficiency,
};
pub use search_core::id::{
    ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, SourceId,
};
pub use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
pub use search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    FormatSettings, NativeLocator, RawBinding, ResourceVersionRef, TextSpan, UnitId,
};
pub use search_core::observation::Coverage;
pub use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
pub use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
pub use search_core::resource::ResourceKind;
pub use search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
pub use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
pub use time::OffsetDateTime;
pub use uuid::Uuid;

pub fn sid() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}
pub fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
pub fn cid() -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(3))
}

pub fn manifest() -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: sid(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(5)),
        projection_schema_version: "v1".into(),
        lens_version: 1,
        semantic_registry_version: "v1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: "snap-5".into(),
        resource_count: 2,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: "digest-5".into(),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

pub fn projection(resource: ResourceId) -> CompiledResourceProjection {
    CompiledResourceProjection {
        manifest: manifest(),
        retention_mode: RetentionMode::PersistentResource,
        directory: DirectoryProjection {
            resource_ref: resource,
            resource_version: None,
            kind: ResourceKind::Document,
            canonical_name: "synthetic".into(),
            title: None,
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: resource,
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets: BTreeMap::new(),
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: resource,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: resource,
            access_scope: None,
            source_access_model: None,
        },
        relations: vec![],
    }
}

pub fn candidate(resource: ResourceId, retriever: &str) -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        format!("{}:{}", sid().as_uuid(), resource.as_uuid()),
        CandidateIdentityClass::DurableResource,
        sid(),
        retriever,
    );
    candidate.resource_ref = Some(resource);
    candidate
}

pub fn profile() -> ExtractionProfileId {
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

pub fn unit_hit(parent: ResourceId) -> KnowledgeUnitHitRef {
    let version = ResourceVersionRef {
        source_id: sid(),
        resource_id: parent,
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: "part-1".into(),
        logical_path: "本文/primary".into(),
        ordinal: 0,
    };
    let locator = NativeLocator::Text {
        line_start: 0,
        line_end: 1,
    };
    let profile = profile();
    KnowledgeUnitHitRef {
        generation: manifest().key(),
        parent_resource: parent,
        unit_id: UnitId::derive(&version, &part, &profile, &locator, 0).unwrap(),
        version,
        part,
        authoritative_representation_ref: "representation-1".into(),
        span: TextSpan::new("本文語を含む", 0, 9).unwrap(),
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

pub fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(2)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find body".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Document],
            required_claims: vec![cid()],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![cid()]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(4)),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "principal".into(),
    }
}

pub fn config() -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![sid()],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Capability,
        retriever_support: RetrieverSupport {
            structured: true,
            directory: true,
            lexical: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs: RetrievalInputs {
            lexical_query: Some("constructor text".into()),
            max_initial_retrievers_per_source: 1,
            ..RetrievalInputs::default()
        },
        structured_filters: vec![],
        discriminators: vec![],
        // A constructor-time query must never become the body query.
        lexical_query: Some(LexicalQuery::new("constructor text", 10)),
        temporal_policy: TemporalPolicy {
            require_effective_at_target: false,
            max_current_age_seconds: None,
        },
        probe_budget: ProbeBudget {
            max_content_bytes: 1000,
            max_latency_ms: 100,
            max_remote_calls: 1,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 10,
        evaluation_currency: "USD".into(),
    }
}

pub fn body(text: &str) -> DiscoveryScope {
    DiscoveryScope::BodyRequired(BodySearchSpec {
        query: LexicalQuery::body_only(text, 10),
        exact_text_claim: None,
    })
}

pub struct NoConcept;
impl ConceptResolver for NoConcept {
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

pub enum BodyPort {
    Refuse,
    Batch(LexicalRetrievalBatch),
}

pub struct Fixture {
    pub source: DiscoverableSource,
    pub calls: Mutex<Vec<String>>,
    pub body_queries: Mutex<Vec<LexicalQuery>>,
    pub body: BodyPort,
    /// Trusted exact-text selector and the Source verification result.
    pub exact_selector: Option<ExactTextSelector>,
    pub exact_verified: Option<VerifiedExtractedTextEvidence>,
    /// Access-filtered coverage gaps; `None` leaves the coverage port unwired.
    pub coverage: Option<Vec<InformationGap>>,
}

impl Fixture {
    pub fn new(body: BodyPort) -> Self {
        let mut source = DiscoverableSource::new(
            sid(),
            "document",
            EnumerationSemantics::Complete,
            RetentionMode::PersistentResource,
        );
        source.resource_types = vec![ResourceKind::Document];
        source.discovery_modes = vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ];
        Self {
            source,
            calls: Mutex::new(vec![]),
            body_queries: Mutex::new(vec![]),
            body,
            exact_selector: None,
            exact_verified: None,
            coverage: Some(vec![]),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn service(&self, config: DiscoveryConfig) -> DiscoveryService<'_> {
        let service = DiscoveryService::new(
            config,
            DiscoveryPorts {
                sources: self,
                generations: self,
                concepts: self,
                retrieval: RetrievalExecutionPorts {
                    directory: Some(self),
                    structured: Some(self),
                    lexical: Some(self),
                    hypergraph: None,
                    graph_resource_access: None,
                    access: self,
                },
                selectors: self,
                assertions: self,
                evidence: self,
                probe: None,
                probe_catalog: None,
                source_policy: None,
            },
        )
        .unwrap()
        .with_exact_text_evidence(self);
        match self.coverage {
            Some(_) => service.with_body_coverage(self),
            None => service,
        }
    }
}

impl SourceRegistryPort for Fixture {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>> {
        Box::pin(async move { Ok((source_id == sid()).then(|| self.source.clone())) })
    }
    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>> {
        Box::pin(async move { Ok(vec![self.source.clone()]) })
    }
}

impl ProjectionGenerationStore for Fixture {
    fn begin_generation<'a>(
        &'a self,
        _: search_application::projection::PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: search_application::projection::PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn stage_resource<'a>(
        &'a self,
        _: search_application::projection::PersistableResourceProjection,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: search_application::ports::SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn pin_current<'a>(
        &'a self,
        _: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async { Ok(Some(manifest())) })
    }
    fn resource_at<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        resource: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        Box::pin(async move { Ok(Some(projection(resource))) })
    }
}

impl ConceptRegistryPort for Fixture {
    fn pin_view<'a>(
        &'a self,
        _: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        Box::pin(async { Ok(Arc::new(NoConcept) as Arc<dyn ConceptResolver + Send + Sync>) })
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

impl StructuredRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        _: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>> {
        self.calls.lock().unwrap().push("structured".into());
        // A title/metadata hit that fully supports the claim in Normal scope.
        Box::pin(async {
            Ok(vec![StructuredRetrievalHit {
                candidate: candidate(rid(20), "structured"),
                outcomes: vec![],
            }])
        })
    }
}

impl DirectoryRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        self.calls.lock().unwrap().push("directory".into());
        Box::pin(async { Ok(vec![candidate(rid(20), "directory")]) })
    }
}

impl LexicalRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        _: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        self.calls.lock().unwrap().push("lexical".into());
        Box::pin(async { Ok(vec![candidate(rid(20), "lexical")]) })
    }

    fn retrieve_body<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, LexicalRetrievalBatch> {
        self.calls.lock().unwrap().push("lexical-body".into());
        self.body_queries.lock().unwrap().push(query.clone());
        Box::pin(async move {
            match &self.body {
                BodyPort::Refuse => Err(SearchError::SourceUnavailable("no body bundle".into())),
                BodyPort::Batch(batch) => Ok(batch.clone()),
            }
        })
    }
}

impl CurrentCandidateAccessEvaluatorPort for Fixture {
    fn evaluate<'a>(
        &'a self,
        _: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

impl CurrentAccessEvaluatorPort for Fixture {
    fn evaluate<'a>(&'a self, _: ResourceId, _: &'a str) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

impl ClaimSelectorPort for Fixture {
    fn selector_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            Ok((claim_id == cid()).then(|| ClaimSelector {
                claim_id,
                subject_ref: "resource".into(),
                predicate: "supports".into(),
                expected_value: Some(TypedValue::Bool(true)),
            }))
        })
    }
}

impl AssertionStorePort for Fixture {
    fn assertions_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async {
            let mut assertion = Assertion::new(
                "resource",
                "supports",
                TypedValue::Bool(true),
                "native-1",
                AssertionOrigin::Declared,
                "scope",
                OffsetDateTime::UNIX_EPOCH,
            );
            assertion.evidence_refs = vec!["evidence-1".into()];
            Ok(vec![assertion])
        })
    }
}

impl EvidenceResolverPort for Fixture {
    fn resolve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move {
            Ok(Some(ResolvedAssertionEvidence {
                generation: key,
                source_id: sid(),
                resource_id: resource,
                evidence_ref: evidence_ref.to_owned(),
                upstream_origin: "publisher-1".into(),
                role: EvidenceRole::Primary,
                citation_chain: vec![],
                content_digest: Some("sha256:real".into()),
                is_summary: false,
            }))
        })
    }
}

pub fn batch(hits: Vec<LexicalHit>, exhausted: bool) -> BodyPort {
    BodyPort::Batch(LexicalRetrievalBatch {
        hits,
        exhausted_matching_units: exhausted,
    })
}

pub fn gap_codes(result: &search_core::discovery::DiscoveryResult) -> Vec<String> {
    result
        .unresolved_gaps
        .iter()
        .filter(|gap| gap.blocking)
        .map(|gap| gap.required_fact.clone())
        .collect()
}

impl ExactTextEvidencePort for Fixture {
    fn selector_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ExactTextSelector>> {
        self.calls.lock().unwrap().push("exact-selector".into());
        Box::pin(async move {
            Ok(self
                .exact_selector
                .clone()
                .filter(|selector| selector.claim_id == claim_id))
        })
    }

    fn resolve_hit<'a>(
        &'a self,
        _: &'a DiscoveryRequest,
        hit: &'a KnowledgeUnitHitRef,
        _: &'a ExactTextSelector,
    ) -> BoxFuture<'a, Option<VerifiedExtractedTextEvidence>> {
        self.calls.lock().unwrap().push("exact-resolve".into());
        Box::pin(async move {
            Ok(self
                .exact_verified
                .clone()
                .filter(|verified| verified.resolved.resource_id == hit.parent_resource))
        })
    }
}

impl BodyCoverageGapPort for Fixture {
    fn coverage_gaps<'a>(
        &'a self,
        _: &'a DiscoveryRequest,
        _: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Vec<InformationGap>> {
        Box::pin(async move { Ok(self.coverage.clone().unwrap_or_default()) })
    }
}

/// Trusted selector binding the required Claim to one parent and literal.
pub fn selector(parent: ResourceId, text: &str) -> ExactTextSelector {
    ExactTextSelector {
        claim_id: cid(),
        parent_resource: parent,
        predicate: CONTAINS_EXACT_PREDICATE.into(),
        expected_exact_text: text.into(),
    }
}

/// What a Source returns after re-reading the raw bytes for one Unit hit.
pub fn verified(parent: ResourceId, text: &str) -> VerifiedExtractedTextEvidence {
    let evidence_ref = format!("document-body:v1:{}", parent.as_uuid());
    let mut assertion = Assertion::new(
        format!("document-version:{}", parent.as_uuid()),
        CONTAINS_EXACT_PREDICATE,
        TypedValue::String(text.into()),
        sid().as_uuid().to_string(),
        AssertionOrigin::Extracted,
        "document:synthetic",
        OffsetDateTime::UNIX_EPOCH,
    );
    assertion.evidence_refs = vec![evidence_ref.clone()];
    VerifiedExtractedTextEvidence {
        assertion,
        resolved: ResolvedAssertionEvidence {
            generation: manifest().key(),
            source_id: sid(),
            resource_id: parent,
            evidence_ref,
            upstream_origin: format!("document:synthetic:version:{}", parent.as_uuid()),
            role: EvidenceRole::Primary,
            citation_chain: vec!["unit:synthetic".into()],
            content_digest: Some("sha256:unit".into()),
            is_summary: false,
        },
        matched_span: TextSpan::new(text, 0, u32::try_from(text.len()).unwrap()).unwrap(),
    }
}

/// A request whose only required Claim is the exact-text Claim.
pub fn exact_scope(text: &str) -> DiscoveryScope {
    DiscoveryScope::BodyRequired(BodySearchSpec {
        query: LexicalQuery::body_only(text, 10),
        exact_text_claim: Some(cid()),
    })
}
