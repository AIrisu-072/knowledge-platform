//! E (2026-10-06): the pre-registered Vector re-measurement with the pinned
//! E5 model on Candle CPU. The public MIRACL ja dev lane calibrates the
//! similarity floor `tau` on its 40 calibration queries and evaluates LD
//! against the production lexical arm on 100 untouched queries (G3/G4). The
//! frozen synthetic lane then runs L / LG / D / LD / LGD with that `tau` in
//! the planner's S1 order, now Lexical → HyperGraph → Vector (G1/G2/G5/G6).
//! The gates are fixed in docs/superpowers/programs/search-platform-production/plan.md.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use search_vector_model_poc::arms::{build_index, dense_stage, planned_ldg_order, run};
use search_vector_model_poc::embed::{Embedder, ModelId, TextKind};
use search_vector_model_poc::public::{
    self, Dense, Lexical, LexicalMode, bootstrap, concat, ndcg_at_10, recall_at_10,
};
use search_vector_poc::corpus::{Corpus, SCALES, SEED};
use search_vector_poc::metrics::score_run;
use search_vector_poc::run::{ACTOR, Arm, Harness, QueryRun, pinned, run_arm};

const WINDOW: usize = 20;
const K: usize = 10;

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

fn p95(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)]
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn verdict(pass: bool) -> &'static str {
    if pass { "PASS" } else { "FAIL" }
}

struct PublicResult {
    tau: f32,
    g3: bool,
    g4: bool,
}

fn public_lane(root: &std::path::Path, embedder: &Embedder) -> Result<PublicResult, String> {
    let cache = std::env::var("VECTOR_CACHE").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/knowledge-platform-vector")
    });
    let lane = public::load(&cache.join("public-ja-lane.json"))?;
    let _ = root;
    let started = Instant::now();
    let lexical = Lexical::build(&lane.passages)?;
    let dense = Dense::build(embedder, &lane.passages)?;
    println!(
        "public passages={} queries={} build_ms={:.0}",
        lane.passages.len(),
        lane.queries.len(),
        ms(started.elapsed())
    );
    let mut queries = Vec::new();
    for query in &lane.queries {
        let embedded = embedder.embed(&[query.text.as_str()], TextKind::Query)?;
        queries.push((query, embedded.normalized[0].clone()));
    }
    let none = BTreeSet::new();
    let relevant = |id: &str| lane.relevant.get(id).cloned().unwrap_or_default();

    // Calibration: the first 20 calibration queries as asked, the next 20
    // with their positives removed (no answer exists in the corpus).
    let calibration: Vec<_> = queries.iter().filter(|(q, _)| q.split == "calibration").collect();
    let (positive_cal, empty_cal) = calibration.split_at(20);
    let lexical_fp = mean(
        &empty_cal
            .iter()
            .map(|(q, _)| {
                lexical
                    .search(&q.text, LexicalMode::Production, K, &relevant(&q.id))
                    .map(|hits| hits.len() as f64)
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut tau = 1.0_f32;
    for step in 0..=50 {
        let candidate = 0.70 + step as f32 * 0.005;
        let mut fps = Vec::new();
        for (q, vector) in empty_cal {
            let removed = relevant(&q.id);
            let l = lexical.search(&q.text, LexicalMode::Production, K, &removed)?;
            let d = dense.search(vector, candidate, K, &removed);
            fps.push(concat(&[&l, &d], K).len() as f64);
        }
        let fp = mean(&fps);
        let mut recalls = Vec::new();
        for (q, vector) in positive_cal {
            let l = lexical.search(&q.text, LexicalMode::Production, K, &none)?;
            let d = dense.search(vector, candidate, K, &none);
            recalls.push(recall_at_10(&concat(&[&l, &d], K), &relevant(&q.id)));
        }
        println!(
            "calibrate tau={candidate:.3} recall@10={:.4} no_answer_fp@10={fp:.2} lexical_fp@10={lexical_fp:.2}",
            mean(&recalls)
        );
        if fp <= lexical_fp + 1.0 {
            tau = candidate;
            break;
        }
    }
    println!("calibrated tau={tau:.3}");

    // Evaluation on the 100 untouched queries.
    let evaluation: Vec<_> = queries.iter().filter(|(q, _)| q.split == "evaluation").collect();
    let mut arms: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut fp: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut latency = Vec::new();
    for (q, vector) in &evaluation {
        let gold = relevant(&q.id);
        let started = Instant::now();
        let fresh = embedder.embed(&[q.text.as_str()], TextKind::Query)?;
        let _ = dense.search(&fresh.normalized[0], tau, K, &none);
        latency.push(ms(started.elapsed()));
        let prod = lexical.search(&q.text, LexicalMode::Production, K, &none)?;
        let bm25 = lexical.search(&q.text, LexicalMode::Bm25, K, &none)?;
        let bigram = lexical.search(&q.text, LexicalMode::Bigram, K, &none)?;
        let d = dense.search(vector, tau, K, &none);
        for (arm, ranked) in [
            ("L", prod.clone()),
            ("L_bm25", bm25.clone()),
            ("L_bigram", bigram.clone()),
            ("D", d.clone()),
            ("LD", concat(&[&prod, &d], K)),
            ("L_bigram+D", concat(&[&bigram, &d], K)),
        ] {
            arms.entry(arm).or_default().push(ndcg_at_10(&ranked, &gold));
        }
        let lp = lexical.search(&q.text, LexicalMode::Production, K, &gold)?;
        let dp = dense.search(vector, tau, K, &gold);
        fp.entry("L").or_default().push(lp.len() as f64);
        fp.entry("LD").or_default().push(concat(&[&lp, &dp], K).len() as f64);
    }
    for (arm, values) in &arms {
        println!("public arm={arm} ndcg@10={:.4}", mean(values));
    }
    let differences: Vec<f64> = arms["LD"].iter().zip(&arms["L"]).map(|(a, b)| a - b).collect();
    let (gain, low, high) = bootstrap(&differences, 10_000, 0x9e37_79b9_7f4a_7c15);
    let sensitivity: Vec<f64> = arms["L_bigram+D"]
        .iter()
        .zip(&arms["L_bigram"])
        .map(|(a, b)| a - b)
        .collect();
    let (s_gain, s_low, s_high) = bootstrap(&sensitivity, 10_000, 0x9e37_79b9_7f4a_7c15);
    let (fp_l, fp_ld) = (mean(&fp["L"]), mean(&fp["LD"]));
    let g3 = gain >= 0.05 && low > 0.0;
    let g4 = fp_ld <= fp_l + 1.0;
    println!(
        "G3 {} ndcg_gain(LD-L)={gain:.4} ci95=[{low:.4},{high:.4}] sensitivity(bigram+D - bigram)={s_gain:.4} ci95=[{s_low:.4},{s_high:.4}]",
        verdict(g3)
    );
    println!(
        "G4 {} no_answer_fp@10 L={fp_l:.2} LD={fp_ld:.2} public_query_embed_scan_p95_ms={:.2}",
        verdict(g4),
        p95(&mut latency)
    );
    Ok(PublicResult { tau, g3, g4 })
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let model = ModelId::E5;
    let embedder = Embedder::load(model, &root)?;
    println!("seed={SEED}; window={WINDOW}; scales={SCALES:?}; model=e5; S1 order checked from the planner");
    let public = public_lane(&root, &embedder)?;
    let tau = public.tau;

    let (mut g1, mut g2, mut g5) = (true, true, true);
    let mut g6_p95 = 0.0;
    for scale in SCALES {
        let harness = Harness::build(Corpus::synthetic(scale, SEED)?)?;
        let pin = pinned(&harness);
        let order = planned_ldg_order(&harness);
        let kinds: Vec<&str> = order
            .iter()
            .map(|id| id.rsplit(':').next().unwrap_or_default())
            .collect();
        if kinds != ["Lexical", "HyperGraph", "Vector"] {
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
            ("LGD", vec![]),
        ]);
        let mut dense_ms = Vec::new();
        for query in &harness.corpus.queries {
            let started = Instant::now();
            let embedded = embedder.embed(&[query.text.as_str()], TextKind::Query)?;
            let dense = dense_stage(&index, &embedded.normalized[0], &visible, WINDOW, tau);
            let dense_time = started.elapsed();
            dense_ms.push(ms(dense_time));
            let l = lexical.iter().find(|r| r.query_id == query.query_id).ok_or("L run")?;
            let lg = lexical_graph
                .iter()
                .find(|r| r.query_id == query.query_id)
                .ok_or("LG run")?;
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
                vec![("Lexical".into(), stage(l, "Lexical")), ("Vector".into(), dense.clone())],
                &visible,
                l.query_time + dense_time,
            ));
            arms.get_mut("LGD").unwrap().push(run(
                &harness,
                id,
                vec![
                    ("Lexical".into(), stage(lg, "Lexical")),
                    ("HyperGraph".into(), stage(lg, "HyperGraph")),
                    ("Vector".into(), dense),
                ],
                &visible,
                lg.query_time + dense_time,
            ));
        }
        let latency = p95(&mut dense_ms);
        if scale == 1024 {
            g6_p95 = latency;
        }
        let mut scores = BTreeMap::new();
        for (arm, runs) in &arms {
            let score = score_run(runs, &harness.corpus.qrels, &visible)?;
            println!(
                "synthetic scale={scale} arm={arm} ndcg@10={:.4} recall@10={:.4} visible_false_positives={:?} unauthorized_disclosures={:?}",
                score.ndcg_at_10.unwrap_or(f64::NAN),
                score.recall_at[&10],
                score.visible_false_positives,
                score.unauthorized_disclosures,
            );
            scores.insert(*arm, score);
        }
        let ndcg = |arm: &str| scores[arm].ndcg_at_10.unwrap_or(f64::NAN);
        g1 &= ndcg("LGD") >= ndcg("LG") - 0.01;
        let false_positives =
            |arm: &str| scores[arm].visible_false_positives.values().sum::<usize>();
        g2 &= false_positives("LGD") <= false_positives("LG") + 1;
        g5 &= scores
            .values()
            .all(|score| score.unauthorized_disclosures.values().sum::<usize>() == 0);
        println!(
            "synthetic scale={scale} units={} query_embed_scan_p95_ms={latency:.2}",
            index.units.len()
        );
    }
    let g6 = g6_p95 <= 150.0;
    println!("G1 {} (LGD >= LG - 0.01 at every scale)", verdict(g1));
    println!("G2 {} (LGD visible FP <= LG + 1 at every scale)", verdict(g2));
    println!("G3 {}", verdict(public.g3));
    println!("G4 {}", verdict(public.g4));
    println!("G5 {} (no unauthorized disclosure)", verdict(g5));
    println!("G6 {} (p95 {g6_p95:.2} ms at 1024 units)", verdict(g6));
    let all = g1 && g2 && public.g3 && public.g4 && g5 && g6;
    println!("DECISION {} tau={tau:.3}", if all { "DEFAULT_ENABLED" } else { "OPT_IN_ONLY" });
    Ok(())
}
