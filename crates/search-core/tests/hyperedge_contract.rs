use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::DiscoveryEvaluationId;
use search_core::id::{RelationId, ResourceId};
use search_core::relation::{
    RelationNamespace, RelationParticipant, TypedRelationInstance, matching_relation_ids,
};
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

fn resource(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

fn relation(value: u128, product: ResourceId, collateral: ResourceId) -> TypedRelationInstance {
    let mut relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(value)),
        RelationNamespace::Discovery,
        "loan",
        vec![
            RelationParticipant::new("borrower", resource(1)),
            RelationParticipant::new("product", product),
            RelationParticipant::new("collateral", collateral),
        ],
    );
    relation.authority = Some("finance:authoritative".into());
    relation
}

#[test]
fn product_and_collateral_from_different_relations_never_form_a_composite() {
    let r1 = relation(101, resource(2), resource(10));
    let r2 = relation(102, resource(3), resource(11));
    let pattern = RelationPathPattern::new(
        RelationNamespace::Discovery,
        "loan",
        "product",
        "collateral",
    )
    .with_endpoints(resource(2), resource(11));
    assert!(matching_relation_ids(&[r1, r2], &pattern).is_empty());
}

#[test]
fn three_participant_constraints_must_match_the_same_relation() {
    let r1 = relation(101, resource(2), resource(10));
    let r2 = relation(102, resource(3), resource(11));
    let pattern =
        RelationPathPattern::new(RelationNamespace::Discovery, "loan", "borrower", "product")
            .with_endpoints(resource(1), resource(2))
            .with_participant("collateral", resource(11));
    assert!(matching_relation_ids(&[r1.clone(), r2], &pattern).is_empty());
    let matching =
        RelationPathPattern::new(RelationNamespace::Discovery, "loan", "borrower", "product")
            .with_endpoints(resource(1), resource(2))
            .with_participant("collateral", resource(10));
    assert_eq!(
        matching_relation_ids(&[r1], &matching),
        vec![RelationId::from_uuid(Uuid::from_u128(101))]
    );
}

#[test]
fn participant_order_does_not_change_relation_semantics_but_roles_do() {
    let first = relation(101, resource(2), resource(10));
    let mut reordered = first.clone();
    reordered.participants.reverse();
    assert!(first.same_participants_as(&reordered));

    reordered.participants[0].role = "product".into();
    assert!(!first.same_participants_as(&reordered));
}

#[test]
fn traversal_is_constrained_by_type_roles_scope_access_authority_and_budget() {
    let plan = GraphTraversalPlan {
        seed_nodes: vec![resource(1)],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "loan",
            "borrower",
            "product",
        )],
        allowed_relation_types: vec!["loan".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some("finance:authoritative".into()),
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(99)),
            OffsetDateTime::from_unix_timestamp(300).unwrap(),
            OffsetDateTime::from_unix_timestamp(200).unwrap(),
            "Asia/Tokyo",
        )),
        access_context: "tenant:example".into(),
        expansion_budget: TraversalBudget {
            max_hops: 2,
            max_relations: 10,
            max_branching_per_node: 3,
            max_seed_nodes: 1,
            max_paths: 10,
        },
        stop_conditions: vec![],
    };
    assert!(plan.validate().is_ok());
    assert!(plan.allows(&relation(101, resource(2), resource(10))));
    let mut unrelated = relation(102, resource(3), resource(11));
    unrelated.namespace = RelationNamespace::Evidence;
    assert!(!plan.allows(&unrelated));
}

#[test]
fn unlimited_or_context_free_traversal_is_invalid() {
    let plan = GraphTraversalPlan {
        seed_nodes: vec![resource(1)],
        path_patterns: vec![],
        allowed_relation_types: vec![],
        allowed_namespaces: vec![],
        authority_requirement: None,
        temporal_context: None,
        access_context: String::new(),
        expansion_budget: TraversalBudget {
            max_hops: 0,
            max_relations: 0,
            max_branching_per_node: 0,
            max_seed_nodes: 0,
            max_paths: 0,
        },
        stop_conditions: vec![],
    };
    assert!(plan.validate().is_err());
}
