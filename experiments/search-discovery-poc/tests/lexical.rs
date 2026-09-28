use search_discovery_poc::lexical::{AnalyzerKind, LexicalCase, LexicalResource, evaluate_lexical};

fn fixtures() -> (Vec<LexicalResource>, Vec<LexicalCase>) {
    (
        serde_json::from_str(include_str!("../fixtures/lexical/resources.json")).unwrap(),
        serde_json::from_str(include_str!("../fixtures/lexical/queries.json")).unwrap(),
    )
}

#[test]
fn exact_and_alias_cases_are_recoverable_in_top_ten() {
    let (resources, cases) = fixtures();
    let result = evaluate_lexical(AnalyzerKind::TantivyDefault, &resources, &cases).unwrap();
    for case in cases.iter().filter(|case| case.mandatory) {
        let outcome = result
            .cases
            .iter()
            .find(|outcome| outcome.case_id == case.id)
            .unwrap();
        for expected in &case.expected_resource_ids {
            assert!(
                outcome.retrieved_ids.contains(expected),
                "{} failed to retrieve {}",
                case.id,
                expected,
            );
        }
    }
    assert!(result.measurement.index_bytes > 0);
    assert!(result.measurement.recall_at_10 > 0.0);
}

#[test]
fn hard_discriminators_filter_confusable_hits_after_lexical_retrieval() {
    let (resources, cases) = fixtures();
    let result = evaluate_lexical(AnalyzerKind::TantivyDefault, &resources, &cases).unwrap();
    let broad = result
        .cases
        .iter()
        .find(|outcome| outcome.case_id == "ambiguous-loan")
        .unwrap();
    assert!(broad.retrieved_ids.contains(&"loan-personal".to_owned()));
    assert!(broad.qualified_ids.contains(&"loan-corp".to_owned()));
    assert!(!broad.qualified_ids.contains(&"loan-personal".to_owned()));
    for (case, outcome) in cases.iter().zip(&result.cases) {
        for forbidden in &case.forbidden_resource_ids {
            assert!(!outcome.qualified_ids.contains(forbidden), "{}", case.id);
        }
    }
}

#[test]
fn lindera_ipadic_candidate_preserves_mandatory_exact_and_alias_cases() {
    let (resources, cases) = fixtures();
    let result = evaluate_lexical(AnalyzerKind::LinderaIpadic, &resources, &cases).unwrap();
    for case in cases.iter().filter(|case| case.mandatory) {
        let outcome = result
            .cases
            .iter()
            .find(|outcome| outcome.case_id == case.id)
            .unwrap();
        for expected in &case.expected_resource_ids {
            assert!(
                outcome.retrieved_ids.contains(expected),
                "Lindera {} failed to retrieve {}",
                case.id,
                expected,
            );
        }
        for forbidden in &case.forbidden_resource_ids {
            assert!(!outcome.qualified_ids.contains(forbidden), "{}", case.id);
        }
    }
}
