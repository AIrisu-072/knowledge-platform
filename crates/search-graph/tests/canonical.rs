//! P3-G02: canonical typed n-ary encoding and exact temporal round trip.

use std::collections::BTreeMap;

use search_application::graph_generation::{GraphResourceRecord, GraphSourceMapping};
use search_core::id::{RelationId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use search_core::projection::TemporalProjection;
use search_core::relation::{
    RelationNamespace, RelationParticipant, RelationTemporalScope, TypedRelationInstance,
};
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalDiscoveryProfile;
use search_graph::{
    GRAPH_SCHEMA_VERSION, GraphError, canonical_graph_digest, canonical_relation, decode_temporal,
    encode_temporal,
};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn source(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}

fn at(nanos: i128, offset_hours: i8) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .unwrap()
        .to_offset(UtcOffset::from_hms(offset_hours, 0, 0).unwrap())
}

fn relation(
    qualifier: TypedValue,
    participants: Vec<RelationParticipant>,
) -> TypedRelationInstance {
    let mut relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(500)),
        RelationNamespace::Discovery,
        "placement",
        participants,
    );
    relation.qualifiers = BTreeMap::from([("order".to_owned(), qualifier)]);
    relation
}

fn participants() -> Vec<RelationParticipant> {
    vec![
        RelationParticipant::new("document", rid(10)),
        RelationParticipant::new("folder", rid(11)),
    ]
}

fn temporal(resource: ResourceId, basis: Option<&str>) -> TemporalProjection {
    TemporalProjection {
        resource_ref: resource,
        valid_from: Some(at(1_700_000_000_123_456_789, 9)),
        valid_to: None,
        profile: TemporalDiscoveryProfile {
            freshness_anchor_at: None,
            freshness_basis: basis.map(str::to_owned),
            effective_from: None,
            effective_to: Some(at(1_800_000_000_000_000_001, -5)),
        },
    }
}

fn record(resource: ResourceId, relations: Vec<TypedRelationInstance>) -> GraphResourceRecord {
    GraphResourceRecord {
        resource_ref: resource,
        kind: ResourceKind::Document,
        resource_version_ref: None,
        temporal: temporal(resource, Some("document.published_at")),
        mapping: GraphSourceMapping::Document {
            document_id: resource.as_uuid(),
        },
        attached_relations: relations,
    }
}

fn graph(
    relation: TypedRelationInstance,
) -> (Vec<GraphResourceRecord>, Vec<TypedRelationInstance>) {
    (
        vec![
            record(rid(10), vec![relation.clone()]),
            record(rid(11), vec![relation.clone()]),
        ],
        vec![relation],
    )
}

#[test]
fn temporal_exact_roundtrip_and_digest() {
    let original = temporal(rid(10), Some("document.published_at"));
    let columns = encode_temporal(&original).unwrap();
    let restored = decode_temporal(&columns).unwrap();
    assert_eq!(restored, original);
    assert_eq!(
        restored.valid_from.unwrap().offset(),
        UtcOffset::from_hms(9, 0, 0).unwrap()
    );
    assert_eq!(
        columns.valid_from.unwrap().epoch_nanos,
        1_700_000_000_123_456_789
    );

    // The same instant in another offset is a different stored value and digest.
    let relation = relation(TypedValue::Bool(true), participants());
    let (resources, relations) = graph(relation.clone());
    let mut shifted = resources.clone();
    shifted[0].temporal.valid_from = Some(at(1_700_000_000_123_456_789, 0));
    assert_ne!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap(),
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &shifted, &relations).unwrap()
    );

    // Half-open intervals: an empty or reversed interval is rejected.
    let mut empty = original.clone();
    empty.valid_to = empty.valid_from;
    assert!(matches!(
        encode_temporal(&empty),
        Err(GraphError::Invalid(_))
    ));
    let mut scoped = relation;
    scoped.temporal_scope = RelationTemporalScope {
        valid_from: Some(at(2, 0)),
        valid_to: Some(at(1, 0)),
    };
    assert!(canonical_relation(&scoped).is_err());
}

#[test]
fn typed_value_list_order_changes_digest() {
    let a = TypedValue::String("a".into());
    let b = TypedValue::String("b".into());
    let list_ab = relation(TypedValue::List(vec![a.clone(), b.clone()]), participants());
    let list_ba = relation(TypedValue::List(vec![b.clone(), a.clone()]), participants());
    assert_ne!(
        canonical_relation(&list_ab).unwrap(),
        canonical_relation(&list_ba).unwrap()
    );
    let set_ab = relation(TypedValue::Set(vec![a.clone(), b.clone()]), participants());
    let set_ba = relation(TypedValue::Set(vec![b, a.clone()]), participants());
    assert_eq!(
        canonical_relation(&set_ab).unwrap(),
        canonical_relation(&set_ba).unwrap()
    );
    let duplicate = relation(TypedValue::Set(vec![a.clone(), a]), participants());
    assert!(canonical_relation(&duplicate).is_err());
}

#[test]
fn participant_set_order_does_not_change_digest() {
    let forward = relation(TypedValue::Bool(true), participants());
    let mut reversed = participants();
    reversed.reverse();
    let backward = relation(TypedValue::Bool(true), reversed);
    assert_eq!(
        canonical_relation(&forward).unwrap(),
        canonical_relation(&backward).unwrap()
    );
    let (resources, relations) = graph(forward);
    let (other_resources, other_relations) = graph(backward);
    assert_eq!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap(),
        canonical_graph_digest(
            source(1),
            GRAPH_SCHEMA_VERSION,
            &other_resources,
            &other_relations
        )
        .unwrap()
    );
    // A repeated (role, resource) pair is not a set.
    let mut repeated = participants();
    repeated.push(RelationParticipant::new("document", rid(10)));
    assert!(canonical_relation(&relation(TypedValue::Bool(true), repeated)).is_err());
    // A participant Resource that lacks the attachment, or a diverging copy, fails.
    let original = relation(TypedValue::Bool(true), participants());
    let (mut detached, relations) = graph(original.clone());
    detached[1].attached_relations.clear();
    assert!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &detached, &relations).is_err()
    );
    let (mut diverged, relations) = graph(original);
    diverged[1].attached_relations[0].authority = Some("other".into());
    assert!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &diverged, &relations).is_err()
    );
}

#[test]
fn null_and_empty_are_distinct() {
    let relation = relation(TypedValue::Bool(true), participants());
    let (resources, relations) = graph(relation.clone());
    let mut empty_basis = resources.clone();
    empty_basis[0].temporal.profile.freshness_basis = Some(String::new());
    let mut no_basis = resources.clone();
    no_basis[0].temporal.profile.freshness_basis = None;
    let digest = |resources: &[GraphResourceRecord], relations: &[TypedRelationInstance]| {
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, resources, relations).unwrap()
    };
    assert_ne!(
        digest(&empty_basis, &relations),
        digest(&no_basis, &relations)
    );
    let mut no_authority = relation.clone();
    no_authority.authority = None;
    let mut empty_authority = relation;
    empty_authority.authority = Some(String::new());
    assert_ne!(
        canonical_relation(&no_authority).unwrap(),
        canonical_relation(&empty_authority).unwrap()
    );
}

#[test]
fn different_source_same_ids_change_digest() {
    let (resources, relations) = graph(relation(TypedValue::Bool(true), participants()));
    assert_ne!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap(),
        canonical_graph_digest(source(2), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap()
    );
    assert_ne!(
        canonical_graph_digest(source(1), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap(),
        canonical_graph_digest(source(1), "search-graph-v2", &resources, &relations).unwrap()
    );
}
