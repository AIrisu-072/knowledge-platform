use search_discovery_poc::fusion::{
    FusionCase, FusionStrategy, eligible_retrievers, evaluate_fusion, rank_fuse,
};
use search_discovery_poc::hypergraph::{
    IncidenceIndex, Participant, Relation, TraversalQuery, TraversalStep,
};
use search_discovery_poc::lexical::{AnalyzerKind, LexicalCase, LexicalResource, evaluate_lexical};
use std::collections::BTreeSet;

fn cases() -> Vec<FusionCase> {
    serde_json::from_str(include_str!("../fixtures/fusion/cases.json")).unwrap()
}

fn case_named(id: &str) -> FusionCase {
    cases()
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("missing fusion scenario: {id}"))
}

fn rank_of_expected(case: &FusionCase, strategy: FusionStrategy) -> Option<usize> {
    rank_fuse(&eligible_retrievers(case), strategy)
        .iter()
        .position(|candidate| candidate.resource_id == case.expected_resource_id)
        .map(|position| position + 1)
}

#[test]
fn multiple_hard_eligible_candidates_expose_rrf_help_and_harm() {
    let rescue = case_named("eligible-graph-rescues-lexical-rank");
    let noisy = case_named("eligible-noisy-graph-harms-rrf");
    for case in [&rescue, &noisy] {
        assert!(case.eligible_resource_ids.len() >= 3, "{}", case.id);
        assert!(
            case.eligible_resource_ids
                .contains(&case.expected_resource_id)
        );
    }
    assert_eq!(
        rank_of_expected(&rescue, FusionStrategy::PriorityConcat),
        Some(3)
    );
    assert_eq!(
        rank_of_expected(&rescue, FusionStrategy::Rrf { k: 20 }),
        Some(1)
    );
    assert_eq!(
        rank_of_expected(&noisy, FusionStrategy::PriorityConcat),
        Some(1)
    );
    assert_eq!(
        rank_of_expected(&noisy, FusionStrategy::Rrf { k: 20 }),
        Some(2)
    );
}

#[test]
fn graph_only_relevance_has_a_typed_path_and_an_observed_lexical_miss() {
    let case = case_named("graph-only-corporate-loan");
    let resources: Vec<LexicalResource> =
        serde_json::from_str(include_str!("../fixtures/lexical/resources.json")).unwrap();
    let queries: Vec<LexicalCase> =
        serde_json::from_str(include_str!("../fixtures/lexical/queries.json")).unwrap();
    let query = queries
        .into_iter()
        .find(|query| query.id == case.lexical_query_id)
        .unwrap();
    let lexical = evaluate_lexical(AnalyzerKind::TantivyDefault, &resources, &[query]).unwrap();
    let qualified = &lexical.cases[0].qualified_ids;
    assert!(!qualified.contains(&case.expected_resource_id));
    assert_eq!(
        case.retrievers[0]
            .candidates
            .iter()
            .map(|candidate| &candidate.resource_id)
            .collect::<Vec<_>>(),
        qualified.iter().collect::<Vec<_>>()
    );

    let graph_fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/fusion/graph-relations.json"
    ))
    .unwrap();
    let relations: Vec<Relation> = serde_json::from_str(&graph_fixture).unwrap();
    let index = IncidenceIndex::build(relations).unwrap();
    let path = index
        .traverse(&TraversalQuery {
            seed_resource_id: "company-a".into(),
            steps: vec![TraversalStep {
                namespace: "discovery".into(),
                relation_type: "workflow_for_product".into(),
                from_role: "source".into(),
                to_role: "workflow".into(),
                required_participants: vec![Participant::new("product", "product-b")],
            }],
            max_paths: 4,
            max_branching_per_node: 4,
        })
        .unwrap();
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].relation_ids, vec!["fusion-loan-workflow"]);
    assert_eq!(path[0].target_resource_id, case.expected_resource_id);
    assert_eq!(
        case.retrievers[1]
            .candidates
            .iter()
            .map(|candidate| &candidate.resource_id)
            .collect::<Vec<_>>(),
        path.iter()
            .map(|path| &path.target_resource_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        rank_of_expected(&case, FusionStrategy::FirstRetrieverOnly),
        None
    );
    assert_eq!(
        rank_of_expected(&case, FusionStrategy::PriorityConcat),
        Some(1)
    );
}

#[test]
fn heterogeneous_raw_scores_never_change_rank_only_rrf() {
    let case = cases().remove(0);
    let strategy = FusionStrategy::Rrf { k: 20 };
    let initial = rank_fuse(&case.retrievers, strategy);
    let mut changed = case.retrievers.clone();
    for (retriever_index, retriever) in changed.iter_mut().enumerate() {
        for candidate in &mut retriever.candidates {
            candidate.raw_score = (retriever_index as f64 + 1.0) * 1_000_000_000.0;
        }
    }
    let after = rank_fuse(&changed, strategy);
    assert_eq!(
        initial
            .iter()
            .map(|candidate| &candidate.resource_id)
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|candidate| &candidate.resource_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(initial[0].resource_id, "loan-corp");
    assert_eq!(after[0].resource_id, "loan-corp");
    assert!(
        initial[0]
            .trace
            .iter()
            .any(|trace| trace.raw_score == 12000.0)
    );
}

#[test]
fn hard_applicability_remains_outside_soft_fusion() {
    let case = cases().remove(0);
    let raw = rank_fuse(&case.retrievers, FusionStrategy::PriorityConcat);
    assert!(
        raw.iter()
            .any(|candidate| candidate.resource_id == "loan-personal")
    );
    let eligible = rank_fuse(&eligible_retrievers(&case), FusionStrategy::PriorityConcat);
    assert_eq!(eligible[0].resource_id, "loan-corp");
    assert!(
        eligible
            .iter()
            .all(|candidate| candidate.resource_id != "loan-personal")
    );
}

#[test]
fn fusion_eligibility_matches_lexical_hard_discriminators() {
    let resources: Vec<LexicalResource> =
        serde_json::from_str(include_str!("../fixtures/lexical/resources.json")).unwrap();
    let queries: Vec<LexicalCase> =
        serde_json::from_str(include_str!("../fixtures/lexical/queries.json")).unwrap();
    for case in cases() {
        let query = queries
            .iter()
            .find(|query| query.id == case.lexical_query_id)
            .unwrap();
        let candidate_ids = case
            .retrievers
            .iter()
            .flat_map(|retriever| &retriever.candidates)
            .map(|candidate| candidate.resource_id.as_str())
            .collect::<BTreeSet<_>>();
        let expected = resources
            .iter()
            .filter(|resource| candidate_ids.contains(resource.id.as_str()))
            .filter(|resource| {
                query
                    .required_kind
                    .as_deref()
                    .is_none_or(|kind| resource.kind == kind)
                    && query.required_audience.as_deref().is_none_or(|audience| {
                        resource.audience == audience || resource.audience == "any"
                    })
            })
            .map(|resource| resource.id.as_str())
            .collect::<BTreeSet<_>>();
        let actual = case
            .eligible_resource_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{}", case.id);
    }
}

#[test]
fn evaluation_filters_hard_ineligible_candidates_before_ranking() {
    let case = cases().remove(0);
    let expected = rank_fuse(
        &eligible_retrievers(&case),
        FusionStrategy::FirstRetrieverOnly,
    );
    let result = evaluate_fusion(&[case], FusionStrategy::FirstRetrieverOnly);
    assert_eq!(expected[0].resource_id, "loan-corp");
    assert_eq!(result.mrr, 1.0);
}

#[test]
fn priority_concat_keeps_an_eligible_graph_fallback_when_lexical_misses() {
    let mut case = cases().remove(0);
    case.retrievers[0].candidates.clear();
    let first = evaluate_fusion(&[case.clone()], FusionStrategy::FirstRetrieverOnly);
    let priority = evaluate_fusion(&[case], FusionStrategy::PriorityConcat);
    assert_eq!(first.recall_at_10, 0.0);
    assert_eq!(priority.recall_at_10, 1.0);
}

#[test]
fn strategy_measurements_are_reported_without_universal_slo() {
    let fixtures = cases();
    let first = evaluate_fusion(&fixtures, FusionStrategy::FirstRetrieverOnly);
    let graph_only = evaluate_fusion(
        &fixtures
            .iter()
            .cloned()
            .map(|mut case| {
                case.retrievers = vec![case.retrievers[1].clone()];
                case
            })
            .collect::<Vec<_>>(),
        FusionStrategy::FirstRetrieverOnly,
    );
    let vector_like_only = evaluate_fusion(
        &fixtures
            .iter()
            .cloned()
            .map(|mut case| {
                case.retrievers = vec![case.retrievers[2].clone()];
                case
            })
            .collect::<Vec<_>>(),
        FusionStrategy::FirstRetrieverOnly,
    );
    let priority = evaluate_fusion(&fixtures, FusionStrategy::PriorityConcat);
    let rrf = evaluate_fusion(&fixtures, FusionStrategy::Rrf { k: 20 });
    let lexical_graph = fixtures
        .iter()
        .cloned()
        .map(|mut case| {
            case.retrievers.truncate(2);
            case
        })
        .collect::<Vec<_>>();
    let rrf_lexical_graph = evaluate_fusion(&lexical_graph, FusionStrategy::Rrf { k: 20 });
    // The added cases contain several eligible candidates and one observed
    // lexical miss. They expose both an RRF gain and a noisy-graph regression.
    assert_eq!(fixtures.len(), 8);
    assert_eq!(first.recall_at_10, 7.0 / 8.0);
    assert_eq!(priority.recall_at_10, 1.0);
    assert_eq!(rrf_lexical_graph.recall_at_10, 1.0);
    assert!(rrf_lexical_graph.mrr > priority.mrr);
    assert!(rrf_lexical_graph.mrr < 1.0);
    for (name, metrics) in [
        ("first", first),
        ("graph_only", graph_only),
        ("vector_like_only", vector_like_only),
        ("priority", priority),
        ("rrf_lexical_graph_k20", rrf_lexical_graph),
        ("rrf_k20", rrf),
    ] {
        println!(
            "MEASUREMENT {}",
            serde_json::json!({"strategy":name,"metrics":metrics})
        );
    }
}
