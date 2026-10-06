//! P1-A02 Document coverage preflight: `BodyRequired` only with a trusted,
//! well-formed and request-bound `BodySearchSpec`.

#[path = "support/document_discovery.rs"]
mod discovery_support;

use std::sync::atomic::{AtomicUsize, Ordering};

use search_application::content_scope::BodySearchSpec;
use search_application::discovery_service::{DiscoveryPorts, DiscoveryService};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentCandidateAccessEvaluatorPort, LexicalQuery,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{DiscoveryResult, FederatedCandidate, GapReason};
use search_core::evidence::EvidenceSufficiency;
use search_core::id::{ClaimId, SourceId};
use search_source_document::{
    DocumentCoveragePreflight, DocumentCoverageRequirement, MemoryDocumentIndexRuntime,
};
use uuid::Uuid;

#[derive(Default)]
struct CountingAccess(AtomicUsize);

impl CurrentCandidateAccessEvaluatorPort for CountingAccess {
    fn evaluate<'a>(
        &'a self,
        _: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

fn spec(query: LexicalQuery, claim: Option<ClaimId>) -> BodySearchSpec {
    BodySearchSpec {
        query,
        exact_text_claim: claim,
    }
}

fn assert_blocked(result: &DiscoveryResult, gap: &str) {
    assert!(result.qualified_resources.is_empty());
    assert!(result.evidence_set.is_empty());
    assert!(result.rejected_candidates.is_empty());
    assert!(result.retrieval_trace.is_empty());
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert_eq!(result.unresolved_gaps.len(), 1);
    let only = &result.unresolved_gaps[0];
    assert_eq!(only.required_fact, gap);
    assert_eq!(only.reason, GapReason::UnsupportedCoverage);
    assert!(only.blocking);
}

#[tokio::test]
async fn body_required_without_a_trusted_spec_is_a_blocking_gap() {
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let runtime = MemoryDocumentIndexRuntime::new();
    let reader = runtime.projection_reader();
    let lexical = runtime.lexical_reader();
    let access = CountingAccess::default();
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
                vector: None,
            },
            selectors: &discovery_support::NoEvidence,
            assertions: &discovery_support::NoEvidence,
            evidence: &discovery_support::NoEvidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();

    let request = discovery_support::request();
    let required = request.need.required_claims[0];
    let cases = [
        (
            DocumentCoverageRequirement::BodyRequired,
            None,
            "document.body.search_spec_missing",
        ),
        (
            DocumentCoverageRequirement::BodyRequired,
            Some(spec(LexicalQuery::new("本文", 10), None)),
            "document.body.search_spec_invalid",
        ),
        (
            DocumentCoverageRequirement::BodyRequired,
            Some(spec(LexicalQuery::body_only(" ", 10), None)),
            "document.body.search_spec_invalid",
        ),
        (
            DocumentCoverageRequirement::BodyRequired,
            Some(spec(LexicalQuery::body_only("本文", 257), None)),
            "document.body.search_spec_invalid",
        ),
        // A selector forged for a Claim this request does not require.
        (
            DocumentCoverageRequirement::BodyRequired,
            Some(spec(
                LexicalQuery::body_only("本文", 10),
                Some(ClaimId::from_uuid(Uuid::now_v7())),
            )),
            "document.body.selector_unbound",
        ),
        // A body spec smuggled into a metadata request is a scope swap.
        (
            DocumentCoverageRequirement::TitleAndPermittedMetadata,
            Some(spec(LexicalQuery::body_only("本文", 10), Some(required))),
            "document.body.scope_mismatch",
        ),
    ];
    for (requirement, body, gap) in cases {
        let result =
            DocumentCoveragePreflight::discover(&service, request.clone(), requirement, body)
                .await
                .unwrap();
        assert_blocked(&result, gap);
    }
    assert_eq!(access.0.load(Ordering::SeqCst), 0);

    // A bound spec reaches the shared loop; with no published bundle it stays unresolved.
    let result = DocumentCoveragePreflight::discover(
        &service,
        request.clone(),
        DocumentCoverageRequirement::BodyRequired,
        Some(spec(LexicalQuery::body_only("本文", 10), Some(required))),
    )
    .await
    .unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.unresolved_gaps.iter().any(|gap| gap.blocking));
}
