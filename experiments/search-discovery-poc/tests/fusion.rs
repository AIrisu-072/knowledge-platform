use search_discovery_poc::fusion::{
    FusionCase, FusionStrategy, eligible_retrievers, evaluate_fusion, rank_fuse,
};
use search_discovery_poc::lexical::{LexicalCase, LexicalResource};
use std::collections::BTreeSet;

fn cases() -> Vec<FusionCase> {
    serde_json::from_str(include_str!("../fixtures/fusion/cases.json")).unwrap()
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
    // Hard eligibility removes every confusable head hit in this fixture.
    // It cannot qualify RRF as a quality improvement over a simpler ordering.
    for metrics in [
        &first,
        &graph_only,
        &vector_like_only,
        &priority,
        &rrf_lexical_graph,
        &rrf,
    ] {
        assert_eq!(metrics.mrr, 1.0);
    }
    assert_eq!(rrf.recall_at_10, 1.0);
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
