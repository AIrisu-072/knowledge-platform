//! P5-04: actor-visible Claims in the Discover route.

#[path = "support/remote_discovery.rs"]
mod discovery;
#[path = "support/remote.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use discovery::*;
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::discover_route::{
    DiscoverInput, DiscoverRouteService, DiscoverRouteWiring,
};
use search_application::public_projection::{
    DiscoveryEvaluationView, PublicClaimState, PublicSufficiency,
};
use search_application::remote_disclosure::{RemoteWiring, ScopedDisclosureGate};
use search_application::remote_evidence::RegisteredLineage;
use search_application::remote_generation::REMOTE_CLAIM_SUBJECT;
use search_application::remote_lease::SystemLeaseClock;
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::scoped::TenantId;
use search_application::search_query::{PublicGapCode, SearchCoverage};
use search_application::source_registration::TrustedVisibleRegistry;
use search_application::source_registry::InMemorySourceRegistry;
use search_application::visible_claim::{
    ActorVisibleClaimCatalogPort, ClaimDefinition, InMemoryClaimCatalog,
};
use search_core::id::{ClaimId, ProjectionGenerationId, SourceId};
use search_core::projection::ProjectionGenerationKey;
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use support::*;
use uuid::Uuid;

const FOREIGN: u128 = 71;
const HIDDEN: u128 = 72;
const WITHDRAWN: u128 = 73;
const UNKNOWN: u128 = 74;

fn definition(claim: u128, tenant: &str, source: SourceId, predicate: &str) -> ClaimDefinition {
    ClaimDefinition {
        claim_id: discovery::claim(claim),
        tenant: TenantId::new(tenant).unwrap(),
        source_id: source,
        subject_ref: REMOTE_CLAIM_SUBJECT.into(),
        predicate: predicate.into(),
        expected_value: None,
        revision: 1,
    }
}

fn catalog(remote: &Remote) -> InMemoryClaimCatalog {
    let source = remote.registration.source_id();
    let catalog = InMemoryClaimCatalog::new(vec![
        definition(TITLE, "tenant-a", source, "catalog.title"),
        definition(DEPARTMENT, "tenant-a", source, "catalog.department"),
        definition(FOREIGN, "tenant-b", source, "catalog.title"),
        definition(
            HIDDEN,
            "tenant-a",
            SourceId::from_uuid(Uuid::from_u128(9_999)),
            "catalog.title",
        ),
        definition(WITHDRAWN, "tenant-a", source, "catalog.title"),
    ]);
    catalog.withdraw(discovery::claim(WITHDRAWN)).unwrap();
    catalog
}

fn input(claims: &[u128]) -> DiscoverInput {
    DiscoverInput {
        purpose: "find a rule".into(),
        required_resource_types: vec![ResourceKind::Knowledge],
        required_claim_ids: claims
            .iter()
            .map(|value| discovery::claim(*value))
            .collect(),
        temporal_target: None,
        business_timezone: None,
        query: None,
        coverage: SearchCoverage::TitleAndPermittedMetadata,
        graph: None,
    }
}

/// One Discover call through the route; returns the public view and the
/// provider batch sizes.
async fn discover(
    remote: &Remote,
    claims: Option<&dyn ActorVisibleClaimCatalogPort>,
    input: DiscoverInput,
) -> (Result<DiscoveryEvaluationView, ApiError>, Vec<usize>) {
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let registry = TrustedVisibleRegistry::new(&remote.authority, &visibility, &remote.catalog);
    let verifier = Verifier::shared("snapshot-1");
    let port = Port {
        remote,
        visibility: &visibility,
        verifier: &verifier,
        answers: answers(Some("doc-2"), Some("doc-1")),
        reverse: false,
        batches: Mutex::new(vec![]),
    };
    let nothing = Nothing;
    let access = AllowAll;
    let sources = InMemorySourceRegistry::default();
    let route = DiscoverRouteService::new(DiscoverRouteWiring {
        config: config(source, 2),
        sources: &sources,
        generations: &nothing,
        concepts: &nothing,
        retrieval: RetrievalExecutionPorts {
            directory: None,
            structured: None,
            lexical: None,
            hypergraph: None,
            graph_resource_access: None,
            remote: None,
            access: &access,
            vector: None,
        },
        assertions: &nothing,
        evidence: &nothing,
        probe: None,
        probe_catalog: None,
        source_policy: None,
        authority: &remote.authority,
        visibility: &visibility,
        registry: &registry,
        claims,
        remote: vec![(
            source,
            RemoteWiring {
                port: &port,
                lineage: RegisteredLineage::new(&remote.registration, vec![]).unwrap(),
                provenance: &Lookup,
                evaluation_ttl: Duration::from_secs(5),
                idle_timeout: None,
            },
        )],
        clock: Arc::new(SystemLeaseClock),
        disclosure_ttl: Duration::from_secs(30),
    });
    let context = SearchOperationContext::authenticate(
        &remote.authority,
        &remote.handle,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let snapshot = prepare_api_visible_sources(&remote.authority, &registry, &context)
        .await
        .unwrap();
    let outcome = match route.discover(&context, &snapshot, input).await {
        Ok(mut disclosure) => {
            let gate = ScopedDisclosureGate::new(&remote.authority, &visibility);
            let mut view = None;
            disclosure
                .with_disclosure(&gate, |disclosed| {
                    view = Some(disclosed.public(Uuid::new_v4()));
                    Ok(())
                })
                .await
                .unwrap();
            Ok(view.unwrap())
        }
        Err(error) => Err(error),
    };
    let batches = port.batches.lock().unwrap().clone();
    (outcome, batches)
}

/// The claim-independent shape of a view.
fn shape(
    view: &DiscoveryEvaluationView,
) -> (
    PublicSufficiency,
    Vec<PublicClaimState>,
    Vec<(PublicGapCode, bool)>,
) {
    (
        view.sufficiency,
        view.evidence
            .iter()
            .map(|evidence| evidence.state)
            .collect(),
        view.gaps
            .iter()
            .map(|gap| (gap.code, gap.blocking))
            .collect(),
    )
}

#[tokio::test]
async fn unknown_foreign_hidden_revoked_claim_same_unresolved_gap() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let catalog = catalog(&remote);
    let mut shapes = Vec::new();
    for claim in [UNKNOWN, FOREIGN, HIDDEN, WITHDRAWN] {
        let (view, _) = discover(&remote, Some(&catalog), input(&[claim])).await;
        let view = view.unwrap();
        assert_eq!(view.sufficiency, PublicSufficiency::Unresolved);
        assert!(
            view.gaps
                .iter()
                .any(|gap| gap.code == PublicGapCode::RequiredClaimUnresolved && gap.blocking)
        );
        assert!(view.evidence.iter().all(
            |evidence| evidence.state == PublicClaimState::Unknown && evidence.value.is_none()
        ));
        shapes.push(shape(&view));
    }
    assert!(shapes.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
async fn required_claim_not_dropped_or_sufficient() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let catalog = catalog(&remote);
    // Positive control: both visible Claims are Supported.
    let (view, _) = discover(&remote, Some(&catalog), input(&[TITLE, DEPARTMENT])).await;
    assert_eq!(view.unwrap().sufficiency, PublicSufficiency::Sufficient);
    // A satisfiable Claim plus an unknown one: neither dropped, never sufficient.
    let (view, _) = discover(&remote, Some(&catalog), input(&[TITLE, UNKNOWN])).await;
    let view = view.unwrap();
    assert_ne!(view.sufficiency, PublicSufficiency::Sufficient);
    let title = discovery::claim(TITLE);
    let unknown = discovery::claim(UNKNOWN);
    assert!(view.evidence.iter().any(
        |evidence| evidence.claim_id == title && evidence.state == PublicClaimState::Supported
    ));
    assert!(view.evidence.iter().any(
        |evidence| evidence.claim_id == unknown && evidence.state == PublicClaimState::Unknown
    ));
    assert!(
        view.gaps
            .iter()
            .any(|gap| gap.code == PublicGapCode::RequiredClaimUnresolved && gap.blocking)
    );
    // No visible Source can evaluate the unknown Claim, and the result says so.
    assert!(
        view.gaps
            .iter()
            .any(|gap| gap.code == PublicGapCode::UnsupportedCoverage && gap.blocking)
    );
}

#[tokio::test]
async fn selector_generation_parent_subject_field_rechecked() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let catalog = catalog(&remote);
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let actor = remote.binding.actor();
    let scopes = vec![context.source_scope().clone()];
    let source = remote.registration.source_id();
    let key = |source: SourceId| ProjectionGenerationKey {
        source_id: source,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
    };
    let title: ClaimId = discovery::claim(TITLE);
    let binding = catalog.bind(actor, &scopes, title).await.unwrap().unwrap();
    let selector = catalog
        .selector_for_visible(&binding, key(source))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (selector.subject_ref.as_str(), selector.predicate.as_str()),
        (REMOTE_CLAIM_SUBJECT, "catalog.title")
    );
    // Another Source's generation never answers for this Claim.
    let other = SourceId::from_uuid(Uuid::from_u128(4_002));
    assert!(
        catalog
            .selector_for_visible(&binding, key(other))
            .await
            .unwrap()
            .is_none()
    );
    // A changed field (new revision) invalidates the old binding.
    let mut changed = definition(TITLE, "tenant-a", source, "catalog.department");
    changed.revision = 2;
    catalog.upsert(changed).unwrap();
    assert!(
        catalog
            .selector_for_visible(&binding, key(source))
            .await
            .unwrap()
            .is_none()
    );
    // An invisible scope never binds.
    assert!(catalog.bind(actor, &[], title).await.unwrap().is_none());
}

#[tokio::test]
async fn claim_catalog_missing_blocks_full_discover() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let (view, batches) = discover(&remote, None, input(&[TITLE])).await;
    assert_eq!(view.unwrap_err(), ApiError::DependencyUnavailable);
    assert!(batches.is_empty());
}
