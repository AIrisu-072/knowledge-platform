//! Paired L / LG / D / LD / LDG measurement on the frozen synthetic baseline
//! with both pinned models (P2-03). L and LG are the baseline crate's own
//! `run_arm`; D is the exact dense stage; LD and LDG concatenate the actual
//! stage lists in the planner's S1 order. All arms share one Source pin,
//! actor, window and scorer.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use search_vector_model_poc::arms::{build_index, dense_stage, planned_ldg_order, run};
use search_vector_model_poc::embed::{Embedder, ModelId, TextKind};
use search_vector_poc::corpus::{Corpus, SCALES, SEED};
use search_vector_poc::metrics::score_run;
use search_vector_poc::report::current_rss_kib;
use search_vector_poc::run::{ACTOR, Arm, Harness, QueryRun, pinned, run_arm};

const WINDOW: usize = 20;

fn stage(run: &QueryRun, suffix: &str) -> Vec<usize> {
    run.stages
        .iter()
        .find(|stage| stage.retriever_id.ends_with(suffix))
        .map(|stage| stage.raw_parent_ranks.clone())
        .unwrap_or_default()
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn summary(samples: &mut [f64]) -> (f64, f64) {
    samples.sort_by(f64::total_cmp);
    (samples[samples.len() / 2], samples[samples.len() - 1])
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    println!(
        "seed={SEED}; window={WINDOW}; scales={SCALES:?}; dense=exact cosine over every Unit, top-window Unit hits, Source Read, parent fold; fusion=S1 PriorityConcat in planner order; timings n=7 per arm/scale (descriptive only)"
    );
    let selected: Vec<String> = std::env::args().skip(1).collect();
    for model in ModelId::ALL
        .into_iter()
        .filter(|model| selected.is_empty() || selected.iter().any(|name| name == model.name()))
    {
        let before = current_rss_kib()?;
        let load_start = Instant::now();
        let embedder = Embedder::load(model, &root)?;
        let load = load_start.elapsed();
        println!(
            "model={} load_ms={:.1} rss_before_kib={before} rss_loaded_kib={}",
            model.name(),
            ms(load),
            current_rss_kib()?
        );
        for scale in SCALES {
            let harness = Harness::build(Corpus::synthetic(scale, SEED)?)?;
            let pin = pinned(&harness);
            let order = planned_ldg_order(&harness);
            let kinds: Vec<&str> = order
                .iter()
                .map(|id| id.rsplit(':').next().unwrap_or_default())
                .collect();
            if kinds != ["Lexical", "Vector", "HyperGraph"] {
                return Err(format!("unexpected planner S1 order {order:?}"));
            }
            let visible = harness.source_visible_parents(ACTOR);
            let index = build_index(&harness, &embedder, 32)?;
            let lexical = run_arm(&harness, Arm::Lexical, pin, ACTOR, WINDOW).await?;
            let lexical_graph = run_arm(&harness, Arm::LexicalGraph, pin, ACTOR, WINDOW).await?;
            let mut arms: BTreeMap<&str, Vec<QueryRun>> = BTreeMap::from([
                ("L", lexical.clone()),
                ("LG", lexical_graph.clone()),
                ("D", vec![]),
                ("LD", vec![]),
                ("LDG", vec![]),
            ]);
            let mut dense_ms = Vec::new();
            for query in &harness.corpus.queries {
                let started = Instant::now();
                let embedded = embedder.embed(&[query.text.as_str()], TextKind::Query)?;
                let dense = dense_stage(&index, &embedded.normalized[0], &visible, WINDOW);
                let dense_time = started.elapsed();
                dense_ms.push(ms(dense_time));
                let l = lexical
                    .iter()
                    .find(|run| run.query_id == query.query_id)
                    .ok_or("missing L run")?;
                let lg = lexical_graph
                    .iter()
                    .find(|run| run.query_id == query.query_id)
                    .ok_or("missing LG run")?;
                let id = query.query_id.as_str();
                arms.get_mut("D").unwrap().push(run(
                    &harness,
                    id,
                    vec![("Vector".into(), dense.clone())],
                    &visible,
                    dense_time,
                ));
                arms.get_mut("LD").unwrap().push(run(
                    &harness,
                    id,
                    vec![
                        ("Lexical".into(), stage(l, "Lexical")),
                        ("Vector".into(), dense.clone()),
                    ],
                    &visible,
                    l.query_time + dense_time,
                ));
                arms.get_mut("LDG").unwrap().push(run(
                    &harness,
                    id,
                    vec![
                        ("Lexical".into(), stage(lg, "Lexical")),
                        ("Vector".into(), dense),
                        ("HyperGraph".into(), stage(lg, "HyperGraph")),
                    ],
                    &visible,
                    lg.query_time + dense_time,
                ));
            }
            let (dense_p50, dense_max) = summary(&mut dense_ms);
            println!(
                "measure model={} scale={scale} units={} dense_build_ms={:.1} dense_query_p50_ms={dense_p50:.2} dense_query_max_ms={dense_max:.2} rss_kib={}",
                model.name(),
                index.units.len(),
                ms(index.build),
                current_rss_kib()?
            );
            for (arm, runs) in &arms {
                let score = score_run(runs, &harness.corpus.qrels, &visible)?;
                println!(
                    "score model={} scale={scale} arm={arm} recall@1={:.4} recall@5={:.4} recall@10={:.4} recall@20={:.4} mrr@10={:.4} ndcg@10={:.4} visible_false_positives={:?} unauthorized_disclosures={:?}",
                    model.name(),
                    score.recall_at[&1],
                    score.recall_at[&5],
                    score.recall_at[&10],
                    score.recall_at[&20],
                    score.mrr_at_10.unwrap_or(f64::NAN),
                    score.ndcg_at_10.unwrap_or(f64::NAN),
                    score.visible_false_positives,
                    score.unauthorized_disclosures,
                );
                for query in runs {
                    println!(
                        "trace model={} scale={scale} arm={arm} query={} ranks={:?}",
                        model.name(),
                        query.query_id,
                        query.parent_ranks
                    );
                }
            }
        }
        drop(embedder);
    }
    Ok(())
}
