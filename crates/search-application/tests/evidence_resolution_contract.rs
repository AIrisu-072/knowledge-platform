use std::collections::BTreeMap;

use search_application::SearchError;
use search_application::ports::{
    AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort, EvidenceResolverPort,
    ResolvedAssertionEvidence, assemble_resource_claims, assess_claim_evidence,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, ProjectionGenerationId, ResourceId, SourceId};
use search_core::predicate::{DecimalValue, TypedValue};
use search_core::projection::ProjectionGenerationKey;
use time::OffsetDateTime;
use uuid::Uuid;

fn source(number: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(number))
}

fn resource(number: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(number))
}

fn claim_id() -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(3))
}

fn generation() -> ProjectionGenerationKey {
    generation_for(1)
}

fn generation_for(source_number: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source(source_number),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(4)),
    }
}

fn selector() -> ClaimSelector {
    ClaimSelector {
        claim_id: claim_id(),
        subject_ref: "resource-subject".into(),
        predicate: "supports".into(),
        expected_value: Some(TypedValue::Bool(true)),
    }
}

fn assertion(value: bool, refs: &[&str]) -> Assertion {
    let mut assertion = Assertion::new(
        "resource-subject",
        "supports",
        TypedValue::Bool(value),
        "source-native-label",
        AssertionOrigin::Declared,
        "scope",
        OffsetDateTime::UNIX_EPOCH,
    );
    assertion.evidence_refs = refs.iter().map(|reference| (*reference).into()).collect();
    assertion
}

fn resolved(
    reference: &str,
    role: EvidenceRole,
    upstream_origin: &str,
) -> ResolvedAssertionEvidence {
    resolved_for(generation(), reference, role, upstream_origin)
}

fn resolved_for(
    pinned: ProjectionGenerationKey,
    reference: &str,
    role: EvidenceRole,
    upstream_origin: &str,
) -> ResolvedAssertionEvidence {
    ResolvedAssertionEvidence {
        generation: pinned,
        source_id: pinned.source_id,
        resource_id: resource(2),
        evidence_ref: reference.into(),
        upstream_origin: upstream_origin.into(),
        role,
        citation_chain: vec!["direct-citation".into()],
        content_digest: Some("sha256:actual-content".into()),
        is_summary: false,
    }
}

struct Assertions(ProjectionGenerationKey, Vec<Assertion>);

impl AssertionStorePort for Assertions {
    fn assertions_for<'a>(
        &'a self,
        pinned: ProjectionGenerationKey,
        resource_ref: ResourceId,
        _predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        assert_eq!(pinned, self.0);
        assert_eq!(resource_ref, resource(2));
        Box::pin(async move { Ok(self.1.clone()) })
    }
}

struct Selectors(ProjectionGenerationKey, Option<ClaimSelector>);

impl ClaimSelectorPort for Selectors {
    fn selector_for<'a>(
        &'a self,
        pinned: ProjectionGenerationKey,
        requested_claim: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        assert_eq!(pinned, self.0);
        assert_eq!(requested_claim, claim_id());
        Box::pin(async move { Ok(self.1.clone()) })
    }
}

struct Evidence(
    ProjectionGenerationKey,
    BTreeMap<String, ResolvedAssertionEvidence>,
);

impl EvidenceResolverPort for Evidence {
    fn resolve<'a>(
        &'a self,
        pinned: ProjectionGenerationKey,
        target: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        assert_eq!(pinned, self.0);
        assert_eq!(target, resource(2));
        Box::pin(async move { Ok(self.1.get(evidence_ref).cloned()) })
    }
}

fn evidence(pinned: ProjectionGenerationKey, records: Vec<ResolvedAssertionEvidence>) -> Evidence {
    Evidence(
        pinned,
        records
            .into_iter()
            .map(|record| (record.evidence_ref.clone(), record))
            .collect(),
    )
}

async fn assemble(
    requirement: &EvidenceRequirement,
    assertions: Vec<Assertion>,
    records: Vec<ResolvedAssertionEvidence>,
) -> Result<Vec<search_core::evidence::Claim>, SearchError> {
    assemble_at(generation(), requirement, assertions, records).await
}

async fn assemble_at(
    pinned: ProjectionGenerationKey,
    requirement: &EvidenceRequirement,
    assertions: Vec<Assertion>,
    records: Vec<ResolvedAssertionEvidence>,
) -> Result<Vec<search_core::evidence::Claim>, SearchError> {
    assemble_resource_claims(
        pinned,
        resource(2),
        requirement,
        &Selectors(pinned, Some(selector())),
        &Assertions(pinned, assertions),
        &evidence(pinned, records),
    )
    .await
}

#[tokio::test]
async fn verified_primary_is_sufficient_and_keeps_provenance() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["ev-1"])],
        vec![resolved(
            "ev-1",
            EvidenceRole::Primary,
            "original-publication",
        )],
    )
    .await
    .unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].state, ClaimState::Supported);
    assert_eq!(claims[0].subject.as_deref(), Some("resource-subject"));
    assert_eq!(claims[0].predicate.as_deref(), Some("supports"));
    assert_eq!(claims[0].evidence_refs.len(), 1);
    assert_eq!(claims[0].evidence_refs[0].source_ref, source(1));
    assert_eq!(
        claims[0].evidence_refs[0].evidence_ref.as_deref(),
        Some("ev-1")
    );
    assert_eq!(
        claims[0].evidence_refs[0].upstream_origin,
        "original-publication"
    );
    assert_eq!(
        claims[0].evidence_refs[0].citation_chain,
        ["direct-citation"]
    );
    assert_eq!(
        claims[0].evidence_refs[0].content_digest.as_deref(),
        Some("sha256:actual-content")
    );
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Sufficient
    );
}

#[tokio::test]
async fn same_opaque_ref_from_two_sources_keeps_separate_source_identity_and_citation() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut first_record = resolved_for(
        generation_for(1),
        "shared-ref",
        EvidenceRole::Primary,
        "origin-1",
    );
    first_record.citation_chain = vec!["citation-from-1".into()];
    let mut second_record = resolved_for(
        generation_for(9),
        "shared-ref",
        EvidenceRole::Primary,
        "origin-9",
    );
    second_record.citation_chain = vec!["citation-from-9".into()];
    let first = assemble_at(
        generation_for(1),
        &requirement,
        vec![assertion(true, &["shared-ref"])],
        vec![first_record],
    )
    .await
    .unwrap();
    let second = assemble_at(
        generation_for(9),
        &requirement,
        vec![assertion(true, &["shared-ref"])],
        vec![second_record],
    )
    .await
    .unwrap();

    let first_evidence = &first[0].evidence_refs[0];
    let second_evidence = &second[0].evidence_refs[0];
    assert_eq!(first_evidence.source_ref, source(1));
    assert_eq!(second_evidence.source_ref, source(9));
    assert_eq!(first_evidence.evidence_ref.as_deref(), Some("shared-ref"));
    assert_eq!(second_evidence.evidence_ref.as_deref(), Some("shared-ref"));
    assert_eq!(first_evidence.citation_chain, ["citation-from-1"]);
    assert_eq!(second_evidence.citation_chain, ["citation-from-9"]);
}

#[tokio::test]
async fn semantically_equal_decimal_selector_and_assertion_support_claim() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut expected = selector();
    expected.expected_value = Some(TypedValue::Decimal(DecimalValue::new(10, 1)));
    let mut observed = assertion(true, &["decimal-ref"]);
    observed.value = TypedValue::Decimal(DecimalValue::new(100, 2));
    let claims = assemble_resource_claims(
        generation(),
        resource(2),
        &requirement,
        &Selectors(generation(), Some(expected)),
        &Assertions(generation(), vec![observed]),
        &evidence(
            generation(),
            vec![resolved(
                "decimal-ref",
                EvidenceRole::Primary,
                "decimal-origin",
            )],
        ),
    )
    .await
    .unwrap();

    assert_eq!(claims[0].state, ClaimState::Supported);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Sufficient
    );
}

#[tokio::test]
async fn unresolved_reference_cannot_support_claim() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["unknown-ref"])],
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert!(claims[0].evidence_refs.is_empty());
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Unresolved
    );
}

#[tokio::test]
async fn summary_or_originless_primary_cannot_support_claim() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut summary = resolved("summary", EvidenceRole::Primary, "original-publication");
    summary.is_summary = true;
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["summary"])],
        vec![summary],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert_ne!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Sufficient
    );

    let claims = assemble(
        &requirement,
        vec![assertion(true, &["originless"])],
        vec![resolved("originless", EvidenceRole::Primary, "  ")],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert_ne!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Sufficient
    );
}

#[tokio::test]
async fn summary_only_contradiction_remains_unresolved() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut summary = resolved(
        "summary",
        EvidenceRole::Contradicting,
        "original-publication",
    );
    summary.is_summary = true;
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["summary"])],
        vec![summary],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Unresolved
    );
}

#[tokio::test]
async fn summary_only_contradiction_beside_direct_primary_remains_unresolved() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut summary = resolved("summary", EvidenceRole::Contradicting, "publisher-b");
    summary.is_summary = true;
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["direct", "summary"])],
        vec![
            resolved("direct", EvidenceRole::Primary, "publisher-a"),
            summary,
        ],
    )
    .await
    .unwrap();

    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert_eq!(claims[0].evidence_refs.len(), 2);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Unresolved
    );
}

#[tokio::test]
async fn verified_value_mismatch_is_conflicted_despite_missing_ref() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    for role in [EvidenceRole::Primary, EvidenceRole::Corroborating] {
        let claims = assemble(
            &requirement,
            vec![assertion(false, &["direct", "missing"])],
            vec![resolved("direct", role, "publisher-a")],
        )
        .await
        .unwrap();

        assert_eq!(claims[0].state, ClaimState::Conflicted);
        assert_eq!(claims[0].evidence_refs.len(), 1);
        assert_eq!(
            assess_claim_evidence(&requirement, &claims).unwrap(),
            EvidenceSufficiency::Conflicted
        );
    }
}

#[tokio::test]
async fn verified_contradiction_remains_conflicted_when_another_ref_is_missing() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["contradiction", "missing"])],
        vec![resolved(
            "contradiction",
            EvidenceRole::Contradicting,
            "original-publication",
        )],
    )
    .await
    .unwrap();

    assert_eq!(claims[0].state, ClaimState::Conflicted);
    assert_eq!(claims[0].evidence_refs.len(), 1);
    assert_eq!(
        claims[0].evidence_refs[0].evidence_ref.as_deref(),
        Some("contradiction")
    );
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Conflicted
    );
}

#[tokio::test]
async fn mismatched_contradiction_binding_does_not_become_conflict() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut invalid = resolved(
        "contradiction",
        EvidenceRole::Contradicting,
        "original-publication",
    );
    invalid.resource_id = resource(99);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["contradiction", "missing"])],
        vec![invalid],
    )
    .await
    .unwrap();

    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert!(claims[0].evidence_refs.is_empty());
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Unresolved
    );
}

#[tokio::test]
async fn copied_evidence_with_one_upstream_origin_is_not_independent() {
    let mut requirement = EvidenceRequirement::new(vec![claim_id()]);
    requirement.minimum_independent_sources = 2;
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["direct", "copy"])],
        vec![
            resolved("direct", EvidenceRole::Primary, "shared-origin"),
            resolved("copy", EvidenceRole::Corroborating, "shared-origin"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].evidence_refs.len(), 2);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Insufficient
    );
}

#[tokio::test]
async fn mismatched_binding_and_assertion_fields_do_not_become_support() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut wrong_binding = resolved("ev-1", EvidenceRole::Primary, "origin");
    wrong_binding.resource_id = resource(99);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["ev-1"])],
        vec![wrong_binding],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);

    let mut wrong_subject = assertion(true, &["ev-1"]);
    wrong_subject.subject_ref = "other-subject".into();
    let mut wrong_predicate = assertion(true, &["ev-1"]);
    wrong_predicate.predicate = "other-predicate".into();
    let claims = assemble(
        &requirement,
        vec![wrong_subject, wrong_predicate],
        vec![resolved("ev-1", EvidenceRole::Primary, "origin")],
    )
    .await
    .unwrap();
    assert_eq!(claims[0].state, ClaimState::Unknown);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Unresolved
    );
}

#[tokio::test]
async fn resolved_identity_must_match_generation_source_resource_and_reference() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let mut variants = Vec::new();
    let mut wrong_generation = resolved("ev-1", EvidenceRole::Primary, "origin");
    wrong_generation.generation.generation_id =
        ProjectionGenerationId::from_uuid(Uuid::from_u128(99));
    variants.push(wrong_generation);
    let mut wrong_source = resolved("ev-1", EvidenceRole::Primary, "origin");
    wrong_source.source_id = source(99);
    variants.push(wrong_source);
    let mut wrong_resource = resolved("ev-1", EvidenceRole::Primary, "origin");
    wrong_resource.resource_id = resource(99);
    variants.push(wrong_resource);
    let mut wrong_reference = resolved("ev-1", EvidenceRole::Primary, "origin");
    wrong_reference.evidence_ref = "different-ref".into();
    variants.push(wrong_reference);

    for record in variants {
        let returned = Evidence(generation(), BTreeMap::from([("ev-1".into(), record)]));
        let claims = assemble_resource_claims(
            generation(),
            resource(2),
            &requirement,
            &Selectors(generation(), Some(selector())),
            &Assertions(generation(), vec![assertion(true, &["ev-1"])]),
            &returned,
        )
        .await
        .unwrap();
        assert_eq!(claims[0].state, ClaimState::Unknown);
        assert_eq!(
            assess_claim_evidence(&requirement, &claims).unwrap(),
            EvidenceSufficiency::Unresolved
        );
    }
}

#[tokio::test]
async fn contradictory_assertion_is_preserved_and_blocks_completion() {
    let requirement = EvidenceRequirement::new(vec![claim_id()]);
    let claims = assemble(
        &requirement,
        vec![assertion(true, &["yes"]), assertion(false, &["no"])],
        vec![
            resolved("yes", EvidenceRole::Primary, "original-a"),
            resolved("no", EvidenceRole::Contradicting, "original-b"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].value, Some(TypedValue::Bool(true)));
    assert_eq!(claims[1].value, Some(TypedValue::Bool(false)));
    assert_eq!(claims[1].state, ClaimState::Conflicted);
    assert_eq!(
        assess_claim_evidence(&requirement, &claims).unwrap(),
        EvidenceSufficiency::Conflicted
    );
}

#[tokio::test]
async fn empty_completion_requirement_is_rejected() {
    let requirement = EvidenceRequirement::new(vec![]);
    assert!(matches!(
        assemble(&requirement, vec![], vec![]).await,
        Err(SearchError::InvalidRequest(_))
    ));
    assert!(matches!(
        assess_claim_evidence(&requirement, &[]),
        Err(SearchError::InvalidRequest(_))
    ));
}
