use search_core::applicability::{
    ApplicabilityState, Discriminator, DiscriminatorImportance, MinimumFactEvidence,
    evaluate_applicability,
};
use search_core::contrast::{ContrastSet, resolve_contrast};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate, RejectedCandidate};
use search_core::evidence::{
    Claim, ClaimState, EvidenceReference, EvidenceRequirement, EvidenceRole, EvidenceSufficiency,
    evaluate_evidence_sufficiency,
};
use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::predicate::{ConceptResolver, Operand, PredicateExpr, TruthValue, TypedValue};
use uuid::Uuid;

struct NoConcepts;
impl ConceptResolver for NoConcepts {
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

fn resource(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

fn candidate() -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        "candidate-1",
        CandidateIdentityClass::DurableResource,
        SourceId::from_uuid(Uuid::from_u128(9)),
        "vector",
    );
    candidate.resource_ref = Some(resource(1));
    candidate
        .matched_signals
        .push("high-vector-similarity".into());
    candidate
}

fn discriminator(importance: DiscriminatorImportance) -> Discriminator {
    Discriminator::new(
        "product.kind",
        importance,
        PredicateExpr::Eq(
            Operand::Fact("product.kind".into()),
            Operand::Value(TypedValue::String("loan".into())),
        ),
    )
}

#[test]
fn hard_mismatch_excludes_even_with_high_similarity_signal() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(TypedValue::String("deposit".into()), FactOrigin::Explicit),
    );
    let result = evaluate_applicability(
        &candidate(),
        &facts,
        &[discriminator(DiscriminatorImportance::Hard)],
        &NoConcepts,
    );
    assert_eq!(result.state, ApplicabilityState::Excluded);
    assert!(
        result
            .reasons
            .iter()
            .any(|reason| reason.contains("product.kind"))
    );
}

#[test]
fn missing_hard_discriminator_is_unresolved_with_blocking_gap() {
    let result = evaluate_applicability(
        &candidate(),
        &FactSet::default(),
        &[discriminator(DiscriminatorImportance::Hard)],
        &NoConcepts,
    );
    assert_eq!(result.state, ApplicabilityState::Unresolved);
    assert_eq!(result.gaps.len(), 1);
    assert!(result.gaps[0].blocking);
    assert_eq!(result.gaps[0].required_fact, "product.kind");
}

#[test]
fn inferred_only_fact_cannot_satisfy_minimum_hard_evidence() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(TypedValue::String("loan".into()), FactOrigin::Inferred),
    );
    let mut rule = discriminator(DiscriminatorImportance::Hard);
    rule.minimum_fact_evidence = MinimumFactEvidence::NotInferred;
    let result = evaluate_applicability(&candidate(), &facts, &[rule], &NoConcepts);
    assert_eq!(result.state, ApplicabilityState::Unresolved);
    assert!(result.gaps[0].blocking);
}

#[test]
fn inferred_fact_on_another_predicate_operand_cannot_satisfy_hard_rule() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(TypedValue::String("loan".into()), FactOrigin::Authoritative),
    );
    facts.insert(
        "customer.segment",
        Fact::new(TypedValue::String("corporate".into()), FactOrigin::Inferred),
    );
    let mut rule = Discriminator::new(
        "product.kind",
        DiscriminatorImportance::Hard,
        PredicateExpr::And(vec![
            PredicateExpr::Eq(
                Operand::Fact("product.kind".into()),
                Operand::Value(TypedValue::String("loan".into())),
            ),
            PredicateExpr::Eq(
                Operand::Fact("customer.segment".into()),
                Operand::Value(TypedValue::String("corporate".into())),
            ),
        ]),
    );
    rule.minimum_fact_evidence = MinimumFactEvidence::NotInferred;
    let result = evaluate_applicability(&candidate(), &facts, &[rule], &NoConcepts);
    assert_eq!(result.state, ApplicabilityState::Unresolved);
    assert_eq!(result.gaps[0].required_fact, "customer.segment");
    assert!(result.gaps[0].blocking);
}

#[test]
fn nonblocking_unknown_survives_in_qualified_resource() {
    let result = evaluate_applicability(
        &candidate(),
        &FactSet::default(),
        &[discriminator(DiscriminatorImportance::Soft)],
        &NoConcepts,
    );
    assert_eq!(result.state, ApplicabilityState::Applicable);
    assert_eq!(result.remaining_nonblocking_unknowns, vec!["product.kind"]);
    let qualified = result
        .qualify(&candidate())
        .expect("nonblocking unknown remains qualified");
    assert_eq!(
        qualified.remaining_nonblocking_unknowns,
        vec!["product.kind"]
    );
}

#[test]
fn contrast_membership_does_not_erase_hard_mismatch() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(TypedValue::String("deposit".into()), FactOrigin::Explicit),
    );
    let set = ContrastSet::new(
        "loan-vs-deposit",
        vec![resource(1), resource(2)],
        vec![discriminator(DiscriminatorImportance::Hard)],
    );
    let result = resolve_contrast(&set, &candidate(), &facts, &NoConcepts);
    assert_eq!(result.state, ApplicabilityState::Excluded);
}

fn claim_id(value: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(value))
}

#[test]
fn conflicting_required_claim_is_not_sufficient() {
    let requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    let claim = Claim::new(claim_id(1), ClaimState::Conflicted);
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[claim]),
        EvidenceSufficiency::Conflicted
    );
}

#[test]
fn duplicate_claim_records_with_different_values_are_conflicted() {
    let requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    let mut first = Claim::new(claim_id(1), ClaimState::Supported);
    first.value = Some(TypedValue::String("loan".into()));
    first.evidence_refs.push(EvidenceReference::new(
        "source-a",
        "publisher-a",
        EvidenceRole::Primary,
    ));
    let mut second = Claim::new(claim_id(1), ClaimState::Supported);
    second.value = Some(TypedValue::String("deposit".into()));
    second.evidence_refs.push(EvidenceReference::new(
        "source-b",
        "publisher-b",
        EvidenceRole::Primary,
    ));
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[first, second]),
        EvidenceSufficiency::Conflicted,
    );
}

#[test]
fn absent_or_missing_required_claim_cannot_be_sufficient() {
    let requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[]),
        EvidenceSufficiency::Unresolved
    );
    let claim = Claim::new(claim_id(1), ClaimState::Absent);
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[claim]),
        EvidenceSufficiency::Insufficient
    );
}

#[test]
fn copied_sources_do_not_count_as_independent_corroboration() {
    let mut requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    requirement.minimum_independent_sources = 2;
    let mut claim = Claim::new(claim_id(1), ClaimState::Supported);
    claim.evidence_refs.push(EvidenceReference::new(
        "source-a",
        "publisher-1",
        EvidenceRole::Primary,
    ));
    claim.evidence_refs.push(EvidenceReference::new(
        "source-b",
        "publisher-1",
        EvidenceRole::Corroborating,
    ));
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[claim]),
        EvidenceSufficiency::Insufficient
    );
}

#[test]
fn unknown_upstream_origin_is_not_independent_corroboration() {
    let mut requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    requirement.minimum_independent_sources = 2;
    let mut claim = Claim::new(claim_id(1), ClaimState::Supported);
    claim.evidence_refs.push(EvidenceReference::new(
        "source-a",
        "publisher-1",
        EvidenceRole::Primary,
    ));
    claim.evidence_refs.push(EvidenceReference::new(
        "source-b",
        "  ",
        EvidenceRole::Corroborating,
    ));
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[claim]),
        EvidenceSufficiency::Insufficient,
    );
}

#[test]
fn equivalent_decimal_claim_values_do_not_conflict() {
    use search_core::predicate::DecimalValue;

    let requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    let mut first = Claim::new(claim_id(1), ClaimState::Supported);
    first.value = Some(TypedValue::Decimal(DecimalValue::new(10, 1)));
    first.evidence_refs.push(EvidenceReference::new(
        "source-a",
        "publisher-a",
        EvidenceRole::Primary,
    ));
    let mut second = Claim::new(claim_id(1), ClaimState::Supported);
    second.value = Some(TypedValue::Decimal(DecimalValue::new(100, 2)));
    second.evidence_refs.push(EvidenceReference::new(
        "source-b",
        "publisher-b",
        EvidenceRole::Corroborating,
    ));
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[first, second]),
        EvidenceSufficiency::Sufficient,
    );
}

#[test]
fn unevaluated_authority_and_freshness_requirements_cannot_be_sufficient() {
    let mut requirement = EvidenceRequirement::new(vec![]);
    requirement
        .authority_requirements
        .push("policy-source".into());
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[]),
        EvidenceSufficiency::Unresolved,
    );
    requirement.authority_requirements.clear();
    requirement.freshness_requirements.push("current".into());
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[]),
        EvidenceSufficiency::Unresolved,
    );
}

#[test]
fn summary_only_evidence_does_not_meet_primary_requirement() {
    let requirement = EvidenceRequirement::new(vec![claim_id(1)]);
    let mut claim = Claim::new(claim_id(1), ClaimState::Supported);
    let mut summary = EvidenceReference::new("summary", "source-a", EvidenceRole::Primary);
    summary.is_summary = true;
    claim.evidence_refs.push(summary);
    assert_eq!(
        evaluate_evidence_sufficiency(&requirement, &[claim]),
        EvidenceSufficiency::Insufficient,
    );
}

#[test]
fn rejected_candidate_retains_qualification_trace() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(TypedValue::String("deposit".into()), FactOrigin::Explicit),
    );
    let candidate = candidate();
    let evaluation = evaluate_applicability(
        &candidate,
        &facts,
        &[discriminator(DiscriminatorImportance::Hard)],
        &NoConcepts,
    );
    let rejected =
        RejectedCandidate::from_evaluation(&candidate, &evaluation).expect("excluded candidate");
    assert_eq!(rejected.candidate_id, "candidate-1");
    assert!(!rejected.reason_trace.is_empty());
}
