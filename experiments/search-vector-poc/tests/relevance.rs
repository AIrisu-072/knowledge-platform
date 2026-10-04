use std::time::Duration;

use search_vector_poc::corpus::Qrel;
use search_vector_poc::metrics::score_run;
use search_vector_poc::run::QueryRun;
use std::collections::BTreeSet;

fn run(id: &str, ranks: Vec<usize>) -> QueryRun {
    QueryRun {
        query_id: id.into(),
        parent_ranks: ranks,
        stages: vec![],
        query_time: Duration::ZERO,
        fusion_time: Duration::ZERO,
    }
}

#[test]
fn independent_multi_positive_metric_fixture_uses_graded_idcg_and_all_gold_denominators() {
    let gold = vec![
        Qrel {
            query_id: "q".into(),
            resource_index: 0,
            grade: 3,
        },
        Qrel {
            query_id: "q".into(),
            resource_index: 1,
            grade: 2,
        },
        Qrel {
            query_id: "q".into(),
            resource_index: 2,
            grade: 1,
        },
    ];
    let score = score_run(&[run("q", vec![2, 0])], &gold, &BTreeSet::from([0, 1, 2])).unwrap();
    assert!((score.recall_at[&1] - 1.0 / 3.0).abs() < 1e-12);
    assert!((score.recall_at[&5] - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(score.mrr_at_10, Some(1.0));
    let dcg = 1.0 + 7.0 / 3_f64.log2();
    let idcg = 7.0 + 3.0 / 3_f64.log2() + 1.0 / 2.0;
    assert!((score.ndcg_at_10.unwrap() - dcg / idcg).abs() < 1e-12);
}

#[test]
fn no_positive_query_is_separate_and_never_labeled_perfect() {
    let score = score_run(&[run("empty", vec![2, 0])], &[], &BTreeSet::from([0, 2])).unwrap();
    assert_eq!(score.scored_queries, 0);
    assert_eq!(score.no_positive_queries, vec!["empty"]);
    assert!(
        score.recall_at.is_empty(),
        "undefined ratios must not appear as zero"
    );
    assert_eq!(score.mrr_at_10, None);
    assert_eq!(score.ndcg_at_10, None);
    assert_eq!(score.visible_false_positives["empty"], 2);
    assert_eq!(score.unauthorized_disclosures["empty"], 0);
}

#[test]
fn duplicate_parent_rank_or_qrel_is_rejected() {
    let gold = vec![Qrel {
        query_id: "q".into(),
        resource_index: 0,
        grade: 2,
    }];
    assert!(score_run(&[run("q", vec![0, 0])], &gold, &BTreeSet::from([0])).is_err());
    assert!(
        score_run(
            &[run("q", vec![0])],
            &[gold[0].clone(), gold[0].clone()],
            &BTreeSet::from([0])
        )
        .is_err()
    );
    assert!(
        score_run(&[run("empty", vec![0, 0])], &[], &BTreeSet::from([0])).is_err(),
        "no-positive ranks also require deduplication"
    );
}

#[test]
fn no_positive_access_stratum_counts_disclosure_separately_from_visible_false_positive() {
    let score = score_run(
        &[run("restricted", vec![6, 31])],
        &[],
        &BTreeSet::from([31]),
    )
    .unwrap();
    assert_eq!(score.visible_false_positives["restricted"], 1);
    assert_eq!(score.unauthorized_disclosures["restricted"], 1);
    assert_eq!(score.mrr_at_10, None);
    assert!(score.recall_at.is_empty());
}
