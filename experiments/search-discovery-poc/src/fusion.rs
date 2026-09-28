//! Rank-only fusion reference; backend scores are retained for trace, never added.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankedCandidate {
    pub resource_id: String,
    pub raw_score: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrieverList {
    pub id: String,
    pub candidates: Vec<RankedCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FusionCase {
    pub id: String,
    pub expected_resource_id: String,
    pub retrievers: Vec<RetrieverList>,
}

#[derive(Debug, Clone, Copy)]
pub enum FusionStrategy {
    FirstRetrieverOnly,
    PriorityConcat,
    Rrf { k: usize },
}

#[derive(Debug, Clone, Serialize)]
pub struct FusionTrace {
    pub retriever_id: String,
    pub rank: usize,
    pub raw_score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FusedCandidate {
    pub resource_id: String,
    pub fusion_score: f64,
    pub trace: Vec<FusionTrace>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FusionMetrics {
    pub case_count: usize,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub p50_ms: f64,
}

pub fn rank_fuse(retrievers: &[RetrieverList], strategy: FusionStrategy) -> Vec<FusedCandidate> {
    match strategy {
        FusionStrategy::FirstRetrieverOnly | FusionStrategy::PriorityConcat => {
            let selected = match strategy {
                FusionStrategy::FirstRetrieverOnly => &retrievers[..retrievers.len().min(1)],
                _ => retrievers,
            };
            let mut seen = BTreeSet::new();
            let mut ordered = Vec::new();
            for retriever in selected {
                for (index, candidate) in retriever.candidates.iter().enumerate() {
                    if seen.insert(candidate.resource_id.clone()) {
                        ordered.push(FusedCandidate {
                            resource_id: candidate.resource_id.clone(),
                            fusion_score: 1.0 / (ordered.len() + 1) as f64,
                            trace: vec![FusionTrace {
                                retriever_id: retriever.id.clone(),
                                rank: index + 1,
                                raw_score: candidate.raw_score,
                            }],
                        });
                    }
                }
            }
            ordered
        }
        FusionStrategy::Rrf { k } => {
            let mut candidates: BTreeMap<String, FusedCandidate> = BTreeMap::new();
            for retriever in retrievers {
                let mut seen_in_retriever = BTreeSet::new();
                for (index, candidate) in retriever.candidates.iter().enumerate() {
                    if !seen_in_retriever.insert(&candidate.resource_id) {
                        continue;
                    }
                    let rank = index + 1;
                    let fused = candidates
                        .entry(candidate.resource_id.clone())
                        .or_insert_with(|| FusedCandidate {
                            resource_id: candidate.resource_id.clone(),
                            fusion_score: 0.0,
                            trace: Vec::new(),
                        });
                    fused.fusion_score += 1.0 / (k + rank) as f64;
                    fused.trace.push(FusionTrace {
                        retriever_id: retriever.id.clone(),
                        rank,
                        raw_score: candidate.raw_score,
                    });
                }
            }
            let mut ordered = candidates.into_values().collect::<Vec<_>>();
            ordered.sort_by(|left, right| {
                right
                    .fusion_score
                    .total_cmp(&left.fusion_score)
                    .then(left.resource_id.cmp(&right.resource_id))
            });
            ordered
        }
    }
}

pub fn evaluate_fusion(cases: &[FusionCase], strategy: FusionStrategy) -> FusionMetrics {
    let mut found = 0usize;
    let mut reciprocal_sum = 0.0;
    let mut ndcg_sum = 0.0;
    let mut samples = Vec::new();
    for case in cases {
        let mut ranked = Vec::new();
        for _ in 0..100 {
            let started = Instant::now();
            ranked = rank_fuse(&case.retrievers, strategy);
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        if let Some(index) = ranked
            .iter()
            .take(10)
            .position(|candidate| candidate.resource_id == case.expected_resource_id)
        {
            found += 1;
            reciprocal_sum += 1.0 / (index + 1) as f64;
            ndcg_sum += 1.0 / ((index + 2) as f64).log2();
        }
    }
    samples.sort_by(f64::total_cmp);
    let divisor = cases.len().max(1) as f64;
    FusionMetrics {
        case_count: cases.len(),
        recall_at_10: found as f64 / divisor,
        mrr: reciprocal_sum / divisor,
        ndcg_at_10: ndcg_sum / divisor,
        p50_ms: samples.get(samples.len() / 2).copied().unwrap_or_default(),
    }
}
