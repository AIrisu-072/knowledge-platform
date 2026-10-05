#[path = "support/document_discovery.rs"]
mod discovery_support;
#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod versioning_support;

use std::sync::Arc;

use document_application::{
    AccessPolicyService, BootstrapRootPolicy, DocumentAccessCheckService, InvocationKind,
    ManagementCommand, ManagementOperationId, VerifiedActorContext,
};
use document_domain::{
    Action, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget,
};
use document_repository_postgres::PostgresDocumentRepository;
use search_application::discovery_service::{DiscoveryPorts, DiscoveryService};
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    CurrentCandidateAccessEvaluatorPort, EvidenceResolverPort, LexicalQuery, LexicalRetrieverPort,
    ResolvedAssertionEvidence,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::evidence::{ClaimState, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;
use search_source_document::{
    DocumentCurrentAccessAdapter, DocumentOutboxIndexer, MemoryDocumentIndexRuntime,
    PostgresDocumentSnapshotReader,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn actor() -> VerifiedActorContext {
    let principal = versioning_support::actor();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
        actions,
    )
    .unwrap()
}

// This is deliberately test-only Claim data. The Document Source adapter does
// not yet project production Assertions or resolve Document evidence (D8).
// It makes a previously visible Version identity observable in every Discovery
// output that must be redacted after the real Document Read grant is revoked.
struct VersionEvidence {
    generation: ProjectionGenerationKey,
    resource: ResourceId,
    claim: ClaimId,
    subject: String,
    evidence_ref: String,
    upstream_origin: String,
}

impl VersionEvidence {
    fn new(
        generation: ProjectionGenerationKey,
        document_id: document_domain::DocumentId,
        resource: ResourceId,
        claim: ClaimId,
    ) -> Self {
        Self {
            generation,
            resource,
            claim,
            subject: format!(
                "document:{}:version:{}",
                document_id.as_uuid(),
                resource.as_uuid()
            ),
            evidence_ref: format!("test-evidence:version:{}", resource.as_uuid()),
            upstream_origin: format!("test-document:{}", document_id.as_uuid()),
        }
    }
}

impl ClaimSelectorPort for VersionEvidence {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            Ok(
                (generation == self.generation && claim_id == self.claim).then(|| ClaimSelector {
                    claim_id,
                    subject_ref: self.subject.clone(),
                    predicate: "test.version.visible".into(),
                    expected_value: Some(TypedValue::Bool(true)),
                }),
            )
        })
    }
}

impl AssertionStorePort for VersionEvidence {
    fn assertions_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async move {
            if generation != self.generation
                || resource_ref != self.resource
                || predicate != "test.version.visible"
            {
                return Ok(vec![]);
            }
            let mut assertion = Assertion::new(
                self.subject.clone(),
                predicate,
                TypedValue::Bool(true),
                self.generation.source_id.as_uuid().to_string(),
                AssertionOrigin::Declared,
                "test-only",
                OffsetDateTime::now_utc(),
            );
            assertion.evidence_refs.push(self.evidence_ref.clone());
            Ok(vec![assertion])
        })
    }
}

impl EvidenceResolverPort for VersionEvidence {
    fn resolve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move {
            Ok((generation == self.generation
                && resource_ref == self.resource
                && evidence_ref == self.evidence_ref)
                .then(|| ResolvedAssertionEvidence {
                    generation,
                    source_id: generation.source_id,
                    resource_id: resource_ref,
                    evidence_ref: evidence_ref.into(),
                    upstream_origin: self.upstream_origin.clone(),
                    role: EvidenceRole::Primary,
                    citation_chain: vec![self.subject.clone()],
                    content_digest: None,
                    is_summary: false,
                }))
        })
    }
}

#[tokio::test]
async fn candidate_access_is_bound_to_source_version_and_current_document_policy() {
    let fixture = versioning_support::fixture().await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        fixture.pool.clone(),
        versioning_support::actor(),
    ));
    repository
        .initialize_root_policy(&actor(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let access = DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let version_resource = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let mut candidate = FederatedCandidate::new(
        format!("{}:{}", source_id.as_uuid(), version_resource.as_uuid()),
        CandidateIdentityClass::DurableResource,
        source_id,
        "lexical",
    );
    candidate.resource_ref = Some(version_resource);

    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    let generation = match indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            fixture.document_id.as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected published generation: {other:?}"),
    };
    let projection = runtime.projection_reader();
    let lexical = runtime.lexical_reader();
    let request = discovery_support::request();
    let evidence = VersionEvidence::new(
        generation,
        fixture.document_id,
        version_resource,
        request.need.required_claims[0],
    );
    assert!(
        projection
            .resource_at(generation, version_resource)
            .await
            .unwrap()
            .is_some()
    );
    let indexed_hits = lexical
        .retrieve(generation, &request, &LexicalQuery::new("Base", 10))
        .await
        .unwrap();
    assert_eq!(indexed_hits.len(), 1);
    assert_eq!(
        access
            .evaluate(&indexed_hits[0], discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed,
        "{indexed_hits:?}"
    );
    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let service = DiscoveryService::new(
        discovery_support::discovery_config(source_id, "Base"),
        DiscoveryPorts {
            sources: &sources,
            generations: &projection,
            concepts: &projection,
            retrieval: RetrievalExecutionPorts {
                remote: None,
                directory: None,
                structured: None,
                lexical: Some(&lexical),
                hypergraph: None,
                graph_resource_access: None,
                access: &access,
            },
            selectors: &evidence,
            assertions: &evidence,
            evidence: &evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let before = service.discover(request.clone()).await.unwrap();
    assert!(
        before
            .qualified_resources
            .iter()
            .any(|item| item.resource_ref == version_resource),
        "{before:?}"
    );
    assert!(
        before
            .evidence_set
            .iter()
            .any(|claim| claim.state == ClaimState::Supported && !claim.evidence_refs.is_empty()),
        "pre-revocation evidence must make redaction observable: {before:?}"
    );
    assert!(before.evidence_set.iter().any(|claim| {
        claim.subject.as_deref() == Some(evidence.subject.as_str())
            && claim.evidence_refs.iter().any(|item| {
                item.evidence_ref.as_deref() == Some(evidence.evidence_ref.as_str())
                    && item.upstream_origin == evidence.upstream_origin
            })
    }));
    assert_eq!(before.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(
        before.qualified_resources[0].evidence_refs,
        vec![evidence.evidence_ref.clone()]
    );
    assert!(
        before
            .retrieval_trace
            .iter()
            .any(|trace| trace.contains(&candidate.candidate_id))
    );
    assert!(
        before
            .qualification_trace
            .iter()
            .any(|trace| trace.contains(&candidate.candidate_id))
    );
    let visible = serde_json::to_string(&before).unwrap();
    assert!(visible.contains(&fixture.document_id.as_uuid().to_string()));
    assert!(visible.contains(&version_resource.as_uuid().to_string()));
    assert!(visible.contains(&candidate.candidate_id));
    assert_eq!(
        access
            .evaluate(&candidate, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );

    let mut wrong_source = candidate.clone();
    wrong_source.source_ref = SourceId::from_uuid(Uuid::now_v7());
    assert_ne!(
        access
            .evaluate(&wrong_source, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    let mut wrong_identity = candidate.clone();
    wrong_identity.candidate_id = "unbound-version".into();
    assert_ne!(
        access
            .evaluate(&wrong_identity, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    let mut unknown = candidate.clone();
    let unknown_resource = ResourceId::from_uuid(Uuid::now_v7());
    unknown.resource_ref = Some(unknown_resource);
    unknown.candidate_id = format!("{}:{}", source_id.as_uuid(), unknown_resource.as_uuid());
    assert_ne!(
        access
            .evaluate(&unknown, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    let mut ephemeral = candidate.clone();
    ephemeral.identity_class = CandidateIdentityClass::EphemeralCandidate;
    assert_eq!(
        access
            .evaluate(&ephemeral, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Denied
    );
    let mut reference_only = candidate.clone();
    reference_only.resource_ref = None;
    assert_eq!(
        access
            .evaluate(&reference_only, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Denied
    );
    assert_ne!(
        access
            .evaluate(&candidate, "forged-request-user")
            .await
            .unwrap(),
        AccessDecision::Allowed
    );

    AccessPolicyService::new(repository)
        .set_access_policy(
            &actor(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(fixture.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([Action::Administer])]),
                reason: "revoke current read".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        access
            .evaluate(&candidate, discovery_support::ACCESS_CONTEXT)
            .await
            .unwrap(),
        AccessDecision::Denied
    );
    // Revocation did not trigger reindexing. The old generation is intact but
    // no candidate identity, rejection, trace, or evidence can escape Discovery.
    assert_eq!(
        projection
            .pin_current(source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        generation
    );
    assert!(
        projection
            .resource_at(generation, version_resource)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        lexical
            .retrieve(generation, &request, &LexicalQuery::new("Base", 10))
            .await
            .unwrap()
            .len(),
        1
    );
    let after = service.discover(request).await.unwrap();
    assert!(after.qualified_resources.is_empty());
    assert!(after.rejected_candidates.is_empty());
    assert!(after.evidence_set.is_empty());
    assert!(
        after
            .retrieval_trace
            .iter()
            .all(|trace| !trace.contains(&candidate.candidate_id))
    );
    assert!(
        after
            .qualification_trace
            .iter()
            .all(|trace| !trace.contains(&candidate.candidate_id))
    );
    let disclosed = serde_json::to_string(&after).unwrap();
    assert!(!disclosed.contains(&fixture.document_id.as_uuid().to_string()));
    assert!(!disclosed.contains(&version_resource.as_uuid().to_string()));
    assert!(!disclosed.contains(&candidate.candidate_id));
}
