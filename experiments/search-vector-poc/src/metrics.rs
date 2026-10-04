use std::collections::{BTreeMap, BTreeSet};

use crate::corpus::Qrel;
use crate::run::QueryRun;

#[derive(Clone, Debug)]
pub struct Score {
    pub scored_queries: usize,
    pub no_positive_queries: Vec<String>,
    pub visible_false_positives: BTreeMap<String, usize>,
    pub unauthorized_disclosures: BTreeMap<String, usize>,
    pub recall_at: BTreeMap<usize, f64>,
    pub mrr_at_10: Option<f64>,
    pub ndcg_at_10: Option<f64>,
}

fn gain(grade: u8) -> f64 {
    (2_u32.pow(grade as u32) - 1) as f64
}
fn dcg(grades: impl IntoIterator<Item = u8>) -> f64 {
    grades
        .into_iter()
        .enumerate()
        .map(|(i, grade)| gain(grade) / ((i + 2) as f64).log2())
        .sum()
}

pub fn score_run(
    runs: &[QueryRun],
    qrels: &[Qrel],
    source_visible_parents: &BTreeSet<usize>,
) -> Result<Score, String> {
    let mut gold: BTreeMap<&str, BTreeMap<usize, u8>> = BTreeMap::new();
    for qrel in qrels {
        if qrel.grade > 3
            || gold
                .entry(&qrel.query_id)
                .or_default()
                .insert(qrel.resource_index, qrel.grade)
                .is_some()
        {
            return Err("duplicate or out-of-range qrel".into());
        }
    }
    let mut seen_queries = BTreeSet::new();
    let mut recall_at = BTreeMap::from([(1, 0.0), (5, 0.0), (10, 0.0), (20, 0.0)]);
    let mut score = Score {
        scored_queries: 0,
        no_positive_queries: vec![],
        visible_false_positives: BTreeMap::new(),
        unauthorized_disclosures: BTreeMap::new(),
        recall_at: BTreeMap::new(),
        mrr_at_10: None,
        ndcg_at_10: None,
    };
    let mut mrr_total = 0.0;
    let mut ndcg_total = 0.0;
    for run in runs {
        if !seen_queries.insert(run.query_id.as_str()) {
            return Err("duplicate query run".into());
        }
        if run
            .parent_ranks
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != run.parent_ranks.len()
        {
            return Err("ranked parents are not deduplicated".into());
        }
        let disclosure_count = run
            .parent_ranks
            .iter()
            .filter(|id| !source_visible_parents.contains(*id))
            .count();
        score
            .unauthorized_disclosures
            .insert(run.query_id.clone(), disclosure_count);
        let grades = gold.get(run.query_id.as_str());
        let positive: BTreeMap<_, _> = grades
            .into_iter()
            .flat_map(|g| g.iter())
            .filter(|(id, grade)| **grade > 0 && source_visible_parents.contains(*id))
            .map(|(id, grade)| (*id, *grade))
            .collect();
        if positive.is_empty() {
            score.no_positive_queries.push(run.query_id.clone());
            score.visible_false_positives.insert(
                run.query_id.clone(),
                run.parent_ranks.len() - disclosure_count,
            );
            continue;
        }
        score.scored_queries += 1;
        for k in [1, 5, 10, 20] {
            let found = run
                .parent_ranks
                .iter()
                .take(k)
                .filter(|id| positive.contains_key(id))
                .count();
            *recall_at.get_mut(&k).unwrap() += found as f64 / positive.len() as f64;
        }
        if let Some(rank) = run
            .parent_ranks
            .iter()
            .take(10)
            .position(|id| positive.contains_key(id))
        {
            mrr_total += 1.0 / (rank + 1) as f64;
        }
        let retrieved_grades = run
            .parent_ranks
            .iter()
            .take(10)
            .map(|id| positive.get(id).copied().unwrap_or(0));
        let observed = dcg(retrieved_grades);
        let mut ideal = positive.values().copied().collect::<Vec<_>>();
        ideal.sort_unstable_by(|a, b| b.cmp(a));
        let denominator = dcg(ideal.into_iter().take(10));
        ndcg_total += observed / denominator;
    }
    if score.scored_queries > 0 {
        for value in recall_at.values_mut() {
            *value /= score.scored_queries as f64;
        }
        score.mrr_at_10 = Some(mrr_total / score.scored_queries as f64);
        score.ndcg_at_10 = Some(ndcg_total / score.scored_queries as f64);
        score.recall_at = recall_at;
    }
    Ok(score)
}
