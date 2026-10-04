use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::authority::{AuthorityPolicy, AuthorityResolution, resolve_assertions};
use search_core::id::DiscoveryEvaluationId;
use search_core::identity::{
    IdentityEvidence, IdentityEvidenceKind, IdentityState, resolve_identity,
};
use search_core::observation::{
    Coverage, Freshness, Presence, Reachability, ResourceObservation,
    derive_effective_resource_state,
};
use search_core::predicate::{DecimalValue, TruthValue, TypedValue};
use search_core::temporal::{
    TemporalDiscoveryProfile, TemporalEvaluationContext, evaluate_temporal_profile,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn timestamp(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap()
}

fn assertion(value: &str, origin: AssertionOrigin) -> Assertion {
    Assertion::new(
        "resource-1",
        "product.kind",
        TypedValue::String(value.into()),
        "source-1",
        origin,
        "finance",
        timestamp(100),
    )
}

#[test]
fn inferred_assertion_cannot_replace_authoritative_assertion() {
    let mut policy = AuthorityPolicy::default();
    policy.grant_rank(
        "finance",
        "product.kind",
        AssertionOrigin::Authoritative,
        100,
    );
    policy.grant_rank("finance", "product.kind", AssertionOrigin::Inferred, 1);
    let assertions = vec![
        assertion("loan", AssertionOrigin::Authoritative),
        assertion("deposit", AssertionOrigin::Inferred),
    ];
    let resolution = resolve_assertions(
        &assertions,
        &policy,
        "resource-1",
        "product.kind",
        "finance",
    );
    assert_eq!(
        resolution,
        AuthorityResolution::Resolved(TypedValue::String("loan".into()))
    );
}

#[test]
fn equal_authority_conflicting_values_remain_a_conflict() {
    let mut policy = AuthorityPolicy::default();
    policy.grant_rank("finance", "product.kind", AssertionOrigin::Curated, 50);
    let assertions = vec![
        assertion("loan", AssertionOrigin::Curated),
        assertion("deposit", AssertionOrigin::Curated),
        assertion("loan", AssertionOrigin::Curated),
    ];
    let resolution = resolve_assertions(
        &assertions,
        &policy,
        "resource-1",
        "product.kind",
        "finance",
    );
    let AuthorityResolution::Conflict(conflict) = resolution else {
        panic!("equal authority must not be majority-voted away");
    };
    assert_eq!(conflict.values.len(), 2);
}

#[test]
fn equivalent_decimal_assertions_resolve_without_conflict() {
    let mut policy = AuthorityPolicy::default();
    policy.grant_rank("finance", "amount", AssertionOrigin::Curated, 50);
    let mut first = assertion("unused", AssertionOrigin::Curated);
    first.predicate = "amount".into();
    first.value = TypedValue::Decimal(DecimalValue::new(10, 1));
    let mut second = first.clone();
    second.value = TypedValue::Decimal(DecimalValue::new(100, 2));
    assert_eq!(
        resolve_assertions(&[first, second], &policy, "resource-1", "amount", "finance"),
        AuthorityResolution::Resolved(TypedValue::Decimal(DecimalValue::new(10, 1))),
    );
}

#[test]
fn names_and_schema_similarity_do_not_resolve_identity() {
    let weak = [
        IdentityEvidence::new(IdentityEvidenceKind::NameSimilarity, true),
        IdentityEvidence::new(IdentityEvidenceKind::SchemaSimilarity, true),
    ];
    assert_eq!(resolve_identity(&weak), IdentityState::Provisional);
    assert_eq!(resolve_identity(&[]), IdentityState::Unresolved);
    assert_eq!(
        resolve_identity(&[IdentityEvidence::new(
            IdentityEvidenceKind::StableProviderId,
            true
        )]),
        IdentityState::Resolved,
    );
}

#[test]
fn opposing_strong_identity_evidence_is_a_conflict() {
    let evidence = [
        IdentityEvidence::new(IdentityEvidenceKind::StableProviderId, true),
        IdentityEvidence::new(IdentityEvidenceKind::ExplicitDeclaration, false),
    ];
    assert_eq!(resolve_identity(&evidence), IdentityState::Conflict);
}

#[test]
fn explicit_nonidentity_overrides_weak_name_similarity() {
    let evidence = [
        IdentityEvidence::new(IdentityEvidenceKind::ExplicitDeclaration, false),
        IdentityEvidence::new(IdentityEvidenceKind::NameSimilarity, true),
    ];
    assert_eq!(resolve_identity(&evidence), IdentityState::Unresolved);
}

fn observation(at: i64, coverage: Coverage, presence: Presence) -> ResourceObservation {
    ResourceObservation::new(
        "resource-1",
        timestamp(at),
        "enumeration",
        coverage,
        presence,
    )
}

#[test]
fn query_result_miss_leaves_presence_unknown() {
    let result = derive_effective_resource_state(
        "resource-1",
        &[observation(100, Coverage::QueryResult, Presence::Absent)],
    );
    assert_eq!(result.presence, Presence::Unknown);
    assert_eq!(result.reachability, Reachability::Unknown);
}

#[test]
fn complete_enumeration_requires_explicit_omission_observation_for_absence() {
    assert_eq!(
        derive_effective_resource_state("resource-1", &[]).presence,
        Presence::Unknown
    );
    let omitted = observation(100, Coverage::CompleteEnumeration, Presence::Absent);
    assert_eq!(
        derive_effective_resource_state("resource-1", &[omitted]).presence,
        Presence::Absent
    );
    let partial = observation(101, Coverage::PartialEnumeration, Presence::Absent);
    assert_eq!(
        derive_effective_resource_state("resource-1", &[partial]).presence,
        Presence::Unknown
    );
}

#[test]
fn stale_current_guarantee_does_not_invalidate_historical_evidence() {
    let profile = TemporalDiscoveryProfile {
        freshness_anchor_at: Some(timestamp(100)),
        freshness_basis: Some("source observation".into()),
        effective_from: Some(timestamp(50)),
        effective_to: Some(timestamp(150)),
    };
    let context = TemporalEvaluationContext::new(
        DiscoveryEvaluationId::from_uuid(Uuid::nil()),
        timestamp(300),
        timestamp(125),
        "Asia/Tokyo",
    );
    let result = evaluate_temporal_profile(&profile, &context, Some(Duration::seconds(50)));
    assert_eq!(result.freshness, Freshness::Stale);
    assert_eq!(result.effective_at_target, TruthValue::True);
}

#[test]
fn future_freshness_anchor_cannot_be_fresh() {
    let profile = TemporalDiscoveryProfile {
        freshness_anchor_at: Some(timestamp(301)),
        ..Default::default()
    };
    let context = TemporalEvaluationContext::new(
        DiscoveryEvaluationId::from_uuid(Uuid::nil()),
        timestamp(300),
        timestamp(300),
        "Asia/Tokyo",
    );
    let result = evaluate_temporal_profile(&profile, &context, Some(Duration::seconds(50)));
    assert_eq!(result.freshness, Freshness::Unknown);
}

#[test]
fn changed_digest_for_same_remote_version_is_integrity_conflict() {
    let mut first = observation(100, Coverage::DirectLookup, Presence::Present);
    first.remote_version = Some("v1".into());
    first.digest = Some("sha256:a".into());
    let mut second = observation(200, Coverage::DirectLookup, Presence::Present);
    second.remote_version = Some("v1".into());
    second.digest = Some("sha256:b".into());
    let result = derive_effective_resource_state("resource-1", &[first, second]);
    let conflict = result
        .integrity_conflict
        .expect("same version changed digest");
    assert_eq!(conflict.remote_version, "v1");
    assert_eq!(conflict.digests.len(), 2);
}

#[test]
fn observations_from_other_resources_do_not_change_effective_state() {
    let mut unrelated = observation(300, Coverage::DirectLookup, Presence::Absent);
    unrelated.resource_ref = "resource-2".into();
    let result = derive_effective_resource_state(
        "resource-1",
        &[
            observation(100, Coverage::DirectLookup, Presence::Present),
            unrelated,
        ],
    );
    assert_eq!(result.presence, Presence::Present);
}
