#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use search_application::discovery_service::{DiscoveryConfig, TemporalPolicy};
use search_application::indexing_service::DocumentSourceEvent;
use search_application::materialization::ProbeBudget;
use search_application::ports::{
    AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort, EvidenceResolverPort,
    LexicalQuery, ResolvedAssertionEvidence, SemanticRegistrySnapshot,
};
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::routing::RoutingConstraints;
use search_core::assertion::Assertion;
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{ClaimId, DiscoveryEvaluationId, NeedId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::profile::DiscoveryLens;
use search_core::projection::ProjectionGenerationKey;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_source_document::{DocumentIndexingConfig, IndexingReceipt, IndexingReceiptStore};
use time::OffsetDateTime;
use uuid::Uuid;

pub const ACCESS_CONTEXT: &str = "trusted-session";

#[derive(Clone, Default)]
pub struct Receipts(pub Arc<Mutex<BTreeMap<Uuid, IndexingReceipt>>>);

impl IndexingReceiptStore for Receipts {
    fn get<'a>(&'a self, event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().get(&event_id).cloned()) })
    }

    fn put<'a>(&'a self, event_id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(event_id, receipt);
            Ok(())
        })
    }
}

pub fn source(source_id: SourceId) -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        source_id,
        "document-platform",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    source.resource_types.push(ResourceKind::Knowledge);
    source
        .discovery_modes
        .push(DiscoveryMode::LocalContentSearch);
    source.discovery_modes.push(DiscoveryMode::LocalDirectory);
    source.access_model = Some("document-current-check".into());
    source
}

pub fn index_config(source_id: SourceId) -> DocumentIndexingConfig {
    DocumentIndexingConfig {
        source: source(source_id),
        lens: DiscoveryLens {
            lens_id: "document-version".into(),
            lens_version: 1,
            resource_type: ResourceKind::Knowledge,
            domain_scope: None,
            source_scope: Some(source_id),
            identity_fields: vec!["document_version_id".into()],
            high_signal_facets: vec!["document_type".into()],
            searchable_fields: vec!["title".into(), "permitted_metadata".into()],
            applicability_fields: vec![],
            temporal_fields: vec![],
            relation_fields: vec![],
            extraction_policy: None,
            projection_policy: None,
        },
        projection_schema_version: "schema-1".into(),
        analyzer_version: "tantivy-default-0.26.2".into(),
        semantic_registry: SemanticRegistrySnapshot::new("registry-1"),
    }
}

pub fn event(event_type: &str, aggregate_id: Uuid) -> DocumentSourceEvent {
    DocumentSourceEvent {
        event_id: Uuid::now_v7(),
        event_type: event_type.into(),
        aggregate_id,
        occurred_at: OffsetDateTime::now_utc(),
    }
}

pub fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::now_utc();
    let claim = ClaimId::from_uuid(Uuid::now_v7());
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::now_v7()),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find Document Version".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: vec![claim],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![claim]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::now_v7()),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: ACCESS_CONTEXT.into(),
    }
}

pub fn discovery_config(source_id: SourceId, query: &str) -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![source_id],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Exploratory,
        retriever_support: RetrieverSupport {
            lexical: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs: RetrievalInputs {
            lexical_query: Some(query.into()),
            max_initial_retrievers_per_source: 1,
            ..RetrievalInputs::default()
        },
        structured_filters: vec![],
        discriminators: vec![],
        lexical_query: Some(LexicalQuery::new(query, 10)),
        temporal_policy: TemporalPolicy::default(),
        probe_budget: ProbeBudget {
            max_content_bytes: 0,
            max_latency_ms: 0,
            max_remote_calls: 0,
            max_monetary_cost_minor_units: 0,
            currency: "JPY".into(),
        },
        max_actions: 1,
        evaluation_currency: "JPY".into(),
    }
}

pub struct NoEvidence;

impl ClaimSelectorPort for NoEvidence {
    fn selector_for<'a>(
        &'a self,
        _generation: ProjectionGenerationKey,
        _claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async { Ok(None) })
    }
}

impl AssertionStorePort for NoEvidence {
    fn assertions_for<'a>(
        &'a self,
        _generation: ProjectionGenerationKey,
        _resource_ref: ResourceId,
        _predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async { Ok(vec![]) })
    }
}

impl EvidenceResolverPort for NoEvidence {
    fn resolve<'a>(
        &'a self,
        _generation: ProjectionGenerationKey,
        _resource_ref: ResourceId,
        _evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async { Ok(None) })
    }
}
