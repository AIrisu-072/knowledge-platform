use search_vector_poc::corpus::{Corpus, SCALES, SEED};
use search_vector_poc::metrics::score_run;
use search_vector_poc::report::{current_rss_kib, measure_run};
use search_vector_poc::run::{ACTOR, Arm, Harness, export_synthetic_input, pinned, run_arm};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|arg| arg == "--export-synthetic-input")
    {
        if args.len() != 3 {
            return Err("usage: --export-synthetic-input SCALE WINDOW".into());
        }
        let scale = args[1].parse::<usize>().map_err(|e| e.to_string())?;
        let window = args[2].parse::<usize>().map_err(|e| e.to_string())?;
        let corpus = Corpus::synthetic(scale, SEED)?;
        let export = export_synthetic_input(&corpus, window)?;
        println!(
            "{}",
            serde_json::to_string(&export).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("unsupported baseline command".into());
    }
    println!(
        "seed={SEED}; process_rss_method=ps -o rss= -p PID; index_store=in-memory; query_samples=7 per arm/scale; cold=first query after build, warm=remaining six"
    );
    for scale in SCALES {
        let harness = Harness::build(Corpus::synthetic(scale, SEED)?)?;
        let pin = pinned(&harness);
        let mut arm_runs = Vec::new();
        for arm in [Arm::Lexical, Arm::LexicalGraph] {
            let result = run_arm(&harness, arm, pin, ACTOR, 20).await?;
            let score = score_run(
                &result,
                &harness.corpus.qrels,
                &harness.source_visible_parents(ACTOR),
            )?;
            if score
                .unauthorized_disclosures
                .values()
                .any(|count| *count != 0)
            {
                return Err("unauthorized disclosure in ranked baseline".into());
            }
            println!(
                "scale={scale} arm={} scored={} no_positive={:?} visible_false_positives={:?} unauthorized_disclosures={:?} recall@1={:.4} recall@5={:.4} recall@10={:.4} recall@20={:.4} mrr@10={:.4} ndcg@10={:.4}",
                arm.name(),
                score.scored_queries,
                score.no_positive_queries,
                score.visible_false_positives,
                score.unauthorized_disclosures,
                score.recall_at[&1],
                score.recall_at[&5],
                score.recall_at[&10],
                score.recall_at[&20],
                score.mrr_at_10.unwrap(),
                score.ndcg_at_10.unwrap()
            );
            for query in &result {
                println!(
                    "trace scale={scale} arm={} query={} ranks={:?} stages={:?}",
                    arm.name(),
                    query.query_id,
                    query.parent_ranks,
                    query.stages
                );
            }
            arm_runs.push((arm, result, current_rss_kib()?));
        }
        let update = harness.measure_update()?;
        for (arm, runs, rss_kib) in arm_runs {
            let measurement = measure_run(&runs, &harness.build, &update, rss_kib)?;
            println!(
                "measure scale={scale} arm={} build_lexical_ms={:.3} build_graph_ms={:.3} update_lexical_ms={:.3} update_graph_ms={:.3} update_n=1 query_n={} query_p50_ms={:.3} query_p95_ms={:.3} query_p99_ms={:.3} cold_query_ms={:.3} warm_n={} warm_p50_ms={:.3} warm_p95_ms={:.3} warm_p99_ms={:.3} fusion_n={} fusion_p50_ms={:.3} fusion_p95_ms={:.3} fusion_p99_ms={:.3} rss_kib={} index_disk_bytes={}",
                arm.name(),
                measurement.build_lexical_ms,
                measurement.build_graph_ms,
                measurement.update_lexical_ms,
                measurement.update_graph_ms,
                measurement.query.samples,
                measurement.query.p50_ms,
                measurement.query.p95_ms,
                measurement.query.p99_ms,
                measurement.cold_query_ms,
                measurement.warm_query.samples,
                measurement.warm_query.p50_ms,
                measurement.warm_query.p95_ms,
                measurement.warm_query.p99_ms,
                measurement.fusion.samples,
                measurement.fusion.p50_ms,
                measurement.fusion.p95_ms,
                measurement.fusion.p99_ms,
                measurement.rss_kib,
                measurement.index_disk_bytes
            );
        }
    }
    println!(
        "dense_arms=UNRUN; vector_mock=security_contract_only; full_P1_body_qualification=UNRUN"
    );
    Ok(())
}
