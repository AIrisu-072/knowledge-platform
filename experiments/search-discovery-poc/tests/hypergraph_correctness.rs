use search_discovery_poc::hypergraph::{
    IncidenceIndex, Participant, Relation, TraversalQuery, TraversalStep,
};

fn relations() -> Vec<Relation> {
    serde_json::from_str(include_str!("../fixtures/graph/relations.json")).unwrap()
}

fn loan_step(from: &str, to: &str) -> TraversalStep {
    TraversalStep {
        namespace: "discovery".into(),
        relation_type: "loan".into(),
        from_role: from.into(),
        to_role: to.into(),
        required_participants: Vec::new(),
    }
}

fn query(seed: &str, step: TraversalStep) -> TraversalQuery {
    TraversalQuery {
        seed_resource_id: seed.into(),
        steps: vec![step],
        max_paths: 10,
        max_branching_per_node: 10,
    }
}

#[test]
fn incidence_keys_preserve_relation_identity_and_roles() {
    let index = IncidenceIndex::build(relations()).unwrap();
    assert_eq!(
        index.by_resource("company-a"),
        vec!["r1", "r2", "r3-role-swap"]
    );
    assert_eq!(
        index.by_resource_role("company-a", "borrower"),
        vec!["r1", "r2"]
    );
    assert_eq!(
        index.by_relation_type("loan"),
        vec!["r1", "r2", "r3-role-swap"]
    );
    assert_eq!(
        index.by_resource_type("company-a", "loan"),
        vec!["r1", "r2", "r3-role-swap"]
    );
}

#[test]
fn false_composite_and_role_swap_never_become_one_relation() {
    let index = IncidenceIndex::build(relations()).unwrap();
    let mut exact = loan_step("borrower", "product");
    exact.required_participants = vec![
        Participant::new("product", "product-b"),
        Participant::new("collateral", "property-x"),
    ];
    let found = index.traverse(&query("company-a", exact.clone())).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].relation_ids, vec!["r1"]);
    assert_eq!(found[0].target_resource_id, "product-b");
    assert_eq!(found[0].participants[0], relations()[0].participants);

    exact.required_participants[1] = Participant::new("collateral", "property-y");
    assert!(
        index
            .traverse(&query("company-a", exact))
            .unwrap()
            .is_empty()
    );
    let role_swap = loan_step("borrower", "collateral");
    let found = index.traverse(&query("company-a", role_swap)).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|path| path.relation_ids[0].as_str())
            .collect::<Vec<_>>(),
        vec!["r1", "r2"]
    );
}

#[test]
fn namespace_and_high_degree_budget_are_enforced() {
    let index = IncidenceIndex::build(relations()).unwrap();
    let evidence_step = TraversalStep {
        namespace: "evidence".into(),
        relation_type: "supported_by".into(),
        from_role: "claim".into(),
        to_role: "evidence".into(),
        required_participants: Vec::new(),
    };
    let evidence = index.traverse(&query("product-b", evidence_step)).unwrap();
    assert_eq!(evidence[0].relation_ids, vec!["r4-evidence"]);
    assert_eq!(evidence[0].target_resource_id, "policy-1");

    let semantic_step = TraversalStep {
        namespace: "semantic".into(),
        relation_type: "is_a".into(),
        from_role: "parent".into(),
        to_role: "child".into(),
        required_participants: Vec::new(),
    };
    let mut broad = query("concept-root", semantic_step.clone());
    broad.max_branching_per_node = 2;
    assert!(index.traverse(&broad).is_err());
    broad.steps[0].required_participants = vec![Participant::new("child", "concept-3")];
    let constrained = index.traverse(&broad).unwrap();
    assert_eq!(constrained.len(), 1);
    assert_eq!(constrained[0].relation_ids, vec!["r7"]);
}

#[test]
fn participant_order_permutations_do_not_change_typed_paths() {
    let original = relations();
    let mut reversed = original.clone();
    for relation in &mut reversed {
        relation.participants.reverse();
    }
    reversed.reverse();
    let a = IncidenceIndex::build(original).unwrap();
    let b = IncidenceIndex::build(reversed).unwrap();
    let mut step = loan_step("borrower", "product");
    step.required_participants = vec![Participant::new("collateral", "property-x")];
    let query = query("company-a", step);
    let left = a.traverse(&query).unwrap();
    let right = b.traverse(&query).unwrap();
    assert_eq!(
        left.iter()
            .map(|path| (&path.relation_ids, &path.target_resource_id))
            .collect::<Vec<_>>(),
        right
            .iter()
            .map(|path| (&path.relation_ids, &path.target_resource_id))
            .collect::<Vec<_>>()
    );
}

#[test]
fn multi_step_paths_keep_each_relation_id_in_evidence() {
    let index = IncidenceIndex::build(relations()).unwrap();
    let mut request = query("product-b", loan_step("product", "borrower"));
    request.steps.push(loan_step("borrower", "collateral"));
    let paths = index.traverse(&request).unwrap();
    let cross_relation = paths
        .iter()
        .find(|path| path.target_resource_id == "property-y")
        .unwrap();
    assert_eq!(cross_relation.relation_ids, vec!["r1", "r2"]);
    assert_eq!(
        cross_relation.resource_path,
        vec!["product-b", "company-a", "property-y"]
    );
}
