use search_discovery_poc::fusion::{FusionCase, FusionStrategy, evaluate_fusion, rank_fuse};

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
    let fused = rank_fuse(&case.retrievers, FusionStrategy::PriorityConcat);
    assert!(
        fused
            .iter()
            .any(|candidate| candidate.resource_id == "loan-personal")
    );
    let eligible = fused
        .iter()
        .filter(|candidate| candidate.resource_id != "loan-personal")
        .collect::<Vec<_>>();
    assert_eq!(eligible[0].resource_id, "loan-corp");
}

#[test]
fn strategy_measurements_are_reported_without_universal_slo() {
    let fixtures = cases();
    let first = evaluate_fusion(&fixtures, FusionStrategy::FirstRetrieverOnly);
    let priority = evaluate_fusion(&fixtures, FusionStrategy::PriorityConcat);
    let rrf = evaluate_fusion(&fixtures, FusionStrategy::Rrf { k: 20 });
    assert!(rrf.mrr > priority.mrr);
    assert!(priority.recall_at_10 >= first.recall_at_10);
    assert_eq!(rrf.recall_at_10, 1.0);
    for (name, metrics) in [("first", first), ("priority", priority), ("rrf_k20", rrf)] {
        println!(
            "MEASUREMENT {}",
            serde_json::json!({"strategy":name,"metrics":metrics})
        );
    }
}
