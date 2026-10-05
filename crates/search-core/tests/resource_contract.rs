use search_core::id::{PredicateId, ResourceId, SourceId, UsageProfileId};
use search_core::profile::{DiscoveryProfile, FacetState};
use search_core::resource::{DiscoverableResource, ResourceBody, ResourceIdentity, ResourceKind};
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::usage::UsageProfile;
use std::collections::HashSet;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

#[test]
fn source_discovery_modes_and_enumeration_round_trip_without_collapsing() {
    let mut source = DiscoverableSource::new(
        source_id(),
        "provider",
        EnumerationSemantics::QueryOnly,
        RetentionMode::SessionOnly,
    );
    source.discovery_modes = vec![DiscoveryMode::RemoteQuery, DiscoveryMode::DirectAddress];
    source.resource_types = vec![ResourceKind::Knowledge, ResourceKind::Capability];

    let decoded: DiscoverableSource =
        serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();

    assert_eq!(
        decoded.enumeration_semantics,
        EnumerationSemantics::QueryOnly
    );
    assert_eq!(decoded.discovery_modes, source.discovery_modes);
    assert_eq!(decoded.resource_types, source.resource_types);
}

#[test]
fn no_retention_source_can_still_answer_live_queries() {
    let mut source = DiscoverableSource::new(
        source_id(),
        "provider",
        EnumerationSemantics::QueryOnly,
        RetentionMode::NoRetention,
    );
    source.discovery_modes.push(DiscoveryMode::RemoteQuery);

    assert!(source.supports(DiscoveryMode::RemoteQuery));
    assert_eq!(source.retention_mode, RetentionMode::NoRetention);
}

#[test]
fn all_six_resource_families_keep_distinct_kinds() {
    let cases = [
        (ResourceBody::Knowledge, ResourceKind::Knowledge),
        (ResourceBody::Semantic, ResourceKind::Semantic),
        (ResourceBody::Capability, ResourceKind::Capability),
        (ResourceBody::AgentSkill, ResourceKind::AgentSkill),
        (ResourceBody::Workflow, ResourceKind::Workflow),
        (ResourceBody::Policy, ResourceKind::Policy),
    ];

    let serialized: HashSet<_> = cases
        .iter()
        .map(|(body, expected)| {
            assert_eq!(body.kind(), *expected);
            serde_json::to_string(body).unwrap()
        })
        .collect();
    assert_eq!(serialized.len(), 6);
}

#[test]
fn one_resource_retains_multiple_usage_profile_ids() {
    let identity = ResourceIdentity::new(
        ResourceId::from_uuid(Uuid::from_u128(2)),
        ResourceKind::Capability,
        source_id(),
    );
    let mut resource = DiscoverableResource::new(
        identity,
        ResourceBody::Capability,
        DiscoveryProfile::new("address change capability"),
    );
    let first = UsageProfileId::from_uuid(Uuid::from_u128(3));
    let second = UsageProfileId::from_uuid(Uuid::from_u128(4));
    resource.usage_profile_ids = vec![first, second];

    let decoded: DiscoverableResource =
        serde_json::from_str(&serde_json::to_string(&resource).unwrap()).unwrap();
    assert_eq!(decoded.usage_profile_ids, vec![first, second]);
}

#[test]
fn negative_applicability_stays_separate_from_positive_rules() {
    let mut usage = UsageProfile::new(
        UsageProfileId::from_uuid(Uuid::from_u128(5)),
        "register an address change",
    );
    let positive = PredicateId::from_uuid(Uuid::from_u128(6));
    let negative = PredicateId::from_uuid(Uuid::from_u128(7));
    usage.applicable_when.push(positive);
    usage.not_applicable_when.push(negative);

    let decoded: UsageProfile =
        serde_json::from_str(&serde_json::to_string(&usage).unwrap()).unwrap();
    assert_eq!(decoded.applicable_when, vec![positive]);
    assert_eq!(decoded.not_applicable_when, vec![negative]);
}

#[test]
fn facet_states_remain_four_distinct_states() {
    let states = [
        FacetState::Known("法人".to_string()),
        FacetState::Unknown,
        FacetState::NotApplicable,
        FacetState::Conflict,
    ];
    let serialized: HashSet<_> = states
        .iter()
        .map(|state| serde_json::to_string(state).unwrap())
        .collect();
    assert_eq!(serialized.len(), 4);
    for state in states {
        let decoded: FacetState<String> =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(decoded, state);
    }
}

#[test]
fn source_native_id_does_not_replace_resource_identity() {
    let first_id = ResourceId::from_uuid(Uuid::from_u128(8));
    let second_id = ResourceId::from_uuid(Uuid::from_u128(9));
    let mut first = ResourceIdentity::new(first_id, ResourceKind::Knowledge, source_id());
    let mut second = ResourceIdentity::new(second_id, ResourceKind::Knowledge, source_id());
    first.source_native_id = Some("same-native-label".into());
    second.source_native_id = Some("same-native-label".into());

    assert_ne!(first.resource_id, second.resource_id);
    assert_eq!(first.source_native_id, second.source_native_id);
    assert_ne!(first, second);
}
